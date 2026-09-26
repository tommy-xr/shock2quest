import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

// End-to-end test for auto-installing software (issue #812).
//
// Softs (`AutoInstallSoft`) are not carryable: the retail `Softs` archetype
// authors a SCRIPT world *and* inventory frob action, so picking one up
// installs it on the character sheet and consumes the object. The sheet keeps
// the higher version (a V3 supersedes a V2).
//
// Use the authored Hack Soft V2 in Hydro1 (corpse 471 -> obj 1000), then take
// Hack Soft V3 from Command1 corpse 2255 through its loot MFD. The old Command1
// world fixture 131 is restricted to the obsolete playtest difficulty
// (P$DiffPer=1) and is correctly absent on every retail difficulty.
// Direct Frob covers the item script; the second pickup covers normal loot UI.
//
// Negative-first: on main `autoinstallsoft` maps to `UnimplementedScript`, so
// the world frob logs "Unimplemented script" and leaves the soft in the world
// (`software.hack` stays 0), and the loot-MFD click routes a use-only item's
// click to that same no-op script - the reported symptom, "10 clicks in the
// container MFD do nothing".
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const HACK_SOFT_V2 = 1000; // Hydro1 corpse 471 contains this authored item
const CORPSE_WITH_HACK_SOFT_V3 = 2255; // Female Corpse 1 -> Contains -> obj 2095

test(
  "softs auto-install on use and loot, preserving versions across decks",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro1.mis",
    });
    await game.step({ frames: 5 });

    const software = async () => {
      const stats = (await game.info()).player.stats;
      assert.ok(stats, "the mission should expose a character sheet");
      return stats.software;
    };
    const byTemplate = async (templateId: number, what: string) => {
      const matches = await game.entities.byTemplate(templateId);
      assert.equal(matches.length, 1, `expected exactly one ${what} (mission id ${templateId})`);
      return matches[0];
    };

    assert.deepEqual(
      await software(),
      { hack: 0, modify: 0, repair: 0, research: 0 },
      "a fresh character has no software installed",
    );

    // --- (i) direct use: the soft installs and is consumed ---
    const worldSoft = await byTemplate(HACK_SOFT_V2, "Hack Soft V2");
    assert.equal(worldSoft.name, "Hack Soft V2");
    await game.entities.sendMessage(worldSoft.id, { type: "Frob" });
    await game.step({ frames: 5 });

    assert.equal(
      (await software()).hack,
      2,
      "frobbing Hack Soft V2 must raise the installed hack software to version 2",
    );
    // NEGATIVE KEY: a soft never occupies inventory - it is consumed.
    const afterWorldPickup = await game.player.inventory();
    assert.ok(
      !afterWorldPickup.items.some((i) => i.entity_id === worldSoft.id),
      `an installed soft must not enter the inventory (got ${JSON.stringify(afterWorldPickup.items)})`,
    );
    assert.equal(
      (await game.entities.byTemplate(HACK_SOFT_V2)).length,
      0,
      "the installed soft is consumed - it no longer exists in the world",
    );
    assert.deepEqual(
      { modify: (await software()).modify, repair: (await software()).repair },
      { modify: 0, repair: 0 },
      "installing a hack soft must not touch the other software slots",
    );

    await game.transitionLevel("command1.mis");
    await game.step({ frames: 5 });
    assert.equal((await software()).hack, 2, "installed version survives the deck transition");

    // --- (ii) container MFD: taking a soft installs it, superseding V2 ---
    const corpse = await byTemplate(CORPSE_WITH_HACK_SOFT_V3, "Female Corpse 1");
    const contained = (await game.entities.detail(corpse.id)).outgoing_links.filter((l) =>
      l.link_type.startsWith("Contains"),
    );
    const softLink = contained.find((l) => l.target_name.includes("Hack Soft V3"));
    assert.ok(softLink, `corpse 2255 should contain a Hack Soft V3 (got ${JSON.stringify(contained)})`);

    await teleportVerified(game, {
      x: corpse.position[0] + 1.0,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.0,
    });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });

    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel, "frobbing the corpse should open its loot MFD panel");
    const softElement = panel.elements.find((e) => e.entity_id === softLink.target_id);
    assert.ok(
      softElement,
      `the loot panel should expose the Hack Soft V3 (got ${JSON.stringify(panel.elements)})`,
    );
    await game.screenshot("soft-loot-panel.png");

    await clickUiElement(game, softElement);

    assert.equal(
      (await software()).hack,
      3,
      "taking Hack Soft V3 from the corpse must supersede the installed version 2",
    );
    const afterTake = await game.player.inventory();
    assert.ok(
      !afterTake.items.some((i) => i.entity_id === softLink.target_id),
      `a looted soft must not enter the inventory (got ${JSON.stringify(afterTake.items)})`,
    );
    const afterPanel = (await game.ui.state()).active_panel;
    assert.ok(
      !afterPanel?.elements.some((e) => e.entity_id === softLink.target_id),
      "the installed soft disappears from the loot panel",
    );
    await game.screenshot("soft-loot-taken.png");

    // --- (iii) the installed version persists through save/load ---
    const saveName = `auto_install_soft_e2e_${Date.now()}`;
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 5 });
    assert.equal(
      (await software()).hack,
      3,
      "the installed software version must survive save/load",
    );
  },
);
