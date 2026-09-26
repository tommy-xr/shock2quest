import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";

// Regression test for the VR psi amp firing into the player's own capsule.
//
// This deliberately runs in a REAL mission - earth.mis's authored Psionic
// training room, with the mission's own Psi Amp (object 290) - and at a
// natural close hold, because that is the combination the existing coverage
// missed. `vr-muzzle-origin.e2e.test.ts` poses the hand at arm's length in the
// synthetic `debug_psi` scene, which clears the player capsule, so it passed
// throughout while a player holding the amp at their chest saw the bolt
// detonate on themselves: psi spent, HP lost, nothing leaving the amp.
//
// The assertions are the two things the player actually experiences - the bolt
// survives and travels, and the cast costs no health - at a hold close enough
// to the body that the muzzle is inside the player's capsule. That closeness is
// asserted rather than assumed: if a future grip change moved the muzzle clear
// of the capsule this test would stop covering anything, so it fails loudly
// instead.
//
// Verified negatively: on the commit before the fix this fails with
// "the bolt cast from a chest hold must survive, not detonate on the player".
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** In front of the authored psi amp in earth.mis's Psionic training room. */
const TRAINING_ROOM = { x: 238.0, y: 23.5, z: 189.2 };

/** Barrel yawed so the model's -X points away from the player, not into them. */
const BARREL_FORWARD = [0, 0.7071068, 0, 0.7071068];

/**
 * The standing player capsule's radius in world units
 * (`PLAYER_STANDING_RADIUS / SCALE_FACTOR` = 1.2 / 2.5). A bolt spawning within
 * this of the pawn's vertical axis starts inside the shooter - the whole
 * condition this test exists to cover.
 */
const PLAYER_CAPSULE_RADIUS = 1.2 / 2.5;

/**
 * Holds that put the amp's muzzle inside the player's own capsule. These are
 * ordinary ways to hold a controller - at the chest, in at the body, at the
 * hip - not contrived poses.
 */
const CLOSE_HOLDS: Array<[string, number[]]> = [
  ["chest", [0.35, 1.1, 0.45]],
  ["in at the body", [0.2, 1.05, 0.2]],
  ["hip", [0.35, 0.9, 0.35]],
];

test(
  "a VR psi amp held close to the body still casts cryokinesis out of the amp",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: e2ePort(),
      debugFlags: ["--vr"],
    });

    await game.step({ frames: 30 });
    await game.player.teleport(TRAINING_ROOM);
    await game.step({ frames: 60 });

    // A VR hand with no grip drops whatever it is handed on the same frame, so
    // squeeze before taking the amp.
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 5 });

    // The mission's own psi amp, discovered by name - runtime ids are not
    // stable across runs.
    const amp = (await game.entities.list({ limit: 2000 })).entities.find(
      (e) => e.name === "Psi Amp",
    );
    assert.ok(amp, "earth.mis Psionic training room should contain a Psi Amp");

    await game.player.give(amp.id);
    await game.step({ frames: 20 });
    await game.input.trigger("EquipPsiAmp");
    await game.step({ frames: 30 });

    let player = (await game.info()).player;
    assert.equal(
      player.right_hand_entity_id,
      amp.id,
      "the amp should be held in the right hand",
    );
    assert.equal(
      player.selected_psi_power,
      "Cryokinesis",
      "a fresh earth.mis character starts with Projected Cryokinesis selected",
    );

    for (const [label, hold] of CLOSE_HOLDS) {
      await game.input.set("right_hand.position", hold);
      await game.input.set("right_hand.rotation", BARREL_FORWARD);
      await game.step({ frames: 10 });

      assert.equal(
        (await cryoBolts(game)).length,
        0,
        `no bolt from an earlier hold may still be alive before the ${label} cast`,
      );

      const before = (await game.info()).player;

      await game.input.set("right_hand.trigger", 1.0);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0.0);

      const spawned = await cryoBolts(game);
      assert.equal(
        spawned.length,
        1,
        `casting from a ${label} hold should spawn exactly one Cryo PSI bolt`,
      );
      const bolt = spawned[0]!;
      const spawn = bolt.position;

      // Precondition: the shot really does start inside the shooter. Without
      // this the test could quietly stop exercising the bug.
      const radial = Math.hypot(
        spawn[0]! - before.position[0]!,
        spawn[2]! - before.position[2]!,
      );
      assert.ok(
        radial < PLAYER_CAPSULE_RADIUS,
        `the ${label} hold must put the muzzle inside the player capsule for this test to mean anything (radial ${radial.toFixed(2)} >= ${PLAYER_CAPSULE_RADIUS})`,
      );

      // Eight frames is far longer than the one frame the bolt used to survive
      // when it collided with the shooter, and well short of its lifetime.
      await game.step({ frames: 8 });

      // Track the SAME entity - a different bolt would make the distance
      // measurement meaningless.
      const inFlight = (await cryoBolts(game)).find((e) => e.id === bolt.id);
      assert.ok(
        inFlight,
        `the bolt cast from a ${label} hold must survive, not detonate on the player`,
      );

      const travelled = Math.hypot(
        inFlight.position[0]! - spawn[0]!,
        inFlight.position[1]! - spawn[1]!,
        inFlight.position[2]! - spawn[2]!,
      );
      assert.ok(
        travelled > 1.0,
        `the bolt from a ${label} hold should travel away from the amp (moved ${travelled.toFixed(2)})`,
      );

      const after = (await game.info()).player;
      assert.equal(
        after.hit_points,
        before.hit_points,
        `casting from a ${label} hold must not damage the caster`,
      );
      assert.equal(
        after.psi_points,
        before.psi_points! - 1,
        `a tier 1 cast from a ${label} hold costs exactly one psi point`,
      );

      // Let the bolts clear before the next hold (asserted at the top of the
      // next iteration).
      await game.step({ frames: 120 });
    }
  },
);

async function cryoBolts(game: GameServer) {
  const { entities } = await game.entities.list({ limit: 2000 });
  return entities.filter((e) => e.name?.startsWith("Cryo PSI"));
}
