import type { PlayedSound } from "../../src/types.js";

// Shared reads of the played-sound log (GET /v1/audio/recent) - the only
// headless way to observe audio. Snapshot the last `sequence` before an
// action, then filter what arrived after it.

/** The value of one schema tag on a played sound, if it carries that tag. */
export function tagValue(sound: PlayedSound, tag: string): string | undefined {
  return sound.tags.find(([t]) => t === tag)?.[1];
}

/** Impact/collision-schema sounds (event=collision) played after `sequence`. */
export function collisionSoundsSince(
  sounds: PlayedSound[],
  sequence: number,
): PlayedSound[] {
  return sounds.filter(
    (s) => s.sequence > sequence && tagValue(s, "event") === "collision",
  );
}

/** Compact sample+tags rendering for assertion messages. */
export function describeSounds(sounds: PlayedSound[]): string {
  return JSON.stringify(sounds.map((s) => ({ sample: s.sample, tags: s.tags })));
}
