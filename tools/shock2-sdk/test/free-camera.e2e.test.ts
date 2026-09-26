import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The free (debug) camera detaches the view from the player without touching
// the simulation. These are the parts that unit tests over `FreeCamera` in
// isolation cannot reach: that the real gate really withholds the toggle, that
// a detached camera really holds its pose while the live player walks, and
// that a scene swap really re-attaches it.
//
// Negative-first, per assertion:
//   - "the gate withholds the toggle" fails if the gate check is removed from
//     `Game::update` (the camera detaches with the developer option off).
//   - "flying moves the camera, not the pawn" fails both ways without this
//     change: with no override the camera does not fly, and without
//     `without_locomotion` the pawn walks off on the same stick.
//   - "a level reload re-attaches" fails against the first version of this
//     change, which left `free_camera` untouched in `set_active_scene` and so
//     carried a stale pose (and `detached: true`) into the new scene.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const skip = !e2eEnabled && "set SHOCK2_E2E=1 to run";

test("the free camera is inert until the developer option enables it", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
  });
  await game.step({ frames: 30 });

  const before = await game.camera.state();
  assert.equal(before.enabled, false, "the gate must default off");
  assert.equal(before.detached, false);

  // Toggling with the gate off must do nothing at all.
  await game.input.trigger("ToggleFreeCamera");
  await game.step({ frames: 2 });
  const gated = await game.camera.state();
  assert.equal(gated.detached, false, "a gated toggle must not detach");
  assert.equal(gated.position, null);

  // With the gate on, the same toggle detaches.
  await game.devParams.set("free_camera", 1);
  await game.input.trigger("ToggleFreeCamera");
  await game.step({ frames: 2 });
  const detached = await game.camera.state();
  assert.equal(detached.detached, true);
  assert.ok(detached.position, "a detached camera reports its pose");

  // Turning the option back off is always a way home, even without the toggle.
  await game.devParams.set("free_camera", 0);
  await game.step({ frames: 2 });
  assert.equal((await game.camera.state()).detached, false);
});

test("flying the camera moves it and leaves the pawn standing", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
  });
  await game.step({ frames: 30 });

  await game.devParams.set("free_camera", 1);
  await game.input.trigger("ToggleFreeCamera");
  await game.step({ frames: 2 });

  const camera = await game.camera.state();
  assert.ok(camera.position, "expected a captured camera pose");
  const playerBefore = await game.player.position();

  // Push forward on the stick. The camera flies; the pawn must NOT walk,
  // because the camera is holding those channels.
  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames: 90 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 3 });

  const after = await game.camera.state();
  assert.ok(after.position);
  const flew = Math.hypot(
    after.position[0] - camera.position[0],
    after.position[2] - camera.position[2],
  );
  assert.ok(flew > 1, `expected the camera to fly (moved ${flew})`);

  const playerAfter = await game.player.position();
  const walked = Math.hypot(
    playerAfter.x - playerBefore.x,
    playerAfter.z - playerBefore.z,
  );
  assert.ok(
    walked < 0.1,
    `the pawn must not walk while the camera flies (moved ${walked})`,
  );
});

test("a level reload re-attaches the camera", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
  });
  await game.step({ frames: 30 });

  await game.devParams.set("free_camera", 1);
  await game.input.trigger("ToggleFreeCamera");
  await game.step({ frames: 2 });
  assert.equal((await game.camera.state()).detached, true);

  // A scene swap: the camera's pose belongs to the outgoing scene, so carrying
  // it over would render the new one from inside its geometry - and strand the
  // player, since the pawn-anchored menu is not visible from a detached camera.
  await game.input.trigger("DebugReloadLevel");
  await game.step({ frames: 120 });

  const after = await game.camera.state();
  assert.equal(after.detached, false, "a scene swap must re-attach the camera");
  assert.equal(after.position, null);
  assert.equal(after.enabled, true, "the developer option itself is not reset");
});

