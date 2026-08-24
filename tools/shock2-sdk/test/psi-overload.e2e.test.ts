import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the psi amp's hold-to-overload charge meter.
// Overloadable powers (Projected Cryokinesis is the default selection) cast
// on trigger RELEASE: a quick release casts normally; releasing with the bar
// in the end zone (>= 85% of the tier-scaled charge duration) casts at +2
// effective PSI (a stronger projectile); holding past a full bar is a psi
// burnout - the cast fails but the points are spent.
//
// Tier 1 charge duration is 2.0s = 120 fixed-timestep frames, so frame
// counts below map exactly onto bar fractions.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "psi amp hold-to-overload: normal cast, overload, and burnout",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
    });

    await game.step({ frames: 10 });
    let player = (await game.info()).player;
    assert.equal(player.selected_psi_power, "Cryokinesis");
    const startPsi = player.psi_points!;

    const hasProjectile = async (name: string) =>
      (await game.entities.list({ limit: 120 })).entities.some((e) => e.name === name);

    // 1) Quick click: released long before the zone -> normal cast at the
    // base effective PSI (5 -> "Cryo PSI 5"), meter cleared immediately.
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 3 });
    player = (await game.info()).player;
    assert.equal(player.psi_charge_phase, "charging", "holding shows the charging meter");
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 2 });
    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi - 1, "normal cast costs the tier");
    assert.equal(player.psi_charge_phase, null, "meter clears on a normal release");
    assert.ok(await hasProjectile("Cryo PSI 5"), "normal cast fires the PSI-5 projectile");

    // 2) Overload: hold 110/120 frames (fraction ~0.92, inside the 0.85+
    // zone), then release -> +2 effective PSI -> "Cryo PSI 7".
    await game.step({ frames: 120 }); // let the previous bolt clear frame counts
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 110 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 2 });
    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi - 2, "overload costs the same as a normal cast");
    assert.equal(player.psi_charge_phase, "overloaded", "meter flashes the overload result");
    assert.ok(await hasProjectile("Cryo PSI 7"), "overload fires the PSI-7 projectile");

    // The result flash expires (~0.6s = 36 frames).
    await game.step({ frames: 45 });
    player = (await game.info()).player;
    assert.equal(player.psi_charge_phase, null, "result flash expires");

    // 3) Burnout: hold past a full bar (130 > 120 frames) -> the cast fails,
    // the points are spent, the player takes 3 damage x tier, the meter
    // flashes burnout, and no new projectile spawns.
    const countBolts = async () =>
      (await game.entities.list({ limit: 120 })).entities.filter((e) =>
        e.name?.startsWith("Cryo PSI"),
      ).length;
    const before = await countBolts();
    const hpBefore = player.hit_points!;
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 130 });
    player = (await game.info()).player;
    assert.equal(player.psi_charge_phase, "burnout", "over-holding burns out");
    assert.equal(player.psi_points, startPsi - 3, "burnout still spends the points");
    assert.equal(
      player.hit_points,
      hpBefore - 3,
      "tier-1 burnout deals 3 damage to the player",
    );
    // Immediately after the burnout frame - before older bolts can despawn
    // and mask a wrongly spawned one.
    assert.equal(await countBolts(), before, "burnout spawns no projectile");
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 45 });
    player = (await game.info()).player;
    assert.equal(player.psi_charge_phase, null, "burnout flash expires");
  },
);
