import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanel } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

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

function button(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(
    found,
    `panel should expose button ${label} (got ${JSON.stringify(
      panel.elements.map((e) => e.label ?? e.texture),
    )})`,
  );
  return found;
}

function hasTexture(panel: UiPanel, texture: string): boolean {
  return panel.elements.some(
    (element) => element.texture?.toLowerCase() === texture,
  );
}

async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "the crate interaction should keep an MFD panel open");
  return panel;
}

/** Stand next to a crate and frob it through the normal squeeze path. */
async function frobCrate(game: GameServer, crateId: number): Promise<void> {
  const [x, y, z] = (await game.entities.detail(crateId)).position;
  await teleportVerified(game, { x: x + 1.2, y: y + 0.5, z: z + 1.2 });
  await game.step({ frames: 3 });
  await game.entities.sendMessage(crateId, { type: "Frob" });
  await game.step({ frames: 5 });
}

/** Total every carried nanite StackCount exposed through inventory. */
async function carriedNanites(game: GameServer): Promise<number> {
  const inventory = await game.player.inventory();
  let total = 0;
  for (const item of inventory.items) {
    if (!item.name?.toLowerCase().includes("nanite")) continue;
    const detail = await game.entities.detail(item.entity_id);
    const stack = detail.properties.find((p) => p.name === "StackCount");
    if (stack) total += Number(stack.value);
  }
  return total;
}

async function containsLinks(game: GameServer, entityId: number) {
  const detail = await game.entities.detail(entityId);
  return detail.outgoing_links.filter((l) => l.link_type.startsWith("Contains"));
}

/**
 * Genuinely play the shared HRM board until the crate opens: START (charging
 * the authored cost), then light nodes toward a connected three, re-dealing a
 * board that burned itself out. No direct success message and no assumption
 * that any one roll must land. Winning flips the panel straight to the crate's
 * loot face, which is what this returns.
 */
async function playHackBoardToWin(game: GameServer): Promise<UiPanel> {
  const routes = [
    ["node-2-0", "node-3-0", "node-4-0"],
    ["node-2-1", "node-2-2", "node-2-3"],
    ["node-0-1", "node-0-2", "node-0-3"],
    ["node-4-0", "node-4-1", "node-4-2"],
    ["node-0-3", "node-1-3", "node-2-3"],
  ];
  for (let attempt = 0; attempt < 15; attempt += 1) {
    let panel = await activePanel(game);
    if (hasTexture(panel, "contain.pcx")) return panel;
    // A ruined crate is terminal - fail loudly rather than spin.
    assert.ok(
      !hasTexture(panel, "loseh.pcx"),
      "a critical failure ruined the crate; max Hack skill should leave no mines",
    );

    // Deal a board only when one is not already in play - the caller's paid
    // START must not be thrown away by an immediate RESET. A burned-out board
    // is re-dealt with RESET, which charges the authored cost again, exactly
    // as retail does.
    const inPlay = panel.elements.some(
      (element) => element.label === "reset-hack",
    );
    const burnedOut = hasTexture(panel, "failh.pcx");
    if (!inPlay || burnedOut) {
      const deal = panel.elements.find(
        (element) =>
          element.label === "start-hack" || element.label === "reset-hack",
      );
      assert.ok(deal, "an unwon board should offer START/RESET");
      await clickUiElement(game, deal);
      assert.ok(
        !hasTexture(await activePanel(game), "payh.pcx"),
        "the test wallet should always cover the authored hack cost",
      );
    }

    for (const label of routes[attempt % routes.length]) {
      panel = await activePanel(game);
      if (hasTexture(panel, "contain.pcx")) return panel;
      if (hasTexture(panel, "failh.pcx") || hasTexture(panel, "loseh.pcx")) break;
      await clickUiElement(game, button(panel, label));
    }
  }
  const final = await activePanel(game);
  assert.fail(
    `the HRM board should open the crate within the attempt budget; rng=${game
      .logs()
      .filter((line) => line.includes("HRM rng"))
      .slice(-6)
      .join(" | ")}`,
  );
  return final;
}

test(
  "hydro1 security crate: hack the board open, then loot the Small HE Clip",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9101),
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
    const nanitesBefore = await carriedNanites(game);
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
      await carriedNanites(game),
      nanitesBefore - 5,
      "starting a hack should charge exactly the authored cost",
    );
    // --- KEY: connecting three nodes opens the crate, and the panel turns
    // into the ordinary loot MFD showing the authored contents. ---
    const loot = await playHackBoardToWin(game);
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
