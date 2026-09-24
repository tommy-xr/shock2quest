import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

for (const hand of ["left", "right"] as const) {
  test(`physical ${hand} gun follows same-frame locomotion without trailing the player`, {
    skip: process.env.SHOCK2_E2E !== "1",
    timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr", "--experimental", "physical_held_items"],
    });
    await game.step({ frames: 10 });
    const gun = await cycleToWeapon(game, e => e.template_id === -18, { settleFrames: 90 });
    await aimVrHandAt(game, gun.position!, 0.45, 1, 0, { hand });
    await game.step({ frames: 8 });
    const player = (await game.info()).player;
    assert.equal(hand === "right" ? player.right_hand_entity_id : player.wielded_entity_id, gun.id);
    await game.input.set("head.rotation", [0, 0, 0, 1]);
    await game.input.set(`${hand}_hand.position`, [0.25, 0.95, -0.65]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 90 });

    const snapshot = async () => {
      const player = (await game.info()).player;
      const pawn = player.position;
      const draws = (await game.scene.objects({ entityId: gun.id })).objects;
      assert.ok(draws.length > 0, "the held weapon must be rendered");
      return { pawn, rotation: player.rotation, relative: quatRotate(quatConjugate(player.rotation), sub(draws[0]!.position, pawn)) };
    };
    const original = await snapshot();
    await game.input.set(`${hand}_hand.position`, [0.45, 0.95, -0.65]);
    await game.step({ frames: 1 });
    const translated = await snapshot();
    assert.ok(Math.abs(translated.relative[0] - original.relative[0] - 0.2) < 0.002,
      "a controller translation must reach the rendered gun in the same frame");
    await game.input.set(`${hand}_hand.position`, [0.25, 0.95, -0.65]);
    await game.step({ frames: 1 });
    const resting = await snapshot();
    // A fixed controller pose must remain fixed relative to the moving pawn.
    // Reversals also expose an old target that a steady-speed check could miss.
    for (const stick of [[1, 0], [-1, 0], [0, 1], [0, -1], [0, 0]] as [number, number][]) {
      await game.input.set("right_hand.thumbstick", stick);
      let previous = await snapshot();
      let distance = 0;
      for (let frame = 0; frame < 8; frame++) {
        await game.step({ frames: 1 });
        const current = await snapshot();
        distance += Math.hypot(...current.pawn.map((v, i) => v - previous.pawn[i]!));
        const error = Math.hypot(...current.relative.map((v, i) => v - resting.relative[i]!));
        assert.ok(error < 0.002, `stick ${stick}, frame ${frame}: rendered weapon trails pawn by ${error}`);
        previous = current;
      }
      if (stick.some(v => v !== 0)) assert.ok(distance > 0.5, "exercise real unobstructed locomotion");
    }
    const beforeTurn = await snapshot();
    await game.input.set("left_hand.thumbstick", [1, 0]);
    for (let frame = 0; frame < 8; frame++) {
      await game.step({ frames: 1 });
      const current = await snapshot();
      const error = Math.hypot(...current.relative.map((v, i) => v - resting.relative[i]!));
      assert.ok(error < 0.002, `turn frame ${frame}: pawn-local weapon pose error ${error}`);
    }
    await game.input.set("left_hand.thumbstick", [0, 0]);
    const afterTurn = await snapshot();
    assert.ok(Math.hypot(...afterTurn.rotation.map((v, i) => v - beforeTurn.rotation[i]!)) > 0.01,
      "the turn input must actually rotate the pawn");
  });
}
