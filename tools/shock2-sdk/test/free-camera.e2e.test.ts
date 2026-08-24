import assert from "node:assert/strict";
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
//   - "the camera holds its pose" fails against a build that returns the pawn
//     pose unconditionally from `Game::render`.
//   - "a level reload re-attaches" fails against the first version of this
//     change, which left `free_camera` untouched in `set_active_scene` and so
//     carried a stale pose (and `detached: true`) into the new scene.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const skip = !e2eEnabled && "set SHOCK2_E2E=1 to run";

test("the free camera is inert until the developer option enables it", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
    port: 8113,
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

test("a detached camera holds its pose while the player walks", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
    port: 8114,
  });
  await game.step({ frames: 30 });

  await game.devParams.set("free_camera", 1);
  await game.input.trigger("ToggleFreeCamera");
  await game.step({ frames: 2 });

  const camera = await game.camera.state();
  assert.ok(camera.position, "expected a captured camera pose");
  const playerBefore = await game.player.position();

  // Walk the player away. The camera must not follow: it is a render-layer
  // override, and the simulation underneath it is untouched.
  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames: 90 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 3 });

  const after = await game.camera.state();
  assert.ok(after.position);
  for (let axis = 0; axis < 3; axis++) {
    assert.ok(
      Math.abs(after.position[axis] - camera.position[axis]) < 1e-3,
      `camera drifted on axis ${axis}: ${camera.position} -> ${after.position}`,
    );
  }

  // ...and the player really did move, so the camera holding still is a
  // decoupled camera rather than a frozen simulation.
  const playerAfter = await game.player.position();
  const walked = Math.hypot(
    playerAfter.x - playerBefore.x,
    playerAfter.z - playerBefore.z,
  );
  assert.ok(walked > 1, `expected the pawn to walk away (moved ${walked})`);
});

test("a level reload re-attaches the camera", { skip }, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
    port: 8115,
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
