import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { selectPsiPower } from "./helpers/psi.js";

// Every Pyro tier adds a distinct Contact source down the gamesys hierarchy:
// 7 at PSI 1, then +2 per level. Cryo similarly starts at 4 and adds 1.
// The existing melee-scene arachnid has 60 HP, avoiding lethal-hit clamping,
// and authored Annelid Vulnerability: Incendiary x2, Cold x1. Each case
// starts a fresh low-stat character because provisioning only raises stats.
for (const vr of [false, true]) {
  for (const psi of [1, 2, 3]) {
    for (const power of ["Pyrokinesis", "Cryokinesis"]) {
      test(`${power} PSI ${psi} preserves inherited contact sources (${vr ? "VR" : "flat"})`, {
        skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
      }, async () => {
        await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: vr ? ["--vr"] : [] });
        await game.step({ frames: 10 });
        await game.player.setStats({ psionic_ability: psi });
        if (vr) await game.input.set("right_hand.squeeze", 1);
        const amp = await game.player.spawnItem(-247, vr ? { hand: "right" } : {});
        if (!vr) await game.input.trigger("EquipPsiAmp");
        await game.step({ frames: 10 });
        assert.equal(vr ? (await game.info()).player.right_hand_entity_id : (await game.info()).player.wielded_entity_id, amp.entity_id);
        await selectPsiPower(game, power);
        const [target] = await game.entities.byTemplate(-1439);
        await game.player.teleport({ x: -6.5, y: 1, z: 6 });
        await game.step({ frames: 2 });
        assert.ok(target);
        const aim = await game.player.aimAt(target, { hitbox: "torso", visibility: "required" });
        if (vr) {
          const player = (await game.info()).player;
          const distance = Math.hypot(...aim.world_point.map((value, axis) => value - player.position[axis]! - (axis === 1 ? player.camera_offset[1] : 0)));
          await aimVrHandAt(game, aim.world_point, Math.max(0.5, distance - 0.3), 1);
        }
        await game.step({ frames: 1 });
        const hp = async () => Number((await game.entities.detail(target.id)).properties.find(p => p.name === "HitPoints")?.value);
        const startHp = await hp();
        assert.equal(startHp, 60);
        assert.equal((await game.info()).player.stats?.psionic_ability, psi);
        const beforePsi = (await game.info()).player.psi_points!;
        await game.input.set("right_hand.trigger", 1);
        await game.step({ frames: 1 });
        await game.input.set("right_hand.trigger", 0);
        await game.step({ frames: 3 });
        assert.equal((await game.info()).player.psi_points, beforePsi - (power === "Pyrokinesis" ? 3 : 1));
        await game.step({ frames: 60 });
        const damage = power === "Pyrokinesis" ? 2 * (5 + 2 * psi) : 3 + psi;
        assert.equal(await hp(), startHp - damage, "contact damage sums every authored source through the target receptron");
        await game.step({ frames: 120 });
        assert.equal(await hp(), startHp - damage, "incendiary contact does not invent an unauthored ongoing burn");
      });
    }
  }
}
