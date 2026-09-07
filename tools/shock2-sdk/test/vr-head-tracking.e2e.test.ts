import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const skip = process.env.SHOCK2_E2E !== "1";
const rig = {
  enabled: true,
  head_position: [0, 1.7, 0] as [number, number, number],
  left_hand_position: [-0.2, 1.4, -0.3] as [number, number, number],
  right_hand_position: [0.2, 1.4, -0.3] as [number, number, number],
};

test(
  "VR stage tracking calibrates physical crouch and reports the rendered eye",
  { skip, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_minimal",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 3 });
    const original = await game.info();
    await game.input.set("head.position", [0, 0.2, 0]);
    await game.step({ frames: 3 });
    assert.deepEqual(
      (await game.info()).player.camera_offset,
      original.player.camera_offset,
      "legacy pawn-local head patches do not imply tracked calibration",
    );
    await game.input.setTracking(rig);
    await game.step({ frames: 3 });
    const standing = await game.info();
    assert.equal((await game.input.tracking()).physical_crouch, false);
    await game.input.setTracking({ head_position: [0, 1, 0] });
    await game.step({ frames: 10 });
    const crouched = await game.info();
    const tracking = await game.input.tracking();
    assert.equal(tracking.physical_crouch, true);
    assert.ok(
      standing.player.position[1] - crouched.player.position[1] > 0.3,
      "physical lowering shrinks the actual capsule",
    );
    assert.deepEqual(
      crouched.player.camera_offset,
      tracking.resolved_head_position,
    );
    const inputs = crouched.inputs as {
      hands: { right: { position: [number, number, number] } };
    };
    const relativeHand = inputs.hands.right.position.map(
      (value, axis) => value - crouched.player.camera_offset[axis],
    );
    for (const [axis, meters] of [0.2, 0.4, -0.3].entries()) {
      assert.ok(
        Math.abs(relativeHand[axis] - meters / 0.762) < 1e-4,
        "crouch correction must translate head and hand together",
      );
    }
    const eye = crouched.player.camera_offset[1];
    await game.input.setTracking({
      position_tracked: false,
      head_position: [0, 1.7, 0],
    });
    await game.step({ frames: 3 });
    assert.equal((await game.input.tracking()).physical_crouch, true);
    assert.equal(
      (await game.info()).player.camera_offset[1],
      eye,
      "tracking loss retains last valid head pose",
    );
    await game.input.setTracking({ position_tracked: true });
    await game.step({ frames: 10 });
    assert.equal((await game.input.tracking()).physical_crouch, false);
    await game.input.setTracking({ head_position: [0, 1, 0] });
    await game.step({ frames: 3 });
    assert.equal((await game.input.tracking()).physical_crouch, true);
    await game.input.setTracking({ reset: true });
    await game.step({ frames: 3 });
    assert.equal(
      (await game.input.tracking()).physical_crouch,
      false,
      "new reference space calibrates from its first valid pose",
    );
    await game.input.set("crouch", 1);
    await game.step({ frames: 3 });
    assert.equal((await game.input.tracking()).explicit_crouch, true);
    await game.input.setTracking({ enabled: false });
    await game.step({ frames: 3 });
    assert.equal((await game.input.tracking()).enabled, false);
    assert.equal((await game.input.tracking()).explicit_crouch, true);
  },
);

test(
  "flat runtime rejects stage tracking and preserves its camera",
  { skip, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal" });
    await game.step({ frames: 3 });
    const before = (await game.info()).player.camera_offset;
    await assert.rejects(game.input.setTracking(rig), /requires --vr/);
    await game.input.set("head.position", [0, 0.2, 0]);
    await game.step({ frames: 3 });
    assert.deepEqual((await game.info()).player.camera_offset, before);
  },
);

test(
  "tracked stand-up remains headroom-gated and keeps the rendered rig below the ceiling",
  { skip, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 3 });
    await game.input.setTracking(rig);
    await game.step({ frames: 3 });
    await game.input.setTracking({ head_position: [0, 1, 0] });
    await game.step({ frames: 10 });
    // Isolated physics fixture, not campaign progression: place the crouched
    // capsule inside the authored five-foot Cryo passage (floor -1.6).
    await teleportVerified(game, { x: -27, y: -0.996, z: -16.7 });
    await game.step({ frames: 10 });
    const before = await game.info();
    const ceiling = await game.raycast({
      start: [-27, -1.55, -16.7],
      end: [-27, 2, -16.7],
      collision_groups: ["world"],
    });
    assert.ok(
      ceiling.hit_point && ceiling.hit_point[1] < 0.5,
      "fixture must be under the authored ceiling",
    );
    await game.input.setTracking({ head_position: [0, 1.7, 0] });
    await game.step({ frames: 15 });
    const blocked = await game.info();
    assert.equal(
      (await game.input.tracking()).physical_crouch,
      false,
      "tracked head asks to stand",
    );
    assert.ok(
      Math.abs(blocked.player.position[1] - before.player.position[1]) < 0.05,
      "actual capsule must stay crouched",
    );
    assert.deepEqual(
      blocked.player.camera_offset,
      (await game.input.tracking()).resolved_head_position,
    );
    assert.ok(
      blocked.player.position[1] + blocked.player.camera_offset[1] <
        ceiling.hit_point![1],
      "actual camera must remain below the ceiling",
    );
    await teleportVerified(game, { x: -23.2, y: -0.996, z: -16.1 });
    await game.step({ frames: 15 });
    assert.ok(
      (await game.info()).player.position[1] - blocked.player.position[1] > 0.3,
      "actual stand-up completes in clear headroom",
    );
  },
);
