import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end coverage for the round-1 O/S upgrade effects: Speedy (4),
// Sharpshooter (5), Lethal Weapon (9), Power Psi (14), Spatially Aware (16),
// and Pharmo-Friendly's (2) psi-hypo half. (Strong Metabolism is unit-tested only;
// see the note further down.)
//
// Traits are granted through the debug provisioning path
// (`POST /v1/player/stats` `os_traits`), which applies exactly what the trait
// machine applies - the machine itself is one-shot per level and cannot vend
// the several traits these cases need. `os-traits.e2e.test.ts` covers the
// machine.
//
// Negative-first: every case measures the untraited behavior first in the same
// run (or, where the effect is not reversible, in a paired run) and asserts the
// traited measurement differs by the trait's magnitude. On the base commit the
// grants are refused outright ("Upgrade unavailable in this build"), so
// `os_traits` stays empty and every assertion below fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TRAIT_PHARMO_FRIENDLY = 2;
const TRAIT_SPEEDY = 4;
const TRAIT_SHARPSHOOTER = 5;
const TRAIT_LETHAL_WEAPON = 9;
const TRAIT_POWER_PSI = 14;
const TRAIT_SPATIALLY_AWARE = 16;

const TRAINING_DROID = 593;
const WRENCH = -928;
const PISTOL = -17;

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find(
    (candidate) => candidate.name === "HitPoints",
  );
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

test(
  "Speedy: stick locomotion covers 15% more ground",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal" });
    await game.step({ frames: 30 });

    // The default debug floor is 48 world units across and empty, so a straight
    // run is limited by speed alone.
    const start = (await game.info()).player.position;
    const walk = async () => {
      await game.player.teleport({ x: start[0], y: start[1], z: start[2] });
      await game.step({ frames: 20 });
      const from = (await game.info()).player.position;
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 60 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.step({ frames: 2 });
      const to = (await game.info()).player.position;
      return Math.hypot(to[0] - from[0], to[2] - from[2]);
    };

    const plain = await walk();
    assert.ok(plain > 1.0, `the player should actually walk; covered ${plain}`);

    const stats = await game.player.setStats({ os_traits: [TRAIT_SPEEDY] });
    assert.deepEqual(stats.os_traits, [TRAIT_SPEEDY], "Speedy is recorded");

    const speedy = await walk();
    const ratio = speedy / plain;
    assert.ok(
      Math.abs(ratio - 1.15) < 0.02,
      `Speedy should cover 1.15x the ground; ${plain} -> ${speedy} (${ratio})`,
    );
  },
);

test(
  "Lethal Weapon: a flat wrench swing hits 35% harder",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 5 });

    await game.player.spawnItem(WRENCH);
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 5 });
    const [droid] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(droid, "Earth should contain its authored Training Droid");

    const [x, y, z] = (await game.entities.detail(droid.id)).position;
    await game.player.teleport({ x: x + 1.2, y: y + 1, z });
    await game.step({ frames: 60 });

    // One full swing: the aimed hit resolves at the authored MF_TRIGGER1 event,
    // 41 simulation frames after the trigger edge (see
    // flat-melee-animation-event.e2e.test.ts).
    const swing = async () => {
      const aim = await game.player.aimAt(droid, {
        hitbox: "torso",
        visibility: "required",
      });
      assert.equal(aim.entity_id, droid.id);
      await game.step({ frames: 3 });
      const before = hitPoints(await game.entities.detail(droid.id));
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 45 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 60 });
      const after = hitPoints(await game.entities.detail(droid.id));
      return before - after;
    };

    const plain = await swing();
    assert.equal(plain, 6, "the untraited flat Wrench swing deals its authored 6");

    await game.player.setStats({ os_traits: [TRAIT_LETHAL_WEAPON] });
    const lethal = await swing();
    assert.ok(
      lethal > plain,
      `Lethal Weapon must raise melee damage; ${plain} -> ${lethal}`,
    );
    // 6 * 1.35 = 8.1, applied to an integer hit-point pool.
    assert.equal(lethal, 8, `Lethal Weapon should bill 6 * 1.35; got ${lethal}`);
  },
);

