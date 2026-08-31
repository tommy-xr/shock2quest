import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanel } from "../src/types.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import {
  activePanel,
  button,
  hasTexture,
  playHackBoardToWin,
} from "./helpers/hack.js";

// End-to-end coverage for hydro1's security crates (#811): the `HackableCrate`
// archetype was an UnimplementedScript, so the crates were inert props and
// their authored loot was unreachable for the whole game (43 crates across
// 16 shipped missions; hydro1's only Small HE Clip is sealed in crate 325).
//
// The scenario is the authored one, discovered at run time by TEMPLATE ID
// (runtime entity ids are NOT stable across launches; `template_id` is the
// mission object id and IS stable):
//   - crate 325  -> Contains -> Small HE Clip (the issue's starved ammo)
//
// Both crates are authored `P$ObjState(Locked)` + `P$HackDiff` (cost 5), with
// the archetype's own HackText: "Hack to open crate. Critical failure
// destroys it."
//
// Negative-first (verified against main, cd00795f): frobbing crate 325 opens
// NO panel at all (`/v1/ui.active_panel` is null), so the "hack board opens"
// assertion fails and the clip can never be reached.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CLIP_CRATE = 325; // Contains -> Small HE Clip
const BIG_NANITE_PILE = -1591;




/** Stand next to a crate and frob it through the normal squeeze path. */
async function frobCrate(game: GameServer, crateId: number): Promise<void> {
  const [x, y, z] = (await game.entities.detail(crateId)).position;
  await teleportVerified(game, { x: x + 1.2, y: y + 0.5, z: z + 1.2 });
  await game.step({ frames: 3 });
  await game.entities.sendMessage(crateId, { type: "Frob" });
  await game.step({ frames: 5 });
}

async function containsLinks(game: GameServer, entityId: number) {
  const detail = await game.entities.detail(entityId);
  return detail.outgoing_links.filter((l) => l.link_type.startsWith("Contains"));
}

test(
  "hydro1 security crate: hack the board open, then loot the Small HE Clip",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro1.mis",
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    await game.step({ frames: 5 });

    const [securityCrate] = await game.entities.byTemplate(CLIP_CRATE);
    assert.ok(securityCrate, "hydro1 should contain authored security crate 325");

    // --- The authored loot is real, and sealed: it lives on a Contains link
    // with no world presence of its own. ---
    const links = await containsLinks(game, securityCrate.id);
    assert.equal(links.length, 1, "crate 325 should contain exactly one item");
    const clipId = links[0].target_id;
    assert.ok(
      links[0].target_name.includes("HE Clip"),
      `crate 325's authored loot should be the Small HE Clip, got ${links[0].target_name}`,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: clipId })).bodies.length,
      0,
      "the crated clip must have no world presence until it is looted",
    );

    // --- Provision the hacker: max Hack skill drives the authored -10 base
    // success chance up and the critical (mine) count down, and a nanite pile
    // pays the authored per-attempt cost. ---
    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    await game.player.spawnItem(BIG_NANITE_PILE);
    const nanitesBefore = await carriedNaniteTotal(game);
    assert.ok(nanitesBefore > 0, "the test wallet should hold nanites");

    // --- KEY (#811): frobbing the crate opens the shared retail HRM board.
    // On main this panel does not exist at all. ---
    await frobCrate(game, securityCrate.id);
    const board = await activePanel(game);
    assert.equal(
      board.entity_id,
      securityCrate.id,
      "the open panel should be bound to the crate",
    );
    assert.ok(
      hasTexture(board, "hack.pcx"),
      `a locked crate should present the HRM board (got ${JSON.stringify(
        board.elements.map((e) => e.texture ?? e.label),
      )})`,
    );
    assert.ok(
      !board.elements.some((element) => element.entity_id === clipId),
      "a locked crate must not expose its contents",
    );
    const cost = board.elements
      .filter((element) => element.kind === "text")
      .map((element) => element.text ?? "")
      .find((text) => /^\d+$/.test(text));
    assert.equal(cost, "5", "the board should show the authored HackDiff cost 5");
    await game.screenshot("crate-hack-board.png");

    // --- Genuinely play the board; the first START charges the cost. ---
    await clickUiElement(game, button(board, "start-hack"));
    assert.equal(
      await carriedNaniteTotal(game),
      nanitesBefore - 5,
      "starting a hack should charge exactly the authored cost",
    );
    // --- KEY: connecting three nodes opens the crate, and the panel turns
    // into the ordinary loot MFD showing the authored contents. ---
    const loot = await playHackBoardToWin(game, (panel) => hasTexture(panel, "contain.pcx"));
    assert.ok(
      hasTexture(loot, "contain.pcx"),
      "a won hack should turn the crate into the normal loot MFD",
    );
    assert.ok(
      !hasTexture(loot, "hack.pcx"),
      "an opened crate should not present the board again",
    );
    const clipElement = loot.elements.find(
      (element) => element.kind === "button" && element.entity_id === clipId,
    );
    assert.ok(
      clipElement,
      `the hacked crate should list its Small HE Clip (got ${JSON.stringify(
        loot.elements.map((e) => e.label ?? e.texture),
      )})`,
    );
    await game.screenshot("crate-loot-panel.png");

    // --- Loot it through the container MFD: the clip reaches the backpack. ---
    await clickUiElement(game, clipElement);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === clipId,
      ),
      "clicking the clip should take it into the player's backpack",
    );
    assert.equal(
      (await containsLinks(game, securityCrate.id)).length,
      0,
      "the looted crate should have no Contains link left",
    );

    // --- The opened state is persistent (SetObjectState writes P$ObjState). ---
    const saveName = `hackable_crate_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });

    const [reloadedCrate] = await game.entities.byTemplate(CLIP_CRATE);
    assert.ok(reloadedCrate, "save/load should restore the authored crate");
    assert.ok(
      (await game.player.inventory()).items.some((item) =>
        item.name?.toLowerCase().includes("he clip"),
      ),
      "the looted clip should survive save/load",
    );
    await frobCrate(game, reloadedCrate.id);
    const reloadedPanel = await activePanel(game);
    assert.ok(
      hasTexture(reloadedPanel, "contain.pcx") &&
        !hasTexture(reloadedPanel, "hack.pcx"),
      "a hacked crate must stay hacked across save/load, not relock",
    );
  },
);