// The write side (POST /v1/camera). What it buys over toggling and flying is
// that a capture can be *aimed*: an agent investigating something near the
// player - a held weapon, the player themselves - can stand the camera off to
// one side and photograph it, which the player-anchored eye cannot do.
//
// Negative-first, per assertion:
//   - "the eye lands where it was asked to" fails without the head
//     compensation: the runtime composes the tracked head offset/rotation onto
//     the camera pose, so an uncompensated placement reports (and renders from)
//     an eye-height above the requested point, yawed by the default view.
//   - "the screenshot differs from the player's" fails without the developer-
//     gate write: `Game::update` re-attaches the camera on the very next step,
//     so the placement is gone by the time the picture is taken and the two
//     images come back byte-identical (stepping is deterministic).
//   - the rejection cases fail if a contradictory request is accepted and
//     silently does half of what was asked.
test("a placed camera renders from where it was put", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
  });
  await game.step({ frames: 30 });

  const playerView = await game.screenshot("free-camera-player-view.png");
  const player = await game.player.position();

  // Stand off to one side and above, aimed back at the player's head: the
  // third-person shot the player's own eye cannot take.
  const eye: [number, number, number] = [
    player.x + 3,
    player.y + 2.5,
    player.z + 3,
  ];
  const target: [number, number, number] = [player.x, player.y + 1, player.z];
  const placed = await game.camera.set({ position: eye, lookAt: target });

  assert.equal(placed.detached, true);
  assert.equal(
    placed.enabled,
    true,
    "placing must turn the gate on, or the next step re-attaches",
  );
  assert.ok(placed.eye_position, "a placed camera reports its eye pose");
  for (const axis of [0, 1, 2]) {
    assert.ok(
      Math.abs(placed.eye_position[axis] - eye[axis]) < 1e-3,
      `eye landed at ${placed.eye_position}, wanted ${eye}`,
    );
  }

  // ...and it is AIMED where it was told. Asserting only the position would
  // pass a look-at that quietly points somewhere else - the composition
  // (look_at -> pose_for_eye -> stored -> eye_for_pose) is what this closes.
  assert.ok(placed.eye_rotation);
  const [w, qx, qy, qz] = placed.eye_rotation;
  // Rotate (0, 0, -1) by the reported quaternion - i.e. negate the rotation
  // matrix's third column - to recover the camera's view direction.
  const forward = [
    -2 * (qx * qz + w * qy),
    -2 * (qy * qz - w * qx),
    -(1 - 2 * (qx * qx + qy * qy)),
  ];
  const toTarget = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
  const length = Math.hypot(...toTarget);
  for (const axis of [0, 1, 2]) {
    assert.ok(
      Math.abs(forward[axis] - toTarget[axis] / length) < 1e-3,
      `camera aimed ${forward}, wanted ${toTarget.map((v) => v / length)}`,
    );
  }

  await game.step({ frames: 2 });
  // The pose survives stepping - the gate is on and nothing recaptured it.
  const held = await game.camera.state();
  assert.equal(held.detached, true);
  assert.ok(held.eye_position);
  assert.ok(
    Math.hypot(
      held.eye_position[0] - eye[0],
      held.eye_position[1] - eye[1],
      held.eye_position[2] - eye[2],
    ) < 1e-3,
    `the camera drifted to ${held.eye_position}`,
  );

  const cameraView = await game.screenshot("free-camera-placed-view.png");
  const fromPlayer = readFileSync(playerView.full_path);
  const fromCamera = readFileSync(cameraView.full_path);
  assert.ok(
    !fromPlayer.equals(fromCamera),
    "the camera view is byte-identical to the player view - the camera never moved",
  );

  // And back: the point of a spectator camera is that it is borrowable.
  const attached = await game.camera.attach();
  assert.equal(attached.detached, false);
  assert.equal(attached.eye_position, null);
});

test("a nonsense camera placement is refused, not half-applied", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
  });
  await game.step({ frames: 30 });

  // Nothing placed yet: a bare aim has no position to aim from.
  await assert.rejects(
    () => game.camera.set({ lookAt: [0, 0, 0] }),
    /position/,
    "aiming an unplaced camera must be refused",
  );
  // Two ways to say the same thing.
  await assert.rejects(
    () =>
      game.camera.set({
        position: [0, 0, 0],
        lookAt: [1, 0, 0],
        rotation: [1, 0, 0, 0],
      }),
    /look_at/,
  );
  // Aiming at the camera's own position is not a direction.
  await assert.rejects(
    () => game.camera.set({ position: [1, 2, 3], lookAt: [1, 2, 3] }),
    /direction/,
  );

  // None of that placed anything - including the developer gate, which is the
  // side effect that would leak from a request applied halfway.
  const state = await game.camera.state();
  assert.equal(state.detached, false);
  assert.equal(state.position, null);
  assert.equal(state.enabled, false, "a refused placement must not open the gate");
});
