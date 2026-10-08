import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const [template, name, action, material, emission] of [
  [-24, "rapier", "EquipElectroShock", "ND-rapier_b.psd", 1],
  [-28, "shard", "EquipCrystalShard", "ND-shard.psd", 0.12],
] as const) {
  for (const vr of [true, false]) {
    test(`${vr ? "VR" : "flat"} ${name} has blade emission and a held point light`, { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 30 });
      if (vr) {
        const [item] = await game.entities.byTemplate(template);
        assert.ok(item);
        await aimVrHandAt(game, item.position, 0.2, 1);
        await game.step({ frames: 5 });
        assert.equal((await game.info()).player.right_hand_entity_id, item.id);
        await game.input.set("right_hand.position", [0, 1.4, -0.5]);
      } else {
        await game.player.spawnItem(template);
        await game.input.trigger(action);
      }
      await game.step({ frames: 60 });
      const scene = await game.scene.objects();
      const blades = scene.objects.filter(o => o.material_name?.toLowerCase() === material.toLowerCase());
      assert.ok(blades.length, `rendered ${material} must be identifiable`);
      assert.ok(blades.every(o => Math.abs((o.emissivity ?? -1) - emission) < 1e-5));
      if (name === "rapier") {
        assert.ok(scene.objects.filter(o => o.material_name?.toLowerCase() === "nd-rapier.psd").every(o => o.emissivity === 0), "the hilt must not glow");
      }
      const points = scene.carried_lights!.filter(l => l.kind === "point");
      assert.equal(points.length, 1);
      const player = (await game.info()).player;
      assert.ok(Math.hypot(...points[0]!.position.map((x, i) => x - player.position[i]!)) < 3, "light must be near the wielded weapon, not its old world pickup position");
      if (vr) {
        await game.input.set("right_hand.position", [0.25, 1.4, -0.5]);
        await game.step({ frames: 30 });
        const moved = (await game.scene.objects()).carried_lights!.find(l => l.kind === "point")!;
        assert.ok(Math.hypot(...moved.position.map((x, i) => x - points[0]!.position[i]!)) > 0.05, "light must follow the held weapon");
        await game.input.set("right_hand.squeeze", 0);
        await game.step({ frames: 10 });
        assert.equal((await game.scene.objects()).carried_lights!.filter(l => l.kind === "point").length, 0, "released weapons must not retain a held light");
      }
    });
  }
}
