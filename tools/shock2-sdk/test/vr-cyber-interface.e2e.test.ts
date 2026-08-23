import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/index.js";
import { aimVrHandAt, normalize, quatFromTo } from "./helpers/vr-hand.js";

// The VR cyber interface (use-mode skeleton): in VR presentation
// `ToggleUseMode` (left-controller X on Quest) presents the flat use-mode
// canvas - the top-docked inventory strip - on a head-anchored world panel,
// with the world dimmed behind it, the scene still simulating, and the
// wielded weapon safed (trigger suppressed, weapon stays held).
//
// Negative-first: on the parent, ToggleUseMode is flat-only - in `--vr` the
// mode stays "shooter" and no strip appears, so the first assertion of each
// scenario fails there.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8571);

/** Weapon template cycled by DebugCycleWeapon (mission_core DEBUG_WEAPONS). */
const LASER_PISTOL = -22;

async function uiMode(game: GameServer): Promise<string> {
  return (await game.ui.state()).mode;
}

async function openCyberInterface(game: GameServer): Promise<void> {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
}

test(
  "VR ToggleUseMode presents the use-mode strip while the world keeps simulating",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    assert.equal(await uiMode(game), "shooter");
    assert.equal((await game.ui.state()).strip, null, "no strip outside use mode");

    // Enter: the same /v1/ui contract flat's use mode reports.
    await openCyberInterface(game);
    assert.equal(await uiMode(game), "use", "VR X must enter use mode");
    const strip = (await game.ui.state()).strip;
    assert.ok(strip, "use mode must bind the internal_inventory strip");
    assert.ok(
      strip.elements.length > 0,
      "the strip canvas must render its authored elements on the panel",
    );

    // World time continues while the interface is up - unlike the pause
    // menu. A freshly spawned debug item must fall under gravity.
    await game.input.trigger("SpawnDebugItem");
    await game.step({ frames: 5 });
    const spawned = (await game.physics.bodies()).bodies
      .filter((body) => body.body_type === "dynamic")
      .at(-1);
    assert.ok(spawned, "the debug item must have a dynamic body");
    const before = spawned.position as Vec3;
    await game.step({ frames: 40 });
    const after = (await game.physics.bodies()).bodies.find(
      (body) => body.body_id === spawned.body_id,
    );
    assert.ok(after, "the spawned body must persist");
    const moved = Math.hypot(
      after.position[0] - before[0],
      after.position[1] - before[1],
      after.position[2] - before[2],
    );
    assert.ok(
      moved > 0.05,
      `the world must keep simulating while the cyber interface is open (moved ${moved.toFixed(3)})`,
    );

    // Exit: back to shooter, strip gone.
    await openCyberInterface(game);
    assert.equal(await uiMode(game), "shooter", "X again must leave use mode");
    assert.equal((await game.ui.state()).strip, null);

    // Edge policy: the pause menu owns the screen while it is open - the
    // toggle must not reach the mission (actions are cleared while paused),
    // and opening pause over a live cyber interface closes it.
    await openCyberInterface(game);
    assert.equal(await uiMode(game), "use");
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 3 });
    assert.equal((await game.info()).paused, true, "pause must open");
    assert.equal(
      await uiMode(game),
      "shooter",
      "opening the pause menu must close the cyber interface",
    );
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    assert.equal(
      await uiMode(game),
      "shooter",
      "the toggle must not open the cyber interface under the pause menu",
    );
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 3 });
    assert.equal((await game.info()).paused, false, "pause must close again");

    // Edge policy: no cyber interface while dead - that moment belongs to
    // the game-over flow.
    const info = await game.info();
    assert.ok(info.player.entity_id !== null && info.player.hit_points !== null);
    await game.entities.sendMessage(info.player.entity_id, {
      type: "Damage",
      amount: info.player.hit_points + 100,
    });
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.hit_points, 0, "the player must be dead");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    assert.equal(
      await uiMode(game),
      "shooter",
      "a dead player must not be able to open the cyber interface",
    );
  },
);

test(
  "the cyber interface safes the wielded weapon and swallows a held trigger across exit",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // DebugCycleWeapon drops each weapon in front of the player in VR; grab
    // the laser pistol (self-recharging - no ammo bookkeeping in the way).
    let laser: EntitySummary | undefined;
    for (let cycle = 0; cycle < 12 && !laser; cycle += 1) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 10 });
      laser = (await game.entities.list()).entities.find((e) => e.template_id === LASER_PISTOL);
    }
    assert.ok(laser, "DebugCycleWeapon must spawn the Laser Pistol");
    await aimVrHandAt(game, laser.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      laser.id,
      "the hand must hold the pistol",
    );
    // Aim clear of the player before firing.
    await game.input.set("right_hand.position", [0, 1, -2]);
    await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], normalize([-1, 0, 0])));
    await game.step({ frames: 3 });

    const shotsFired = async (fire: () => Promise<void>): Promise<number> => {
      const before = new Set((await game.entities.list()).entities.map((e) => e.id));
      await fire();
      const shots = (await game.entities.list()).entities.filter(
        (e) => !before.has(e.id) && e.name === "Laser Shot",
      );
      return shots.length;
    };

    // Baseline: the harness genuinely fires in shooter mode.
    const baseline = await shotsFired(async () => {
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 3 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 3 });
    });
    assert.ok(baseline >= 1, "the wielded pistol must fire outside the cyber interface");

    // Weapon-safe: with the interface up, the trigger must not fire - and the
    // weapon must stay wielded.
    await openCyberInterface(game);
    assert.equal(await uiMode(game), "use");
    const whileOpen = await shotsFired(async () => {
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 10 });
    });
    assert.equal(whileOpen, 0, "the trigger must not fire while the cyber interface is open");
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      laser.id,
      "the weapon must stay wielded while safed",
    );

    // Exit under the held trigger: the stale press must NOT fire on the way
    // out (rising-edge / swallow-until-release).
    const acrossExit = await shotsFired(async () => {
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 10 });
    });
    assert.equal(await uiMode(game), "shooter");
    assert.equal(acrossExit, 0, "a trigger held across the exit must not fire the weapon");

    // Release and pull again: the weapon works normally after the mode.
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
    const afterRelease = await shotsFired(async () => {
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 3 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 3 });
    });
    assert.ok(afterRelease >= 1, "a fresh pull after release must fire again");
  },
);