test(
  "Power Psi removes burnout damage; Pharmo-Friendly boosts the psi hypo",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.selected_psi_power, "Cryokinesis");

    // Over-hold the amp past a full charge bar (tier 1 = 120 frames): the cast
    // fails and the points are spent either way; only the damage is at issue.
    const burnout = async () => {
      const before = (await game.info()).player;
      await game.input.set("right_hand.trigger", 1.0);
      await game.step({ frames: 130 });
      const after = (await game.info()).player;
      await game.input.set("right_hand.trigger", 0.0);
      await game.step({ frames: 45 });
      assert.equal(after.psi_charge_phase, "burnout", "over-holding burns out");
      assert.ok(
        after.psi_points! < before.psi_points!,
        "a burnout spends its points regardless",
      );
      return before.hit_points! - after.hit_points!;
    };

    assert.equal(await burnout(), 3, "a tier-1 burnout normally costs 3 hit points");

    // Drain enough psi that a 24-point refill is not clipped by the ceiling,
    // then measure what one hypo restores.
    const drain = async (target: number) => {
      for (let attempt = 0; attempt < 60; attempt += 1) {
        const psi = (await game.info()).player;
        if (psi.psi_points! <= psi.max_psi_points! - target) return;
        await game.input.set("right_hand.trigger", 1.0);
        await game.step({ frames: 3 });
        await game.input.set("right_hand.trigger", 0.0);
        await game.step({ frames: 3 });
      }
      assert.fail(`could not drain ${target} psi points`);
    };
    const useHypo = async () => {
      const hypo = await game.player.spawnItem("Psi Booster");
      const before = (await game.info()).player.psi_points!;
      await game.entities.sendMessage(hypo.entity_id, { type: "Frob" });
      await game.step({ frames: 5 });
      return (await game.info()).player.psi_points! - before;
    };

    await drain(30);
    assert.equal(await useHypo(), 20, "the psi hypo normally restores 20 points");

    const stats = await game.player.setStats({
      os_traits: [TRAIT_POWER_PSI, TRAIT_PHARMO_FRIENDLY],
    });
    assert.deepEqual(stats.os_traits, [TRAIT_POWER_PSI, TRAIT_PHARMO_FRIENDLY]);

    assert.equal(await burnout(), 0, "Power Psi removes the burnout damage entirely");
    await drain(30);
    assert.equal(await useHypo(), 24, "Pharmo-Friendly makes the hypo restore 24");
  },
);

// Strong Metabolism (1) has no end-to-end case here. The only shipped
// radiation source reachable in a test is medsci2's Rad Barrel, whose burst
// puts the stored level on a ~6.5 plateau; a 6-second tick there bills
// trunc(6.5 * 0.25) = 1 with the trait and without it, so the quarter cut is
// below what an integer hit-point pool can show. The arithmetic is covered by
// `strong_metabolism_cuts_the_radiation_pulse_by_a_quarter` in
// `shock2vr/src/scripts/radiation.rs`.

test(
  "Spatially Aware reveals the whole automap, on acquire and on every load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `os_upgrades_map_${Date.now()}`;
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 5 });

    const map = async () => {
      const player = (await game.info()).player;
      return {
        explored: player.explored_map_locations.length,
        total: player.map_location_count,
      };
    };

    const before = await map();
    assert.ok(before.total > 0, "medsci1 ships an automap with locations");
    assert.ok(
      before.explored < before.total,
      `a fresh arrival has not explored the level; ${before.explored}/${before.total}`,
    );

    await game.player.setStats({ os_traits: [TRAIT_SPATIALLY_AWARE] });
    await game.step({ frames: 5 });
    const acquired = await map();
    assert.equal(
      acquired.explored,
      acquired.total,
      "buying the upgrade reveals every location of the current level",
    );

    // The reveal re-derives at load, like the other trait bonuses.
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 5 });
    const loaded = await map();
    assert.equal(
      loaded.explored,
      loaded.total,
      "the reveal survives save/load",
    );

    // ...and covers a level the player has never set foot on.
    await game.transitionLevel("medsci2.mis");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).mission, "medsci2.mis");
    const next = await map();
    assert.ok(next.total > 0, "medsci2 ships an automap too");
    assert.equal(
      next.explored,
      next.total,
      "arriving on a new deck arrives with its map already revealed",
    );
  },
);

test(
  "Sharpshooter: a pistol shot lands 15% harder",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 5 });

    await game.player.spawnItem(PISTOL);
    await game.player.spawnItem("Small Standard Clip");
    await game.input.trigger("EquipPistol");
    await game.step({ frames: 10 });

    const [droid] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(droid, "Earth should contain its authored Training Droid");
    const [x, y, z] = (await game.entities.detail(droid.id)).position;
    await game.player.teleport({ x: x + 2.5, y: y + 1, z });
    await game.step({ frames: 60 });

    // One aimed round, measured on the victim's own hit points - the same pool
    // the projectile's impact paths bill.
    const shoot = async () => {
      const aim = await game.player.aimAt(droid, {
        hitbox: "torso",
        visibility: "required",
      });
      assert.equal(aim.entity_id, droid.id);
      await game.step({ frames: 3 });
      const before = hitPoints(await game.entities.detail(droid.id));
      await fireOnce(game);
      await game.step({ frames: 30 });
      const after = hitPoints(await game.entities.detail(droid.id));
      // A dead droid's pool clamps at zero, which would under-report the
      // second shot and read exactly like "the trait did nothing".
      assert.ok(after > 0, "the droid must survive the shot being measured");
      return before - after;
    };

    const plain = await shoot();
    assert.equal(plain, 6, "the untraited pistol shot deals the flat path's 6");

    await game.player.setStats({ os_traits: [TRAIT_SHARPSHOOTER] });
    const sharp = await shoot();
    // 6 * 1.15 = 6.9, on an integer hit-point pool.
    assert.equal(sharp, 7, `Sharpshooter should bill 6 * 1.15; got ${sharp}`);
  },
);
