import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// An authored corpse is a posed creature, not a live one: it carries
// PropCreature + PropCreaturePose and a skinned mesh, and its collider is the
// model-bounds box every frobbable fixture gets. An animated model used to
// report no bounds at all, so that box collapsed to the 0.2-unit default at
// the corpse's root joint - the body was unfrobbable except for one spot over
// its hip, and the wrench inside it was unreachable.
//
// Negative-first: before the fix the squeeze below opens no panel, and the
// footprint probe finds the corpse under a single sample instead of dozens.
const MEDSCI_CORPSE_NAME = "MS Male Corpse";
const STANDING_SPOT = { x: -37.27, y: -4.4, z: 30.2 };

test(
  "medsci1: a corpse is frobbed by aiming anywhere along its body",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });

    const { entities } = await game.entities.list();
    const corpses = entities.filter((entity) => entity.name === MEDSCI_CORPSE_NAME);
    const corpse = corpses.sort((a, b) => a.distance - b.distance)[0];
    assert.ok(corpse, `expected a ${MEDSCI_CORPSE_NAME} in medsci1`);
    const [cx, cy, cz] = corpse.position;

    // The selection collider must span the body, not sit on its origin. Probe
    // straight down on a grid and count the samples that land on the corpse.
    const probe = async (dx: number, dz: number) =>
      game.raycast({
        start: [cx + dx, cy + 3, cz + dz],
        end: [cx + dx, cy - 1, cz + dz],
        collision_groups: ["world", "entity", "selectable", "raycast", "hitbox"],
      });
    let onBody = 0;
    for (let i = -6; i <= 6; i++) {
      for (let j = -2; j <= 2; j++) {
        const hit = await probe(i * 0.2, j * 0.2);
        if (hit.entity_id === corpse.id) onBody++;
      }
    }
    assert.ok(
      onBody > 20,
      `the corpse's selection collider should cover its body; only ${onBody} of 65 samples hit it`,
    );

    // Production interaction: aim at a point a half body-length off the root
    // joint - the part of the corpse the old point-sized collider missed.
    await teleportVerified(game, STANDING_SPOT);
    await game.step({ frames: 10 });
    await game.input.lookAtWorldPoint([cx + 0.6, cy + 0.2, cz]);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });

    const ui = await game.ui.state();
    assert.ok(
      ui.active_panel,
      `aiming along the corpse and squeezing should open its loot MFD (got ${JSON.stringify(ui)})`,
    );
    assert.equal(
      ui.active_panel.entity_id,
      corpse.id,
      "the active panel should be bound to the corpse entity",
    );
    await game.screenshot("medsci1-corpse-body-frob.png");
  },
);
