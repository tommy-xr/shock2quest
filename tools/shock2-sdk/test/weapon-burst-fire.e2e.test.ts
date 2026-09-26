import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  ammoOf,
  cycleToWeapon,
  pullTrigger,
  waitForShotReady,
} from "./helpers/weapon.js";

// End-to-end coverage for the two fire settings that send more than one round
// per trigger pull: the pistol's BURST (3 rounds, 10 ms apart) and the assault
// rifle's AUTO (unlimited while the trigger is down, 100 ms apart). The other
// numbers behind a fire mode are covered by weapon-fire-mode-stats, and mode
// switching itself by weapon-fire-mode.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Hold the trigger down (no edge on its own - `step` is what makes frames). */
async function holdTrigger(game: GameServer): Promise<void> {
  await game.input.set("right_hand.trigger", 1.0);
}

async function releaseTrigger(game: GameServer): Promise<void> {
  await game.input.set("right_hand.trigger", 0.0);
}

test(
  "the pistol's BURST sends three rounds off one pull, then waits out its interval",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const pistol = await cycleToWeapon(game, (e) => e.name === "Pistol");
    const rounds = async () => ammoOf(await game.entities.detail(pistol.id));
    assert.equal(await rounds(), 12, "the debug pistol starts on a full clip");
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      "NORM",
      "the pistol starts on its single-shot setting",
    );

    // NORM: one round a pull, whatever the trigger does afterwards.
    await waitForShotReady(game);
    await pullTrigger(game);
    await game.step({ frames: 30 });
    assert.equal(
      await rounds(),
      11,
      "a NORM pull is one round and stays one round",
    );

    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      "BURST",
      "switched to the 3-round burst",
    );

    // One pull, released immediately: the burst plays out anyway, and it takes
    // frames to do it rather than emptying into the one the pull landed in.
    await waitForShotReady(game);
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0.0);
    // 11 rounds are loaded here, so 8 left would be the whole burst emptying
    // into the pull's own frame - the regression the burst's frame pacing is
    // there to prevent.
    const partway = await rounds();
    assert.ok(
      partway > 8,
      `the burst should still owe rounds a frame in, got ${partway} left`,
    );

    await game.step({ frames: 30 });
    assert.equal(await rounds(), 8, "three rounds off the one pull");

    // The setting's 700 ms shot interval then holds off the next burst - half a
    // second later the gun is still recovering.
    await pullTrigger(game);
    await game.step({ frames: 30 });
    assert.equal(
      await rounds(),
      8,
      "a pull inside the interval starts nothing",
    );

    // Past the interval, another pull is another three rounds.
    await waitForShotReady(game);
    await pullTrigger(game);
    await game.step({ frames: 30 });
    assert.equal(await rounds(), 5, "the next burst after the interval fires");
  },
);

test(
  "the assault rifle's AUTO fires for as long as the trigger is held",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const rifle = await cycleToWeapon(game, (e) => e.name === "Assault Rifle");
    const rounds = async () => ammoOf(await game.entities.detail(rifle.id));
    const loaded = await rounds();
    assert.ok(loaded >= 8, `the debug rifle starts loaded, got ${loaded}`);

    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      "AUTO",
      "switched to continual fire",
    );

    // Half a second on the trigger at the setting's 100 ms interval: about
    // five rounds. Coarse, because the pull's own round and the frame the
    // burst starts on land either side of the window - and short of the
    // magazine, so it is the interval being measured and not the clip.
    await waitForShotReady(game);
    await holdTrigger(game);
    await game.step({ frames: 30 });
    await releaseTrigger(game);
    await game.step({ frames: 1 });
    const fired = loaded - (await rounds());
    assert.ok(
      fired >= 4 && fired <= 7,
      `half a second of AUTO is about five rounds, got ${fired}`,
    );

    // And the release really is what stops it.
    const afterRelease = await rounds();
    await game.step({ frames: 60 });
    assert.equal(
      await rounds(),
      afterRelease,
      "nothing more goes out once the trigger is up",
    );
  },
);

test(
  "an AUTO burst stops on the last round instead of overdrawing the magazine",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const rifle = await cycleToWeapon(game, (e) => e.name === "Assault Rifle");
    const rounds = async () => ammoOf(await game.entities.detail(rifle.id));

    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_gun_setting_header, "AUTO");

    // Hold the magazine down to its last round. Three frames at a time: a round
    // leaves every six, so the poll cannot step past 1.
    await waitForShotReady(game);
    await holdTrigger(game);
    for (let i = 0; i < 400 && (await rounds()) > 1; i += 1) {
      await game.step({ frames: 3 });
    }
    await releaseTrigger(game);
    await game.step({ frames: 1 });
    assert.equal(await rounds(), 1, "held down to a single round");

    // One more pull spends it and the burst ends there - the magazine never
    // goes below empty, and the empty gun keeps clicking rather than firing.
    await waitForShotReady(game);
    await pullTrigger(game);
    await game.step({ frames: 60 });
    assert.equal(
      await rounds(),
      0,
      "the last round fires, and only the last round",
    );
  },
);
