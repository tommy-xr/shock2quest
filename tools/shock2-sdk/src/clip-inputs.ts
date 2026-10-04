import { writeFile } from "node:fs/promises";

import type { Game } from "./game.js";

/**
 * A clip's controller inputs, written as `<clip>.inputs.json` next to its mp4
 * so the website can pose its 3D Touch controllers in sync with the video.
 *
 * - `duration`: seconds of simulation recorded (60 Hz fixed step).
 * - `parts`: per controller part, `[t, value]` keyframes in ascending `t`
 *   (seconds; `t = 0` is the video's first frame). A value holds until the next
 *   keyframe; before the first keyframe a part is at rest (0, or `[0, 0]`).
 *
 * Part keys are `<hand>.<part>`, hand `L`/`R`, part one of:
 * `stick` (`[x, y]`, +y forward), `click` (stick click), `trigger`, `grip`,
 * `upper` (Y/B), `lower` (X/A), `menu` (left only); button values are 0..1.
 */
export interface ClipInputs {
  format: "shock2quest-clip-inputs/1";
  duration: number;
  parts: Record<string, InputKeyframe[]>;
}

export type PartValue = number | [number, number];
export type InputKeyframe = [number, PartValue];

/** How long a discrete action (an edge, not a level) shows as pressed. */
export const PRESS_SECONDS = 0.2;

const SIM_HZ = 60;

/**
 * Controller part a level channel drives (`input.set`), or null for one that
 * is not a Touch control: tracking (`*.position`, `*.rotation`,
 * `*.world_target`, `head.*`), `pointer.*`, flat-only `lean`, the hand-agnostic
 * `jump` and the debug-only `<hand>_hand.a`.
 */
export function channelPart(channel: string): string | null {
  const match = /^(left|right)_hand\.(thumbstick|trigger|trigger_value|squeeze|squeeze_value)$/.exec(channel);
  if (!match) return null;
  const hand = match[1] === "left" ? "L" : "R";
  const part = { thumbstick: "stick", trigger: "trigger", trigger_value: "trigger", squeeze: "grip", squeeze_value: "grip" }[match[2]];
  return `${hand}.${part}`;
}

/** Controller button a discrete action is (its Quest Touch binding), or null. */
export function actionPart(action: string): string | null {
  return (
    {
      LeftHandLowerButton: "L.lower",
      LeftHandUpperButton: "L.upper",
      RightHandLowerButton: "R.lower",
      RightHandUpperButton: "R.upper",
      MenuButton: "L.menu",
    }[action] ?? null
  );
}

/**
 * Records every controller input a capture script sends through `game.input`
 * (`set`, `trigger`, `hold`, `release`), stamped with the simulation time
 * counted from `game.step` results. Create it before the clip's first
 * `game.step`, or call `start()` there; `write()` saves the timeline.
 */
export class ClipInputRecorder {
  /** Sim frames stepped since the recorder was created. */
  private frame = 0;
  private origin = 0;
  /** Per part: keyframes in absolute seconds since creation. */
  private readonly tracks = new Map<string, InputKeyframe[]>();
  private crouch = 0;
  private readonly restore: () => void;

  constructor(game: Game) {
    const { input } = game;
    const original = { set: input.set, trigger: input.trigger, hold: input.hold, release: input.release, step: game.step };
    input.set = async (channel, value) => {
      await original.set.call(input, channel, value);
      this.onChannel(channel, value);
    };
    input.trigger = async (action) => {
      const result = await original.trigger.call(input, action);
      const part = actionPart(action);
      if (part) this.pulse(part);
      return result;
    };
    input.hold = async (action) => {
      const result = await original.hold.call(input, action);
      const part = actionPart(action);
      if (part) this.level(part, 1);
      return result;
    };
    input.release = async (action) => {
      const result = await original.release.call(input, action);
      const part = actionPart(action);
      if (part) this.level(part, 0);
      return result;
    };
    game.step = async (spec) => {
      const result = await original.step.call(game, spec);
      this.frame += result.frames_advanced;
      return result;
    };
    this.restore = () => {
      const { step, ...inputMethods } = original;
      Object.assign(input, inputMethods);
      game.step = step;
    };
  }

  /** Make now the video's first frame; inputs still held carry over to t = 0. */
  start(): void {
    this.origin = this.frame;
  }

  /** Stop recording: `game` goes back to its unwrapped methods. */
  dispose(): void {
    this.restore();
  }

  private get now(): number {
    return this.frame / SIM_HZ;
  }

  private onChannel(channel: string, value: unknown): void {
    // Quest crouch is a left stick click that toggles it.
    if (channel === "crouch") {
      const crouch = Number(value) ? 1 : 0;
      if (crouch !== this.crouch) this.pulse("L.click");
      this.crouch = crouch;
      return;
    }
    const part = channelPart(channel);
    if (!part) return;
    this.level(part, Array.isArray(value) ? [Number(value[0]), Number(value[1])] : Number(value));
  }

  private track(part: string): InputKeyframe[] {
    let track = this.tracks.get(part);
    if (!track) this.tracks.set(part, (track = []));
    return track;
  }

  /** Set a part's value from now on, dropping a pending pulse release. */
  private level(part: string, value: PartValue): void {
    const track = this.track(part);
    while (track.length && track[track.length - 1][0] > this.now) track.pop();
    track.push([this.now, value]);
  }

  private pulse(part: string): void {
    this.level(part, 1);
    this.track(part).push([this.now + PRESS_SECONDS, 0]);
  }

  /** The timeline since `start()` (or creation). */
  timeline(): ClipInputs {
    const origin = this.origin / SIM_HZ;
    const parts: Record<string, InputKeyframe[]> = {};
    for (const [part, track] of this.tracks) {
      const out: InputKeyframe[] = [];
      for (const [t, value] of track) {
        const at = Math.max(0, Math.round((t - origin) * 1e4) / 1e4);
        // A later write at the same instant wins; an unchanged value adds nothing.
        if (out.length && out[out.length - 1][0] === at) out.pop();
        const previous = out.length ? out[out.length - 1][1] : rest(value);
        if (!same(previous, value)) out.push([at, value]);
      }
      if (out.length) parts[part] = out;
    }
    return { format: "shock2quest-clip-inputs/1", duration: (this.frame - this.origin) / SIM_HZ, parts };
  }

  /** Write the timeline, conventionally to `<clip>.inputs.json` beside `<clip>.mp4`. */
  async write(file: string): Promise<void> {
    await writeFile(file, `${JSON.stringify(this.timeline())}\n`);
  }
}

/** Start recording `game`'s controller inputs for a clip. */
export function recordClipInputs(game: Game): ClipInputRecorder {
  return new ClipInputRecorder(game);
}

const rest = (value: PartValue): PartValue => (Array.isArray(value) ? [0, 0] : 0);
const same = (a: PartValue, b: PartValue) =>
  Array.isArray(a) && Array.isArray(b) ? a[0] === b[0] && a[1] === b[1] : a === b;
