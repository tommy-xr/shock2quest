import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// This regression depends on a deep campaign save, which contains player state
// and retail-derived mission data and therefore cannot live in the repository.
// Opt in by naming the locally preserved save (without `.sav`):
//
//   SHOCK2_CORPSE_BLOCK_SAVE=campaign_ops_shodan_25th_iter03_corpse_block_20260808 \
//     SHOCK2_E2E=1 node --test dist/test/ops2-corpse-corridor.e2e.test.js
const saveName = process.env.SHOCK2_CORPSE_BLOCK_SAVE;
const e2eEnabled = process.env.SHOCK2_E2E === "1" && saveName !== undefined;

// Stable mission-object id, not a runtime entity id: the Protocol Droid that
// died across ops2's Crew-lower catwalk near (63.25, -9.50, 107.33).
const CATWALK_PROTOCOL_DROID = 354;

test(
  "ops2 exact save: the retained Protocol Droid corpse does not seal the Crew catwalk",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
    });

    const loaded = await game.load(saveName!);
    assert.equal(loaded.success, true);
    assert.equal(loaded.mission, "ops2.mis");
    await game.step({ frames: 5 });

    const matches = await game.entities.byTemplate(CATWALK_PROTOCOL_DROID);
    assert.equal(
      matches.length,
      1,
      `expected the saved Protocol Droid object ${CATWALK_PROTOCOL_DROID}`,
    );
    const corpse = matches[0]!;
    const detail = await game.entities.detail(corpse.id);
    assert.equal(
      detail.properties.find((property) => property.name === "AIBehavior")?.value,
      "Dead",
      "the preserved droid must be a terminal corpse, not a live obstruction",
    );

    const bodies = await game.physics.bodies({ entityId: corpse.id });
    assert.equal(bodies.bodies.length, 1, "the corpse keeps one grounded interaction body");
    assert.equal(
      bodies.bodies[0]!.blocks_player,
      false,
      "a retained corpse body must not stop the player capsule",
    );

    // Production interaction still sees and frobs the corpse: use the real
    // camera ray and squeeze edge, then require its ordinary (empty) SEARCH
    // panel. This is the player-observable guard that the fix did not delete
    // the terminal pose or its creaturecontainer target.
    await game.player.aimAt(corpse, { hitbox: "center", visibility: "required" });
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    const ui = await game.ui.state();
    assert.ok(ui.active_panel, "the retained corpse should still open its SEARCH panel");
    assert.equal(ui.active_panel.entity_id, corpse.id);
    await game.screenshot("ops2-corpse-corridor-search.png");

    // Dismiss the SEARCH panel through the normal bare-view click, then walk
    // straight through the only route. The railings close both edges, so
    // reaching z<106.4 proves normal movement crossed the corpse's old capsule.
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 2 });
    const start = (await game.info()).player.position;
    const move = await game.player.moveTo({ x: start[0], y: start[1], z: 104.8 });
    await game.step({ frames: 10 });
    const end = (await game.info()).player.position;
    assert.equal(move.blocked, false, `corpse crossing was blocked: ${JSON.stringify(move)}`);
    assert.ok(
      end[2] < 106.4,
      `the player must cross south of the corpse: z ${start[2]} -> ${end[2]}`,
    );
    await game.screenshot("ops2-corpse-corridor-crossed.png");

    const stillThere = await game.entities.byTemplate(CATWALK_PROTOCOL_DROID);
    assert.equal(stillThere.length, 1, "crossing must not delete the corpse entity");
    assert.equal(stillThere[0]!.id, corpse.id);
  },
);
