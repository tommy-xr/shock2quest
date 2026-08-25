import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// earth.mis's basic-training stand:
//
//   Ecology 599 (EcoType 41, period 8s, min/max 1)
//        --SwitchLink--> Spawn 598 (EcoType 41, SELF_MARKER, "DopeyDroid")
//   Droid 547 (EcoType 41) stands on the marker.
//
// "DopeyDroid" is mission object 597 - a concrete archetype parked off-map,
// NOT a gamesys template - so resolving the spawn archetype by name has to
// look at the mission's own objects. These are stable mission object ids;
// runtime entity ids are discovered afresh on every launch.
const TRAINING_DROID = 547;
const DOPEY_DROID = 597;

function distance(
  left: [number, number, number],
  right: [number, number, number],
): number {
  return Math.hypot(left[0] - right[0], left[1] - right[1], left[2] - right[2]);
}

test(
  "a killed training droid is respawned on its stand by the ecology",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 10 });

    const droids = await game.entities.byTemplate(TRAINING_DROID);
    assert.equal(
      droids.length,
      1,
      `expected the basic-training droid, got ${JSON.stringify(droids)}`,
    );
    const droid = droids[0]!;
    const stand = droid.position;

    await game.entities.sendMessage(droid.id, { type: "Damage", amount: 100 });
    await game.step({ frames: 5 });
    const afterDeath = await game.entities.byTemplate(TRAINING_DROID);
    assert.equal(afterDeath.length, 0, "the killed droid should be gone");

    // The ecology polls every 8s; give it two periods of headroom.
    await game.step({ frames: 900 });

    const respawned = (await game.entities.byTemplate(DOPEY_DROID)).filter(
      (entity) => distance(entity.position, stand) < 2.0,
    );
    assert.equal(
      respawned.length,
      1,
      `expected one droid respawned on the stand at ${JSON.stringify(stand)}, got ${JSON.stringify(
        await game.entities.byTemplate(DOPEY_DROID),
      )}`,
    );

    const detail = await game.entities.detail(respawned[0]!.id);
    const model = detail.properties.find(
      (property) => property.name === "Model",
    )?.value;
    assert.equal(
      model,
      "protonew",
      `the respawn should be a live protocol droid, got ${JSON.stringify(detail.properties)}`,
    );

    // The respawn has to satisfy its ecology's population census, or every
    // later period spawns another droid - on this stand and on the third
    // training stand, whose authored droid is missed by the same census bug.
    // Every stand clones "DopeyDroid", and the archetype object itself is
    // parked off-map, so three more periods later the level must still hold
    // exactly two: the parked archetype and this one respawn.
    await game.step({ frames: 1500 });
    const settled = await game.entities.byTemplate(DOPEY_DROID);
    assert.equal(
      settled.length,
      2,
      `the ecologies must stop at their authored population, got ${JSON.stringify(settled)}`,
    );
  },
);
