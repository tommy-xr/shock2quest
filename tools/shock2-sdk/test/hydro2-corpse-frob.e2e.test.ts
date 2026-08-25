import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable hydro2 mission-object ids (runtime entity ids are rediscovered every
// launch and must never be hardcoded).
const HYD_MALE_CORPSE = 754; // Contains -> Hydro Card B
const CORPSE_ROOM_STANDING_SPOT = { x: 73.08, y: -0.76, z: -14.92 };

// #795: this corpse lies in an alcove with a SEVEN-FOOT ceiling. The six-foot
// standing body clears it, but the camera used to sit 7.0 ft above the feet -
// ABOVE that ceiling - so the crosshair ray started outside the room and hit
// the ceiling plane 0.05 units in front of the eye. No highlight, no frob, and
// with it the Hydro Card B chain (hydro1/hydro3 bulkheads, ACR1/ACR4).
//
// Negative-first: with the previous `PLAYER_EYE_HEIGHT` of 4.0 ft the aim +
// squeeze below opens no panel at all.
test(
  "hydro2: the crosshair frobs the Hydro Card B corpse under its seven-foot ceiling",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 5 });

    const [corpse] = await game.entities.byTemplate(HYD_MALE_CORPSE);
    assert.ok(corpse, "expected hydro2 HYD Male Corpse obj 754");
    const contains = (await game.entities.detail(corpse.id)).outgoing_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    assert.ok(
      contains.some((link) => link.target_name.includes("Hydro Card B")),
      `corpse 754 should contain Hydro Card B; got ${JSON.stringify(contains)}`,
    );

    // Stand where the campaign playthrough stood, under the low ceiling.
    await teleportVerified(game, CORPSE_ROOM_STANDING_SPOT);
    await game.step({ frames: 30 });

    // Fixture sanity: this really is the low alcove the bug depends on - a
    // ceiling barely above the standing body (6.0 SS2 ft / 2.4 world units).
    const { player } = await game.info();
    const ceiling = await game.raycast({
      start: [player.position[0], player.position[1], player.position[2]],
      end: [player.position[0], player.position[1] + 12, player.position[2]],
      ignore_sensors: true,
      collision_groups: ["world", "entity", "selectable"],
    });
    assert.ok(ceiling.hit, "expected a ceiling above the corpse alcove");
    const clearance = ceiling.hit_point![1] - (player.position[1] - 1.2);
    assert.ok(
      clearance > 2.4 && clearance < 3.0,
      `the corpse alcove should be a low room the body just clears; got ${clearance} world units`,
    );

    await game.screenshot("hydro2-corpse-aim.png");

    // Production interaction: real crosshair aim, real use button.
    await game.player.aimAt(corpse, { hitbox: "center" });
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });

    const ui = await game.ui.state();
    assert.ok(
      ui.active_panel,
      `aiming at the corpse and squeezing should open its loot MFD (got ${JSON.stringify(ui)})`,
    );
    assert.equal(
      ui.active_panel.entity_id,
      corpse.id,
      "the active panel should be bound to the corpse entity",
    );
    assert.ok(
      ui.active_panel.elements.some(
        (element) => element.kind === "button" && element.label === "Hydro Card B",
      ),
      `the loot panel should list Hydro Card B (got ${JSON.stringify(ui.active_panel.elements)})`,
    );
    await game.screenshot("hydro2-corpse-panel.png");
  },
);
