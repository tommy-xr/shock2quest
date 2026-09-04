import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the O/S upgrade (trait) machine MFD (flat UI 6f / PR C3).
// Opt-in (SHOCK2_E2E=1); requires game assets in Data/.
//
// The trait machine shows all 16 O/S traits (free, per the original), but only
// choices with live effects can consume its one-time pick. The chosen trait is
// stored on the persistent character sheet, the machine becomes single-use (a
// quest bit keyed by its stable mission object id), and implemented effects
// apply (Tank: +5 max AND current HP -
// original behavior: buying at 25/30 yields 30/35 - live and re-derived
// across loads).
//
// Negative-first: on the C2 base, `traitmachine` is a NoopScript - frobbing
// medsci2's Trait Machine (mission id 133) opens nothing, so the "panel
// opened" assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Find the level's Trait Machine by stable mission object id. */
async function findMachine(game: GameServer, missionId: number) {
  const entities = (await game.entities.list({ filter: "Trait Machine" })).entities;
  const machine = entities.find((e) => e.template_id === missionId);
  assert.ok(machine, `the level should contain Trait Machine (mission id ${missionId})`);
  return machine;
}

/** Teleport next to an entity, probing a few offsets for stable footing
 * (some machines sit next to pits; a bad offset leaves the player falling
 * out of the panel's walk-away radius). */
async function standNear(game: GameServer, id: number) {
  const detail = await game.entities.detail(id);
  const [x, y, z] = detail.position;
  for (const [dx, dz] of [
    [0, 1.2],
    [0, -1.2],
    [1.2, 0],
    [-1.2, 0],
  ]) {
    await teleportVerified(game, { x: x + dx, y: y + 0.5, z: z + dz });
    await game.step({ frames: 30 });
    const p = (await game.info()).player.position;
    const dist = Math.hypot(p[0] - x, p[1] - y, p[2] - z);
    if (dist < 3.0) return;
  }
  assert.fail("could not find stable footing near the trait machine");
}

