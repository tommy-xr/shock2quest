import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { ammoOf, pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { tagValue } from "./helpers/audio.js";

const enabled = process.env.SHOCK2_E2E === "1";
// Real MedSci cryo-room surfaces. Assert the live ray material before shooting,
// so a fixture drift cannot silently test an unrelated surface.
const targets: [string, Vec3][] = [
  ["plasticrete", [-34.9676, -3.716, 16.9]],
  ["metal", [-31.18475, -1.2, 24.81524]],
  ["glass", [-35.6364, -3.716, 16.7]],
];

for (const vr of [false, true]) {
  test(`campaign ballistic impacts render material bursts and expire (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [],
      });
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      await game.step({ frames: 5 });
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 5 });
      const pistol = (await game.entities.list({ filter: "Pistol", limit: 30 }))
        .entities.find(e => e.template_id === -17);
      assert.ok(pistol);
      if (vr) {
        await aimVrHandAt(game, pistol.position as Vec3, 0.3);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, pistol.id);
      }
      await game.player.teleport({ x: -35, y: -4.2, z: 21 });
      await game.step({ frames: 30 });
      for (const [material, target] of targets) {
        const snapshot = await game.info();
        const p = snapshot.player.position;
        const eye: Vec3 = [p[0], p[1] + snapshot.player.camera_offset[1], p[2]];
        const end = target.map((v, i) => eye[i] + (v - eye[i]) * 1.01) as Vec3;
        assert.equal((await game.raycast({ start: eye, end,
          collision_groups: ["world", "entity"] })).surface_material, material);
        await game.input.lookAtWorldPoint(target);
        if (vr) {
          const distance = Math.hypot(...target.map((v, i) => v - eye[i]));
          await aimVrHandAt(game, target, distance - 0.4, 1);
        }
        await game.step({ frames: 30 });
        const ammo = ammoOf(await game.entities.detail(pistol.id));
        const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
        await pullTrigger(game);
        assert.equal(ammoOf(await game.entities.detail(pistol.id)), ammo - 1);
        assert.ok((await game.audio.recent()).sounds.some(s =>
          s.sequence > sequence && tagValue(s, "event") === "collision" &&
          // Glass has no matching standard-ammo collision schema and keeps
          // the existing metal sound fallback; the live ray and draw count
          // independently verify its new glass visual routing.
          tagValue(s, "material") === (material === "glass" ? "metal" : material)));
        const spangs = (await game.entities.list({ filter: "Spang", limit: 50 })).entities;
        const impact = spangs.find(e => e.name === "Standard Terr Spang");
        assert.ok(impact, "real projectile must create its terrain effect host");
        const draws = (await game.scene.objects({ entityId: impact.id })).objects
          .filter(o => o.source === "particle");
        assert.equal(draws.length, material === "metal" ? 7 : material === "glass" ? 9 : 8,
          `expected the bounded ${material} burst instead of the generic terrain particles`);
        const decals = (await game.scene.objects({ limit: 10000 })).objects.filter(o =>
          o.model !== null && (material === "metal" ? o.model === "ND-mtlhit0" : /^ND-pcrhit[0-3]$/.test(o.model)));
        assert.ok(decals.length > 0, `expected the material-specific ${material} bullet hole`);
        // PlanarDecal renders a base and separate incidence-shine overlay,
        // rather than the generic material-stack pass list.
        if (material === "metal") {
          assert.equal(decals.length, 2, "metal bullet hole renders base plus shine");
          assert.equal(decals[0].entity_id, decals[1].entity_id);
          assert.ok(decals.every(o => !o.depth_write));
        }
        const decalIds = new Set(decals.map(o => o.entity_id));
        await game.step({ frames: 120 });
        assert.ok((await game.scene.objects({ limit: 10000 })).objects.some(o => decalIds.has(o.entity_id)),
          "bullet holes outlast the short particle burst");
        assert.equal((await game.entities.list({ filter: "Spang", limit: 50 })).entities.length, 0,
          "the burst host expires instead of accumulating");
        assert.equal((await game.scene.objects({ entityId: impact.id })).objects.length, 0);
        await game.step({ frames: 540 });
        assert.ok(!(await game.scene.objects({ limit: 10000 })).objects.some(o => decalIds.has(o.entity_id)),
          "bullet holes retain their authored ten-second lifetime");
      }
    });
}
