import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { tagValue } from "./helpers/audio.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";
import { aimVrHandAt, quatFromTo } from "./helpers/vr-hand.js";

type Case = { weapon: string; weaponTemplate: number; setting: number; ammoIndex: number;
  projectile: number; projectileName: string; clip: number | null; ammoCost: number; burst: number };
// Snapshot of authored routing/costs from cargo dq weapon-audit, deliberately
// independent of the runtime's selection implementation.
const cases: Case[] = JSON.parse(readFileSync(new URL("../../test/fixtures/weapon-audit.json", import.meta.url), "utf8"));
const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) for (const row of cases) {
  test(`${vr ? "VR" : "flat"} audit: ${row.weapon} setting ${row.setting} ${row.projectileName}`,
    { skip: !enabled || (!!process.env.WEAPON_AUDIT_PRESENTATION && process.env.WEAPON_AUDIT_PRESENTATION !== (vr ? "vr" : "flat")), timeout: 180_000,
      todo: row.weaponTemplate === -27 ? "Homing script crashes on initialization" : undefined }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
    try {
    await game.step({ frames: 5 });
    await game.player.setStats({ skills: { standard_weapons: 6, energy_weapons: 6, heavy_weapons: 6, exotic_weapons: 6 } });
    const gun = await cycleToWeapon(game, e => e.template_id === row.weaponTemplate, { settleFrames: vr ? 90 : 5 });
    if (vr) {
      await aimVrHandAt(game, gun.position, 0.45, 1, 0, { hand: "right" });
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
      await game.input.set("right_hand.position", [0, 1, -2]);
      await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], [-1, 0, 0]));
      await game.step({ frames: 3 });
    }
    if (row.setting) {
      await game.input.trigger("CycleGunSetting");
      await game.step({ frames: 2 });
    }
    if (row.ammoIndex) {
      await game.input.trigger("EjectClip");
      await game.step({ frames: 2 });
      for (let index = 0; index < row.ammoIndex; index++) {
        await game.input.trigger("CycleAmmo");
        await game.step({ frames: 2 });
      }
      assert.ok(row.clip !== null);
      await game.player.spawnItem(row.clip);
      await game.input.trigger("Reload");
      await game.step({ frames: 180 });
    }
    if (ammoOf(await game.entities.detail(gun.id)) < row.ammoCost && row.clip !== null) {
      await game.player.spawnItem(row.clip);
      await game.input.trigger("Reload");
      await game.step({ frames: 180 });
    }
    for (let wait = 0; (await game.info()).player.reloading && wait < 20; wait++) {
      await game.step({ frames: 60 });
    }
    assert.equal((await game.info()).player.reloading, false, "fixture must finish reloading");
    const beforeAmmo = ammoOf(await game.entities.detail(gun.id));
    assert.ok(beforeAmmo >= row.ammoCost, "fixture must load enough ammunition");
    const before = new Set((await game.entities.list({limit: 300})).entities.map(e => e.id));
    const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    const spawned = (await game.entities.list({limit: 300})).entities.filter(e => !before.has(e.id));
    const rounds = spawned.filter(e => e.template_id === row.projectile);
    const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > sequence);
    // Fast projectiles can resolve before the first post-trigger snapshot.
    // The creation applier records their actual template on the firing gun.
    const last = (await game.entities.detail(gun.id)).properties.find(p => p.name === "LastFiredProjectile");
    assert.equal(Number(last?.value), row.projectile, "actual projectile template matches the authored link");
    const spent = beforeAmmo - ammoOf(await game.entities.detail(gun.id));
    assert.ok(spent >= row.ammoCost && spent <= row.ammoCost * Math.max(1, row.burst)
      && spent % row.ammoCost === 0, `authored cost per shot: spent ${spent}`);
    assert.ok(sounds.some(s => tagValue(s, "event") === "shoot"), "a successful shot emits its firing cue");
    console.log(JSON.stringify({ weapon: row.weaponTemplate, setting: row.setting,
      projectile: row.projectile, presentation: vr ? "vr" : "flat", spent, spawned: rounds.length, sounds: sounds.map(s => s.sample) }));
    } catch (error) { console.error(game.logs().slice(-60).join("\n")); throw error; }
  });
}
