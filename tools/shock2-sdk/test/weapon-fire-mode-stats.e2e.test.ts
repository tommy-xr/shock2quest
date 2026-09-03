import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  ammoOf,
  cycleToWeapon,
  pullTrigger,
  waitForShotReady,
} from "./helpers/weapon.js";

// End-to-end coverage for the NUMBERS behind a fire mode (BaseGunDesc): how many
// rounds a shot costs (ammo_usage), how long the gun waits before the next one
// (shot_interval_ms), and how the mode scales the projectile it launches
// (speed_modifier). Mode SWITCHING itself is covered by weapon-fire-mode.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "the laser pistol spends its setting's rounds and waits its setting's interval",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const laser = await cycleToWeapon(game, (e) => e.name === "Laser Pistol");
    const charge = async () => ammoOf(await game.entities.detail(laser.id));
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      "NORM",
      "the laser starts on its normal setting",
    );

    // NORM: 3 charge a shot, 350 ms between shots.
    const start = await charge();
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(await charge(), start - 3, "a normal laser shot costs 3 charge");

    // A second pull two frames into the 350 ms wait is ignored outright.
    await pullTrigger(game);
    assert.equal(await charge(), start - 3, "a pull inside the shot interval does not fire");

    // Past the interval it fires again.
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(await charge(), start - 6, "the next pull after the interval fires");

    // OVER: 20 charge a shot, 3 s between shots.
    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      "OVER",
      "switched to the overcharge",
    );

    await waitForShotReady(game);
    const beforeOvercharge = await charge();
    await pullTrigger(game);
    assert.equal(await charge(), beforeOvercharge - 20, "an overcharge costs 20 charge");

    // One second in - well past NORM's 350 ms, well short of OVER's 3 s.
    await game.step({ frames: 60 });
    await pullTrigger(game);
    assert.equal(
      await charge(),
      beforeOvercharge - 20,
      "the overcharge is still recovering a second later",
    );

    // ... and once the full 3 s is up it fires again.
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(
      await charge(),
      beforeOvercharge - 40,
      "past the 3 s interval the overcharge fires again",
    );
  },
);

test(
  "the shotgun's triple load costs three shells and refuses to fire on two",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const shotgun = await cycleToWeapon(game, (e) => e.name === "Shotgun");
    const shells = async () => ammoOf(await game.entities.detail(shotgun.id));
    const loaded = await shells();
    assert.equal(loaded, 6, "the debug shotgun starts on a full 6-shell tube");

    const switchMode = async (header: "NORM" | "TRIPLE") => {
      await game.input.trigger("CycleGunSetting");
      await game.step({ frames: 2 });
      assert.equal((await game.info()).player.wielded_gun_setting_header, header);
    };

    await switchMode("TRIPLE");
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(await shells(), loaded - 3, "a triple shot burns three shells");

    // Down to two shells on the single load - fewer than a triple shot costs.
    await switchMode("NORM");
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(await shells(), 2, "a single shot costs one shell");

    await switchMode("TRIPLE");
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(
      await shells(),
      2,
      "a magazine that cannot pay for the whole shot dry-fires instead",
    );
  },
);

test(
  "the fusion cannon's DEATH mode launches its shot at the mode's 0.4x speed",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const fusion = await cycleToWeapon(game, (e) => e.name === "Fusion Cannon");
    assert.ok(ammoOf(await game.entities.detail(fusion.id)) > 4, "the fusion starts charged");

    /** Speed of the fusion shot in flight, one frame after the trigger. */
    const shotSpeed = async (): Promise<number> => {
      await waitForShotReady(game);
      await pullTrigger(game);
      const inFlight = (await game.physics.bodies({ limit: 300 })).bodies.filter((b) =>
        (b.entity_name ?? "").includes("Fusion Shot"),
      );
      assert.equal(
        inFlight.length,
        1,
        "exactly the shot just fired should be in flight (an earlier one still " +
          "airborne would be measured instead)",
      );
      const shot = inFlight[0];
      return Math.hypot(shot.velocity[0], shot.velocity[1], shot.velocity[2]);
    };

    const norm = await shotSpeed();
    assert.ok(norm > 0, `the NORM shot should be moving, got ${norm}`);

    // Both of the fusion's shots are authored with the same launch speed, so
    // the whole difference below is the DEATH setting's 0.4x speed modifier.
    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      "DEATH",
      "switched to the slow, heavy mode",
    );

    const death = await shotSpeed();
    assert.ok(
      Math.abs(death / norm - 0.4) < 0.05,
      `DEATH should launch at 0.4x NORM (${death} vs ${norm})`,
    );
  },
);
