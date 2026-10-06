import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, vrGrab, vrPull } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "debug_ladder (VR): Many's nerve surface grips and pulls with either hand without bar snapping",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    await game.step({ frames: 5 });
    const { entities } = await game.entities.list({ filter: "Nerve_Ladder" });
    assert.equal(entities.length, 1, "the Many station uses the shipped nerve ladder");
    const nerve = await game.physics.ladder(entities[0].id);
    assert.equal(nerve.model, "nerve_l2");
    assert.deepEqual(nerve.rungs, [], "organic strands are too thick for a rung wrap");
    assert.deepEqual(nerve.rails, []);

    for (const hand of ["left", "right"] as const) {
      await teleportVerified(game, { x: -6.3, y: 1.5, z: 108 });
      await game.step({ frames: 30 });
      const point: [number, number, number] = [-6.7, 2, 108 + (hand === "left" ? 0.35 : -0.35)];
      const hold = await vrGrab(game, hand, point);
      assert.equal(hold.kind, "ladder");
      assert.equal(hold.entity_id, nerve.entity_id);
      assert.ok(Math.abs(hold.point[1] - point[1]) < 0.01, "surface grip preserves the requested height");
      assert.ok(Math.abs(hold.point[2] - point[2]) < 0.01, "surface grip does not snap sideways to a strand");
      const start = await game.player.position();
      await game.input.set(`${hand}_hand.rotation`, [0.7071068, 0, 0, 0.7071068]);
      await game.step({ frames: 4 });
      assert.deepEqual((await game.info()).player.climb.grips, [hold], "twisting retains the same surface hold");
      await vrPull(game, hand, [0, -0.3, 0], 18);
      const raised = await game.player.position();
      assert.ok(raised.y > start.y + 0.2, `surface pull lifts the pawn: ${raised.y - start.y}`);
      await game.input.set(`${hand}_hand.squeeze`, 0);
      await game.step({ frames: 30 });
      assert.equal((await game.info()).player.climb.grips.length, 0, "opening releases the surface");
    }
  },
);

test(
  "many.mis: all four authored nerve ladders retain surface geometry",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "many.mis" });
    await game.step({ frames: 2 });
    const { entities } = await game.entities.list({ filter: "Nerve_Ladder" });
    assert.deepEqual(entities.map((entity) => entity.template_id).sort((a, b) => a! - b!), [211, 645, 842, 843]);
    for (const entity of entities) {
      const nerve = await game.physics.ladder(entity.id);
      assert.equal(nerve.model, "nerve_l2");
      assert.deepEqual(nerve.rungs, [], `many ${entity.template_id}: no artificial rungs`);
      assert.deepEqual(nerve.rails, [], `many ${entity.template_id}: no artificial rails`);
    }
  },
);
