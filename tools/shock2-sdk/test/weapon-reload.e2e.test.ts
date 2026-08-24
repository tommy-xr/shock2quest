import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { pickupEarthWeapons } from "./helpers/earth-weapons.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end regression for authored reserve accounting: the real Earth
// Weapons Training pistol reloads only from normally-picked-up Small Standard
// Clips, destroying exhausted stacks and retaining a partial remainder.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

function stackOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "StackCount");
  assert.ok(p, "reserve clip should expose a StackCount property");
  return Number(p.value);
}

test(
  "earth reload consumes matching reserve entities and partial stacks",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8097),
    });

    await game.step({ frames: 30 });
    let { pistol, clips } = await pickupEarthWeapons(game, 3);
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), 0, "Earth pistol starts empty");
    for (const clip of clips) {
      assert.equal(stackOf(await game.entities.detail(clip.id)), 6, "authored small clip has 6");
    }

    // The selected ammo type controls compatibility. AP is selected here, so
    // carrying only standard clips must neither consume reserve nor mint ammo.
    assert.equal((await game.info()).player.wielded_ammo_type, "std");
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_ammo_type, "ap");
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), 0);
    assert.equal((await game.info()).player.reloading, false);
    for (const clip of clips) {
      assert.equal(
        stackOf(await game.entities.detail(clip.id)),
        6,
        "mismatched standard reserve remains untouched",
      );
    }

    // Ammo selection is part of a loaded magazine's identity. Save/load must
    // preserve a non-default selection rather than silently converting it.
    const saveName = `reload_selected_ammo_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    assert.equal((await game.info()).player.wielded_ammo_type, "ap");
    const restored = (await game.entities.list()).entities;
    pistol = restored.find((entity) => entity.template_id === 246 && entity.name === "Pistol")!;
    clips = restored
      .filter(
        (entity) =>
          entity.template_id >= 247 &&
          entity.template_id <= 249 &&
          entity.name.includes("Standard Clip"),
      )
      .sort((a, b) => a.template_id - b.template_id);
    assert.ok(pistol, "saved Earth pistol should be restored");
    assert.equal(clips.length, 3, "all three saved reserve clips should be restored");
    assert.equal((await game.info()).player.wielded_entity_id, pistol.id);

    // Return through HE to standard for the matching-reserve scenarios.
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_ammo_type, "he");
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_ammo_type, "std");

    // One reload fills the 12-round magazine from two real six-round entities.
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      12,
      "two six-round reserve entities fill the 12-round pistol",
    );
    const afterFull = await game.player.inventory();
    const liveAfterFull = new Set(
      (await game.entities.list()).entities.map((entity) => entity.id),
    );
    for (const exhausted of clips.slice(0, 2)) {
      assert.ok(
        !afterFull.items.some((item) => item.entity_id === exhausted.id),
        `exhausted reserve ${exhausted.template_id} should leave the backpack`,
      );
      assert.ok(
        !liveAfterFull.has(exhausted.id),
        `exhausted reserve ${exhausted.template_id} should be destroyed`,
      );
    }
    assert.equal(
      stackOf(await game.entities.detail(clips[2].id)),
      6,
      "reload stops once the magazine is full",
    );
    // (What a cycle does to this full magazine - eject it back to reserve
    // rather than convert it - is weapon-ammo-eject's subject; leave the
    // magazine alone here so the reserve accounting below stays readable.)

    // Let the fire-gating animation finish, then exercise a partial reload.
    await game.step({ frames: 130 });
    assert.equal((await game.info()).player.reloading, false, "reload finished");
    await fireOnce(game);
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "std",
      "the guarded magazine still fires its selected standard projectile",
    );
    for (let i = 0; i < 3; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), 8, "four shots consumed");
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      12,
      "partial reload restores only the four missing rounds",
    );
    assert.equal(
      stackOf(await game.entities.detail(clips[2].id)),
      2,
      "partial reload leaves the unused reserve remainder",
    );

    // A repeated reload at capacity cannot consume or mint anything.
    await game.step({ frames: 130 });
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.reloading, false, "full gun does not start reload");
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), 12);
    assert.equal(stackOf(await game.entities.detail(clips[2].id)), 2);
  },
);
