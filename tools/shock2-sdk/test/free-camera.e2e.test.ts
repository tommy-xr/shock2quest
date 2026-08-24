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

test("flying the camera moves it and leaves the pawn standing", { skip }, async () => {
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
