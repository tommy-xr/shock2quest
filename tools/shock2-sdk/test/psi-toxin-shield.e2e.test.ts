import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for Neural Toxin-blocker ("Toxin Shield" in the gamesys,
// tier 3 sustained). Casting it spends 3 psi and, while active, the caster
// resists the toxin (Venom) stim by the power's authored data[0] = 100%.
//
// The port has no toxin *consumer* yet: the shipped `The Player` archetype
// answers the Venom stim with a `toxin` receptron effect the port does not
// implement (a poisoning status model is out of scope, per issue #1308), and
// no ambient radius source other than radiation is pulsed. So the HP
// assertions below are characterization, not proof - the resistance itself is
// proved by the unit tests in `shock2vr/src/scripts/toxin_shield.rs`, which
// resolve a toxin damage receptron through the shield.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TOXIN_SHIELD_COST = 3;
/** Toxin Shield duration at the effective PSI stat of 5: 10 + 5*5 seconds. */
const TOXIN_SHIELD_DURATION_FRAMES = 35 * 60;

/** The debug_psi toxin hazard patch (EggGooCloud), as a world position. */
const TOXIN_PATCH = { x: 0.0, y: 2.0, z: -9.0 };

/** Cycle the psi power selection until `name` is selected (bounded). */
async function selectPsiPower(game: GameServer, name: string): Promise<void> {
  for (let i = 0; i < 40; i++) {
    if ((await game.info()).player.selected_psi_power === name) return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
  }
  assert.fail(`could not cycle the psi power selection to ${name}`);
}

/** Step `frames` in chunks so a single /v1/step call stays small. */
async function stepFrames(game: GameServer, frames: number): Promise<void> {
  const chunk = 300;
  for (let done = 0; done < frames; done += chunk) {
    await game.step({ frames: Math.min(chunk, frames - done) });
  }
}

test(
  "Neural Toxin-blocker: cast spends psi, shields the caster in the toxin patch, and expires",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    let player = (await game.info()).player;
    assert.ok(player.wielded_entity_id !== null, "psi amp should be auto-wielded");
    assert.deepEqual(player.active_psi_powers, [], "no sustained powers active at start");
    const startPsi = player.psi_points;
    assert.ok(startPsi !== null && startPsi >= TOXIN_SHIELD_COST);

    // The scene's toxin hazard station is present.
    const goo = (await game.entities.list({ filter: "EggGooCloud", limit: 5 })).entities[0];
    assert.ok(goo, "debug_psi should contain the EggGooCloud toxin source");

    // Baseline: unshielded exposure to the patch. See the header - no toxin
    // reaches the player today, so this records the rate rather than asserting
    // it is positive.
    const unshieldedStart = player.hit_points!;
    await game.player.teleport(TOXIN_PATCH);
    await stepFrames(game, 180);
    const unshieldedLoss = unshieldedStart - (await game.info()).player.hit_points!;

    // Cast Neural Toxin-blocker (tier 3 sustained power).
    await selectPsiPower(game, "Toxin Shield");
    await fireOnce(game);
    player = (await game.info()).player;
    assert.equal(
      player.psi_points,
      startPsi! - TOXIN_SHIELD_COST,
      "Toxin Shield cast costs 3 psi points",
    );
    assert.deepEqual(
      player.active_psi_powers,
      ["Toxin Shield"],
      "Toxin Shield is active after the cast",
    );

    // Shielded exposure: no toxin damage lands, whatever the baseline was.
    const shieldedStart = player.hit_points!;
    await game.player.teleport(TOXIN_PATCH);
    await stepFrames(game, 300);
    player = (await game.info()).player;
    assert.deepEqual(
      player.active_psi_powers,
      ["Toxin Shield"],
      "still active through the exposure window",
    );
    assert.equal(
      player.hit_points,
      shieldedStart,
      "the shielded caster takes no toxin damage in the patch",
    );
    assert.ok(
      unshieldedLoss <= 0,
      "unshielded exposure is inert today; re-check this test when the toxin response lands",
    );

    // Expiry after 10 + 5*PSI = 35 s.
    await stepFrames(game, TOXIN_SHIELD_DURATION_FRAMES - 300 + 120);
    assert.deepEqual(
      (await game.info()).player.active_psi_powers,
      [],
      "Toxin Shield expires after 35 seconds",
    );
  },
);
