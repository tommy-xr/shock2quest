import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, vrGrab, vrPull, type Vec3 } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const { mission, basin } of [
  { mission: "debug_rec_pool", basin: [16, -6.8, -231.1] as Vec3 },
  { mission: "debug_many_pool", basin: [41.3, 8.8, 46] as Vec3 },
]) {
  for (const vr of [false, true]) {
    test(`${mission} (${vr ? "VR" : "flat"}): empty pool retains water, gaze swimming and reset`, {
      skip: !e2eEnabled, timeout: 600_000,
    }, async () => {
      await using game = await GameServer.launch({ mission, debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 2 });
      const initial = await game.info();
      assert.equal(initial.mission, mission);
      assert.ok(initial.entity_count < 20, "omit mission objects and separately instantiated room scripts");
      assert.deepEqual((await game.entities.list()).entities.filter(e => e.template_id > 0), [],
        "no authored mission objects should remain");
      const spawn = await game.player.position();
      await game.step({ frames: 30 });
      assert.ok(Math.abs((await game.player.position()).y - spawn.y) < 0.05,
        "the preset must spawn in water without intersecting the pool wall");

      for (const pitch of [-45, 0, 45]) {
        await game.player.teleport({ x: basin[0], y: basin[1], z: basin[2] });
        await game.input.set("head.look", [0, pitch]);
        await game.step({ frames: 1 });
        const before = await game.player.position();
        await game.input.set("right_hand.thumbstick", [0, 0.15]);
        await game.step({ frames: 20 });
        await game.input.set("right_hand.thumbstick", [0, 0]);
        const after = await game.player.position();
        const dy = after.y - before.y;
        assert.ok(pitch === 0 ? Math.abs(dy) < 0.03 : dy * Math.sign(-pitch) > 0.15,
          `pitch ${pitch} must steer swimming vertically: ${dy}`);
      }

      await game.input.trigger("DebugReloadLevel");
      await game.step({ frames: 2 });
      assert.equal((await game.info()).mission, mission, "reset must reload the empty preset");
      assert.deepEqual(await game.player.position(), spawn, "reset returns to the pool spawn");
    });
  }
}

for (const hand of ["left", "right"] as const) {
  test(`debug_rec_pool (VR): hold the rim with ${hand}, then pull onto the deck`, {
    skip: !e2eEnabled, timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_rec_pool", debugFlags: ["--vr"] });
    await game.step({ frames: 2 });
    const initial = await game.info();
    assert.equal(initial.mission, "debug_rec_pool");
    assert.ok(initial.entity_count < 20, "the pool scene must omit mission entities");
    const start = await game.player.position();
    await game.step({ frames: 30 });
    assert.ok(Math.abs((await game.player.position()).y - start.y) < 0.05,
      "the real water cells must keep an idle swimmer afloat");

    const hold = await vrGrab(game, hand, [11.95, -5.15, -231.1]);
    assert.equal(hold.kind, "ledge");
    await game.step({ frames: 30 });
    assert.equal((await game.info()).player.climb.vaulting, false, "grabbing alone must not vault");
    assert.ok(Math.abs((await game.player.position()).y - start.y) < 0.05);

    // Same gesture as a ladder: lower the closed hand, with no jump or stick.
    await vrPull(game, hand, [0, -0.45, 0], 30);
    await game.step({ frames: 90 });
    const after = await game.info();
    assert.ok(after.player.position[0] < 11.9 && after.player.position[1] > -4.1,
      `the pull must finish on the deck, got ${after.player.position}`);
    assert.equal(after.player.climb.vaulting, false);
    assert.equal(after.player.climb.grips.length, 0, "top-out releases the hand hold");
  });
}

test("debug_many_pool (VR): swim to the sloped rim and exit with the stick", {
  skip: !e2eEnabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_many_pool", debugFlags: ["--vr"] });
  await game.step({ frames: 2 });
  await game.input.lookAtWorldPoint([41.3, 10.7, 30]);
  await game.input.hold("LeftHandLowerButton");
  await game.input.set("right_hand.thumbstick", [0, 0.2]);
  await game.step({ frames: 210 });
  await game.input.release("LeftHandLowerButton");
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 30 });
  const end = await game.player.position();
  assert.ok(end.y > 11.5 && end.z < 40.9, `must stand on the dry rim, got ${JSON.stringify(end)}`);
});
