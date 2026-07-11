import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the trainer / upgrade station MFD (flat UI 6e / PR C2).
// Opt-in (SHOCK2_E2E=1); requires game assets in Data/.
//
// The four medsci1 trainer machines open category upgrade panels: rows list
// the current level and the cost of the next level from the gamesys cost
// tables (STATCOST 3/8/15/30/50 etc.), and buying spends cyber modules and
// raises the stat persistently (save/load + level transition).
//
// Negative-first: on the C1 base, frobbing the Stats Trainer hits the
// intentional SkillTrainerScript no-op stub (#424) - /v1/ui reports no active
// panel, so the "panel opened" assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8170);

test(
  "trainer MFD: frob opens the stats panel, buying Strength spends modules persistently",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `trainer_e2e_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 5 });

    const stats = async () => {
      const s = (await game.info()).player.stats;
      assert.ok(s, "player should have a character sheet");
      return s;
    };

    // --- Fund the player: fire every module award in the level (EXP traps
    // carry PropExp, cookie piles their stack count). Discovery is by name +
    // script, never by runtime id. ---
    let expected = 0;
    for (const e of (await game.entities.list({ filter: "Experience Trap" })).entities) {
      const detail = await game.entities.detail(e.id);
      const exp = detail.properties.find((p) => p.name === "Exp");
      if (exp && Number(exp.value) > 0) {
        await game.entities.sendMessage(e.id, { type: "TurnOn" });
        expected += Number(exp.value);
      }
    }
    for (const e of (await game.entities.list({ filter: "EXP" })).entities) {
      const detail = await game.entities.detail(e.id);
      const isCookie = detail.properties.some(
        (p) => p.name === "Scripts" && p.value.includes("ExpCookie"),
      );
      const stack = detail.properties.find((p) => p.name === "StackCount");
      if (isCookie && stack && Number(stack.value) > 0) {
        await game.entities.sendMessage(e.id, { type: "Frob" });
        expected += Number(stack.value);
      }
    }
    await game.step({ frames: 5 });
    const funded = await stats();
    assert.equal(funded.cyber_modules, expected, "all module awards landed");
    assert.ok(expected >= 3, `need at least 3 modules to buy Strength (got ${expected})`);
    const baseStrength = funded.strength;

    // --- Find the Stats Trainer (mission id 1352 is its stable template_id)
    // and get within the panel's walk-away radius. ---
    const trainers = (await game.entities.list({ filter: "Trainer" })).entities;
    const statsTrainer = trainers.find((e) => e.template_id === 1352);
    assert.ok(statsTrainer, "medsci1 should contain the Stats Trainer (mission id 1352)");
    const trainerDetail = await game.entities.detail(statsTrainer.id);
    // Stand just in front of the machine (offsets probed on real geometry: a
    // diagonal +1/+1 offset here is over a pit and the player falls out of
    // the panel's 4-unit walk-away radius). Settle physics before frobbing.
    const [tx, ty, tz] = trainerDetail.position;
    await teleportVerified(game, { x: tx, y: ty + 0.5, z: tz + 1.2 });
    await game.step({ frames: 30 });

    // --- Frob opens the panel (fails on the C1 base: no-op stub). ---
    assert.ok(!(await game.ui.state()).active_panel, "no panel before the frob");
    await game.entities.sendMessage(statsTrainer.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const opened = await game.ui.state();
    assert.ok(
      opened.active_panel,
      "frobbing the Stats Trainer should open its upgrade panel",
    );
    assert.equal(opened.active_panel.entity_id, statsTrainer.id);
    await game.screenshot("trainer-panel-open.png");

    // --- The panel lists labeled rows with current level + cost from the
    // gamesys STATCOST table (Strength 1 -> 2 costs 3). ---
    const textEl = (needle: string, els: UiElement[]) =>
      els.find((e) => e.kind === "text" && e.text?.includes(needle));
    const els = opened.active_panel.elements;
    for (const label of ["Strength", "Endurance", "Agility", "Psionics", "Cybernetics"]) {
      assert.ok(textEl(label, els), `panel should list a "${label}" row`);
    }
    const strengthDetail = textEl(`lvl ${baseStrength} > ${baseStrength + 1}: 3 cm`, els);
    assert.ok(
      strengthDetail,
      `the Strength row should quote the STATCOST cost (3 cm), got: ${JSON.stringify(
        els.filter((e) => e.kind === "text").map((e) => e.text),
      )}`,
    );
    assert.ok(textEl(`modules: ${expected}`, els), "panel shows the module pool");

    // --- Click the Strength row's buy button by its semantic label (the
    // shared button-label mechanism from the elevator PR). ---
    const rowButton = (label: string, from: UiElement[]) => {
      const el = from.find((e) => e.kind === "button" && e.label === label);
      assert.ok(el, `the panel should expose a buy button labeled "${label}"`);
      return el;
    };
    const clickElement = async (el: UiElement) => {
      const [x, y, w, h] = el.screen_rect;
      await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 1);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
    };
    await clickElement(rowButton("Strength", els));
    await game.step({ frames: 3 });

    // --- The purchase applied: strength +1, modules -3, panel refreshed. ---
    const afterBuy = await stats();
    assert.equal(afterBuy.strength, baseStrength + 1, "Strength rose by one");
    assert.equal(afterBuy.cyber_modules, expected - 3, "3 modules were spent");
    const refreshed = await game.ui.state();
    assert.ok(refreshed.active_panel, "panel stays open after a purchase");
    assert.ok(
      textEl(`modules: ${expected - 3}`, refreshed.active_panel.elements),
      "the module pool readout refreshed",
    );
    await game.screenshot("trainer-after-buy.png");

    // --- Refusal: the next Strength level costs 8 (STATCOST row 2); drain
    // the balance below it by buying if needed, then assert refusal. ---
    // With the medsci1 awards (9 modules) the balance is now 6 < 8, so a
    // second click must refuse: message shown, nothing changes.
    const balance = afterBuy.cyber_modules;
    if (balance < 8) {
      await clickElement(rowButton("Strength", refreshed.active_panel.elements));
      await game.step({ frames: 3 });
      const afterRefusal = await stats();
      assert.equal(afterRefusal.strength, afterBuy.strength, "refused buy: no stat change");
      assert.equal(
        afterRefusal.cyber_modules,
        balance,
        "refused buy: no modules spent",
      );
      const refusalUi = await game.ui.state();
      assert.ok(
        textEl("Insufficient cyber modules", refusalUi.active_panel!.elements),
        "the panel shows the insufficient-modules error",
      );
      await game.screenshot("trainer-refusal.png");
    }

    // --- Persistence: save/load, then a level transition. ---
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 3 });
    const afterLoad = await stats();
    assert.equal(afterLoad.strength, baseStrength + 1, "strength survives save/load");
    assert.equal(afterLoad.cyber_modules, expected - 3, "balance survives save/load");

    await game.transitionLevel("eng1.mis");
    await game.step({ frames: 3 });
    const afterTransition = await stats();
    assert.equal(
      afterTransition.strength,
      baseStrength + 1,
      "strength survives a level transition",
    );
    assert.equal(
      afterTransition.cyber_modules,
      expected - 3,
      "balance survives a level transition",
    );
  },
);
