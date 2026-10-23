import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";
import { holdDebugMug } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
const METERS_PER_WORLD_UNIT = 0.3048 * 2.5;

/** A tracked right hand in metres, palm down, fingers forward (-Z): an
 * XR_EXT_hand_tracking joint set. `pinch` brings the thumb and index tips
 * together; `left` mirrors it into a left hand. */
function trackedHand(origin: Vec3, { pinch = false, left = false } = {}): Vec3[] {
  const joints: Vec3[] = [
    [0, 0, -0.05], [0, 0, 0],
    [-0.02, -0.01, -0.02], [-0.045, -0.015, -0.045], [-0.06, -0.015, -0.07], [-0.07, -0.015, -0.09],
  ];
  for (const x of [-0.025, -0.008, 0.008, 0.024]) {
    for (const z of [-0.02, -0.09, -0.13, -0.155, -0.175]) joints.push([x, -0.01 * Math.abs(z), z]);
  }
  if (pinch) joints[10] = joints[5]!;
  return joints.map(([x, y, z]) => [
    origin[0] + (left ? -x : x) / METERS_PER_WORLD_UNIT,
    origin[1] + y / METERS_PER_WORLD_UNIT,
    origin[2] + z / METERS_PER_WORLD_UNIT,
  ]);
}

const distance = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

test(
  "grip pose tool: a set-down controller keeps the item; a pinch saves the tracked pose",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const mug = await holdDebugMug(game);
    const mugPosition = async () =>
      (await game.entities.list()).entities.find((e) => e.id === mug.id)!.position;
    const held = await mugPosition();
    const mugDraws = async () => (await game.scene.objects({ entityId: mug.id })).objects.length;
    const drawnOnce = await mugDraws();

    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 2 });
    // Set the controller down: no squeeze, somewhere else entirely.
    const hand = (await game.input.state()).right_hand.position;
    await game.input.setMany({
      "right_hand.squeeze": 0,
      "right_hand.position": [hand[0] + 1, hand[1] - 0.5, hand[2]],
    });
    await game.step({ frames: 10 });
    let grip = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
    assert.equal(grip.entity_id, mug.id, "the posing hand keeps the mug");
    assert.ok(distance(await mugPosition(), held) < 1e-3, "the mug stays where it was held");
    assert.equal(await mugDraws(), drawnOnce * 4, "the mug is drawn again in three extra views");

    // Form the grip with the tracked right hand, then pinch the left.
    await game.input.setMany({
      "right_hand.joints": trackedHand(hand),
      "left_hand.joints": trackedHand([hand[0] - 0.5, hand[1], hand[2]], { left: true }),
    });
    await game.step({ frames: 12 });
    await game.input.set(
      "left_hand.joints",
      trackedHand([hand[0] - 0.5, hand[1], hand[2]], { left: true, pinch: true }),
    );
    await game.step({ frames: 2 });

    const path = join(data.root, "vr-grip-overrides.json");
    assert.ok(existsSync(path), "the capture is saved on the device");
    const saved = JSON.parse(readFileSync(path, "utf8")) as {
      entries: { model: string; hand: string; authored: boolean; grip: { bones?: unknown[] } }[];
    };
    assert.equal(saved.entries.length, 1);
    assert.equal(saved.entries[0]!.model, "mug");
    assert.equal(saved.entries[0]!.hand, "right");
    assert.equal(saved.entries[0]!.grip.bones?.length, 24, "a captured grip holds bone rotations");
    grip = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
    assert.ok(grip.grip && "bones" in grip.grip, "the capture applies at once");
    assert.ok(distance(await mugPosition(), held) < 1e-3, "capturing does not move the mug");

    // Finishing hands the mug back to the controller, which no longer grips.
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 3 });
    assert.notEqual((await game.info()).player.right_hand_entity_id, mug.id);
  },
);