async function clickElement(game: GameServer, el: UiElement) {
  const [x, y, w, h] = el.screen_rect;
  await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

const traitButton = (label: string, els: UiElement[]) => {
  const el = els.find((e) => e.kind === "button" && e.label === label);
  assert.ok(el, `panel should expose a trait button labeled "${label}"`);
  return el;
};

test(
  "O/S trait machine: one-time pick, stored persistently, machine single-use",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `os_traits_e2e_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
    });
    await game.step({ frames: 5 });

    const stats = async () => {
      const s = (await game.info()).player.stats;
      assert.ok(s, "player should have a character sheet");
      return s;
    };
    assert.deepEqual((await stats()).os_traits, [], "fresh character has no O/S traits");
    const before = (await game.info()).player;
    const hpBefore = before.max_hit_points;
    assert.ok(hpBefore != null, "player has a hit-point pool");
    assert.equal(before.hit_points, hpBefore, "the player starts at full health");

    // --- Frob the medsci2 machine: the panel must open (fails on the C2
    // base, where traitmachine is a NoopScript). ---
    const machine = await findMachine(game, 133);
    await standNear(game, machine.id);
    assert.ok(!(await game.ui.state()).active_panel, "no panel before the frob");
    await game.entities.sendMessage(machine.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const opened = await game.ui.state();
    assert.ok(opened.active_panel, "frobbing the Trait Machine should open its panel");
    await game.screenshot("traits-panel-open.png");

    // --- All 16 traits are labeled, clickable buttons. ---
    const els = opened.active_panel.elements;
    for (const name of [
      "Strong Metabolism", "Pharmo-Friendly", "Pack-Rat", "Speedy",
      "Sharpshooter", "Naturally Able", "Cybernetically Enhanced", "Tank",
      "Lethal Weapon", "Security Expert", "Smasher", "Cyber-Assimilation",
      "Replicator Expert", "Power Psi", "Tinker", "Spatially Aware",
    ]) {
      traitButton(name, els);
    }

    // --- A trait with no live effect yet refuses clearly and leaves this
    // one-shot machine available for a live choice. ---
    await clickElement(game, traitButton("Sharpshooter", els));
    await game.step({ frames: 3 });
    assert.deepEqual((await stats()).os_traits, [], "unsupported trait is not recorded");
    let refreshed = await game.ui.state();
    assert.ok(
      refreshed.active_panel!.elements.some(
        (e) => e.kind === "text" && e.text?.includes("Upgrade unavailable"),
      ),
      "the panel explains why the trait cannot be selected",
    );

    // --- Pick Tank: stored + live +5 max AND current HP (the original
    // raises both - buying wounded at 25/30 yields 30/35). The player can't
    // be wounded headlessly (it has no scripts, so a debug Damage message is
    // dropped), but the current-HP grant is still observable at full health:
    // ceiling-only would leave 30/35, the original behavior yields 35/35. ---
    await clickElement(game, traitButton("Tank", refreshed.active_panel!.elements));
    await game.step({ frames: 3 });
    const afterPick = await stats();
    assert.deepEqual(afterPick.os_traits, [8], "Tank (trait 8) is recorded");
    const hpAfter = (await game.info()).player;
    assert.equal(
      hpAfter.max_hit_points,
      (hpBefore as number) + 5,
      "Tank grants +5 max HP live",
    );
    assert.equal(
      hpAfter.hit_points,
      (hpBefore as number) + 5,
      "Tank also grants +5 current HP live (original behavior; not left at the old max)",
    );
    await game.screenshot("traits-after-pick.png");

    // --- The machine is now single-use: a second pick refuses. ---
    refreshed = await game.ui.state();
    assert.ok(refreshed.active_panel, "panel stays open after the pick");
    await clickElement(game, traitButton("Speedy", refreshed.active_panel.elements));
    await game.step({ frames: 3 });
    assert.deepEqual(
      (await stats()).os_traits,
      [8],
      "the used machine must refuse a second trait",
    );
    const refusalUi = await game.ui.state();
    assert.ok(
      refusalUi.active_panel!.elements.some(
        (e) => e.kind === "text" && e.text?.includes("already been upgraded"),
      ),
      "the panel shows the shipped machine-used message (MISC.STR TraitMachineUsed)",
    );

    // --- Used state + trait survive save/load. ---
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 5 });
    assert.deepEqual((await stats()).os_traits, [8], "trait survives save/load");
    const hpLoaded = (await game.info()).player;
    assert.equal(
      hpLoaded.max_hit_points,
      (hpBefore as number) + 5,
      "Tank max-HP bonus re-derives after load (not doubled)",
    );
    // Current and maximum HP persist together. This full-health snapshot stays
    // exactly 35/35, so the live +5 grant cannot double-apply on load.
    assert.equal(
      hpLoaded.hit_points,
      hpLoaded.max_hit_points,
      "current HP persists exactly at the trait-adjusted max",
    );
    const machine2 = await findMachine(game, 133);
    await standNear(game, machine2.id);
    await game.entities.sendMessage(machine2.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const reopened = await game.ui.state();
    assert.ok(reopened.active_panel, "the used machine still opens its panel");
    await clickElement(game, traitButton("Speedy", reopened.active_panel.elements));
    await game.step({ frames: 3 });
    assert.deepEqual(
      (await stats()).os_traits,
      [8],
      "the used machine refuses after save/load too",
    );

    // --- A different machine still works: hydro2's Trait Machine (mission
    // id 879) vends a second trait, and traits accumulate. ---
    await game.transitionLevel("hydro2.mis");
    await game.step({ frames: 5 });
    assert.equal((await game.info()).mission, "hydro2.mis");
    assert.deepEqual((await stats()).os_traits, [8], "trait survives the transition");

    const hydroMachine = await findMachine(game, 879);
    await standNear(game, hydroMachine.id);
    await game.entities.sendMessage(hydroMachine.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const hydroPanel = await game.ui.state();
    assert.ok(hydroPanel.active_panel, "the second machine opens its panel");
    const modulesBefore = (await stats()).cyber_modules;
    await clickElement(
      game,
      traitButton("Naturally Able", hydroPanel.active_panel.elements),
    );
    await game.step({ frames: 3 });
    const finalStats = await stats();
    assert.deepEqual(
      finalStats.os_traits,
      [8, 6],
      "the second machine vends a second trait",
    );
    assert.equal(
      finalStats.cyber_modules,
      modulesBefore + 8,
      "Naturally Able grants its one-time +8 cyber modules",
    );
  },
);
