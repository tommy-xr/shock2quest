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
// tables (STATCOST 3/8/15/30/50 etc.). Only stats with live effects may be
// bought: Strength expands the backpack, Endurance raises maximum HP,
// Cybernetics feeds the hacking path, and storage-only stats refuse without
// spending modules.
//
// Negative-first: on the C1 base, frobbing the Stats Trainer hits the
// intentional SkillTrainerScript no-op stub (#424) - /v1/ui reports no active
// panel, so the "panel opened" assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8170);

test(
  "trainer MFD: Strength expands inventory and Endurance raises max HP persistently",
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

    // Provision only currency/items; purchases still go through the real MFD,
    // button messages, effect queue, cost table, and live player properties.
    const funded = await game.player.setStats({ cyber_modules: 29 });
    assert.equal(funded.cyber_modules, 29);
    assert.equal(funded.endurance, 1, "the regression starts at Endurance 1");
    await Promise.all([
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
      game.player.spawnItem("Nanites"),
    ]);
    const backpackOrdinals = async () => {
      const backpackId = (await game.info()).player.inventory_entity_id;
      assert.ok(backpackId != null);
      return (await game.entities.detail(backpackId)).outgoing_links
        .filter((link) => link.link_type.startsWith("Contains"))
        .map((link) => link.contains_ordinal)
        .filter((slot): slot is number => slot != null)
        .sort((a, b) => a - b);
    };
    assert.deepEqual(await backpackOrdinals(), [0, 10, 20]);
    const beforePlayer = (await game.info()).player;
    const baseMaxHp = beforePlayer.max_hit_points;
    assert.ok(baseMaxHp != null, "player should have a maximum-HP pool");

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
    // gamesys STATCOST table (Endurance 1 -> 2 costs 3). Unsupported stats
    // remain listed but clearly unavailable instead of quoting a price. ---
    const textEl = (needle: string, els: UiElement[]) =>
      els.find((e) => e.kind === "text" && e.text?.includes(needle));
    const els = opened.active_panel.elements;
    for (const label of ["Strength", "Endurance", "Agility", "Psionics", "Cybernetics"]) {
      assert.ok(textEl(label, els), `panel should list a "${label}" row`);
    }
    const enduranceDetail = textEl("lvl 1 > 2: 3 cm", els);
    assert.ok(
      enduranceDetail,
      `the Endurance row should quote the STATCOST cost (3 cm), got: ${JSON.stringify(
        els.filter((e) => e.kind === "text").map((e) => e.text),
      )}`,
    );
    assert.ok(textEl("unavailable", els), "storage-only stats are marked unavailable");
    assert.ok(textEl("modules: 29", els), "panel shows the module pool");

    // --- A storage-only stat refuses without taking currency. The same
    // machine then remains usable for a supported purchase. ---
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
    await clickElement(rowButton("Agility", els));
    await game.step({ frames: 3 });
    assert.deepEqual(await stats(), funded, "refused stat leaves the sheet and modules unchanged");
    assert.equal(
      (await game.info()).player.max_hit_points,
      baseMaxHp,
      "refused stat leaves live HP unchanged",
    );
    let refreshed = await game.ui.state();
    assert.ok(
      textEl("Upgrade unavailable", refreshed.active_panel!.elements),
      "the panel explains the refusal",
    );

    // Strength is now a live purchase: its 1->2 upgrade costs 3 modules and
    // re-encodes existing row ordinals for the new eleven-column backpack.
    await clickElement(rowButton("Strength", refreshed.active_panel!.elements));
    await game.step({ frames: 3 });
    const afterStrength = await stats();
    assert.equal(afterStrength.strength, 2);
    assert.equal(afterStrength.cyber_modules, 26);
    assert.deepEqual(await backpackOrdinals(), [0, 11, 22]);
    assert.equal(
      (await game.info()).player.max_hit_points,
      baseMaxHp,
      "Strength does not change live HP",
    );
    refreshed = await game.ui.state();
    assert.ok(
      textEl("Upgrade complete", refreshed.active_panel!.elements),
      "the panel confirms the live Strength purchase",
    );

    // --- Reproduce issue #813's exact transaction: levels 1->4 cost
    // 3 + 8 + 15 = 26 modules. Each purchase changes the live maximum-HP pool
    // by the retail Normal-difficulty five points. ---
    for (const [level, cost] of [[2, 3], [3, 8], [4, 15]] as const) {
      await clickElement(rowButton("Endurance", refreshed.active_panel!.elements));
      await game.step({ frames: 3 });
      const afterBuy = await stats();
      assert.equal(afterBuy.endurance, level, `Endurance rose to ${level}`);
      const spent = level === 2 ? 3 : level === 3 ? 11 : 26;
      assert.equal(afterBuy.cyber_modules, 26 - spent, `${cost} modules were spent`);
      assert.ok(
        textEl("Max HP +5", (await game.ui.state()).active_panel!.elements),
        "the panel reports the live Endurance benefit",
      );
      refreshed = await game.ui.state();
    }
    const afterPurchases = (await game.info()).player;
    assert.equal(afterPurchases.max_hit_points, baseMaxHp + 15, "three levels grant +15 max HP");
    assert.equal(afterPurchases.hit_points, beforePlayer.hit_points, "Endurance does not heal");
    await game.screenshot("trainer-after-endurance-4.png");

    // --- Persistence: save/load, then a level transition. ---
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 3 });
    const afterLoad = await stats();
    assert.equal(afterLoad.strength, 2, "Strength survives save/load");
    assert.equal(afterLoad.endurance, 4, "Endurance survives save/load");
    assert.equal(afterLoad.cyber_modules, 0, "spent balance survives save/load");
    assert.equal(
      (await game.info()).player.max_hit_points,
      baseMaxHp + 15,
      "Endurance-derived max HP survives save/load",
    );

    await game.transitionLevel("eng1.mis");
    await game.step({ frames: 3 });
    const afterTransition = await stats();
    assert.equal(
      afterTransition.strength,
      2,
      "Strength survives a level transition",
    );
    assert.equal(
      afterTransition.endurance,
      4,
      "Endurance survives a level transition",
    );
    assert.equal(afterTransition.cyber_modules, 0, "balance survives a level transition");
    assert.equal(
      (await game.info()).player.max_hit_points,
      baseMaxHp + 15,
      "Endurance-derived max HP survives a level transition",
    );
  },
);
