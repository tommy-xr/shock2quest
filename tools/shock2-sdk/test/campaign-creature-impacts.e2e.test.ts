import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import type { Vec3 } from "../src/types.js";
import { ammoOf, pullTrigger } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) for (const fixture of [
  { mission: "hydro1.mis", filter: "Sammy", template: 139, spang: "grubspang", particles: 13 },
  { mission: "medsci1.mis", filter: "Slug Turret", template: 611, spang: "Standard Hit Spang", particles: 7 },
]) {
  test(`campaign ${fixture.filter} gets its material burst without a wall decal (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: fixture.mission, debugFlags: vr ? ["--vr"] : [] });
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      await game.step({ frames: 5 });
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 5 });
      if (vr) {
        const gun = (await game.entities.list({ filter: "Pistol", limit: 30 })).entities.find(e => e.template_id === -17);
        assert.ok(gun);
        await aimVrHandAt(game, gun.position, 0.3);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
      }
      const info = await game.info();
      const gunId = vr ? info.player.right_hand_entity_id : info.player.wielded_entity_id;
      assert.ok(gunId !== null);
      const target = (await game.entities.list({ filter: fixture.filter, limit: 30 }))
        .entities.find(e => e.template_id === fixture.template);
      assert.ok(target, "find the native campaign target by stable mission identity");
      await game.player.teleport({ x: target.position[0] - 2, y: target.position[1] + 1, z: target.position[2] });
      await game.step({ frames: 15 });
      // The shared aim API resolves live torso proxies and stationary turret surfaces.
      const aim = await game.player.aimAt(target.id, { hitbox: "torso" });
      if (vr) {
        const snapshot = await game.info();
        const p = snapshot.player.position;
        const eye: Vec3 = [p[0], p[1] + snapshot.player.camera_offset[1], p[2]];
        const distance = Math.hypot(...aim.world_point.map((v, i) => v - eye[i]));
        await aimVrHandAt(game, aim.world_point, distance - 0.4, 1);
      }
      await game.step({ frames: 3 });
      const hpBefore = Number((await game.entities.detail(target.id)).properties.find(p => p.name === "HitPoints")?.value);
      assert.ok(Number.isFinite(hpBefore), "target has authored hit points");
      const ammo = ammoOf(await game.entities.detail(gunId));
      await pullTrigger(game);
      assert.equal(ammoOf(await game.entities.detail(gunId)), ammo - 1);
      const hpAfter = Number((await game.entities.detail(target.id)).properties.find(p => p.name === "HitPoints")?.value);
      assert.ok(hpAfter < hpBefore, "the cosmetic replacement preserves projectile damage");
      const spang = (await game.entities.list({ filter: "Spang", limit: 50 }))
        .entities.find(e => e.name === fixture.spang);
      assert.ok(spang, `real target hit creates ${fixture.spang}`);
      const particles = (await game.scene.objects({ entityId: spang.id })).objects
        .filter(o => o.source === "particle");
      assert.equal(particles.length, fixture.particles, "material-specific particles reach the renderer");
      assert.ok(!(await game.scene.objects({ limit: 10000 })).objects.some(o =>
        /^ND-(mtlhit|pcrhit)/i.test(o.model ?? "")), "actor hits must not receive world-surface bullet holes");
      await game.step({ frames: 120 });
      assert.equal((await game.scene.objects({ entityId: spang.id })).objects.length, 0);
      assert.ok(!(await game.entities.list({ filter: "Spang", limit: 50 })).entities.some(e => e.id === spang.id));
    });
}
