import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce, pullTrigger } from "./helpers/weapon.js";

// End-to-end regression test for ammo-type cycling (InputAction::CycleAmmo). The
// pistol carries three Projectile links (std / he / ap); cycling advances the
// selected one and wraps. Observable headlessly via /v1/info.wielded_ammo_type.
// What a cycle does to a LOADED magazine is weapon-ammo-eject's subject.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function ammoType(game: GameServer): Promise<string | null> {
  return (await game.info()).player.wielded_ammo_type;
}

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

test(
  "cycling advances the selected ammo type and wraps",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
    });

    // Unarmed: no ammo type.
    await game.step({ frames: 5 });
    assert.equal(await ammoType(game), null, "unarmed has no ammo type");

    // Wield the pistol (std / he / ap).
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    assert.equal(await ammoType(game), "std", "pistol starts on its first ammo type");
    const pistolId = (await game.info()).player.wielded_entity_id;
    assert.ok(pistolId !== null, "pistol should be wielded");

    // Cycling a LOADED magazine ejects it to the backpack first, which is
    // weapon-ammo-eject's subject. This test is about the selection ORDER, so
    // fire the magazine off and cycle from empty.
    const loaded = ammoOf(await game.entities.detail(pistolId));
    assert.ok(loaded > 0, "debug pistol starts loaded");
    for (let i = 0; i < loaded; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistolId)), 0, "pistol is empty");

    // Once empty, cycle through all three and wrap back.
    const sequence: string[] = [];
    for (let i = 0; i < 3; i++) {
      await game.input.trigger("CycleAmmo");
      await game.step({ frames: 2 });
      sequence.push((await ammoType(game)) ?? "<null>");
    }
    // The cycle order follows ProjectileOptions.order (the data's intended
    // order), not the raw link order: std -> ap -> he -> std.
    assert.deepEqual(
      sequence,
      ["ap", "he", "std"],
      "cycling advances in ProjectileOptions.order and wraps back to std",
    );
  },
);


test(
  "ammo switch reloads the new type and locks out cycling and firing until ready",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    const pistol = (await game.info()).player.wielded_entity_id;
    assert.ok(pistol !== null);
    const standardBefore = ammoOf(await game.entities.detail(pistol));
    const ap = await game.player.spawnItem(-1360); // Small Armor Piercing Clip
    assert.ok(ap.entity_id);
    const apDetail = await game.entities.detail(ap.entity_id);
    const apRounds = Number(apDetail.properties.find(p => p.name === "StackCount")?.value);
    assert.ok(apRounds > 0);

    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 6 });
    let player = (await game.info()).player;
    assert.equal(player.wielded_ammo_type, "ap");
    assert.equal(player.reloading, true);
    assert.ok(Math.abs(player.reload_pitch_deg) > 1);
    assert.equal(ammoOf(await game.entities.detail(pistol)), apRounds);
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === ap.entity_id));
    const standard = (await game.entities.list()).entities.find(e => e.template_id === -31);
    assert.ok(standard);
    const standardDetail = await game.entities.detail(standard.id);
    assert.equal(Number(standardDetail.properties.find(p => p.name === "StackCount")?.value), standardBefore);

    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    await pullTrigger(game);
    player = (await game.info()).player;
    assert.equal(player.wielded_ammo_type, "ap", "repeat press cannot eject mid-reload");
    assert.equal(player.reloading, true);
    assert.equal(ammoOf(await game.entities.detail(pistol)), apRounds, "cannot fire mid-reload");

    await game.step({ frames: 180 });
    player = (await game.info()).player;
    assert.equal(player.reloading, false);
    assert.equal(player.reload_pitch_deg, 0);
    await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistol)), apRounds - 1);
  },
);
