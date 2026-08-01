import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState } from "../src/types.js";
import { clickUiElement } from "./helpers/ui.js";
import { teleportVerified } from "./helpers/teleport.js";

// Hydroponics' authored Toxin-A research objective. Every player-facing use
// travels through production interactions: the inventory strip double-click,
// the Hydro 2 Tech Trainer, real carried chemical entities, and Hydro 1's ACR1
// regulator. Runtime ids are discovered each time (including after save/load).
//
// Negative-first evidence for #763: on origin/main at 5ebca74d the first
// Toxin-A double-click reaches the `ResearchableScript` stub, leaving
// `/v1/ui.active_panel` null. The first research-panel assertion therefore
// fails before the implementation.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function ensureUseMode(game: GameServer): Promise<void> {
  if ((await game.ui.state()).mode !== "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
  }
}

async function useInventoryItem(
  game: GameServer,
  entityId: number,
): Promise<void> {
  await ensureUseMode(game);
  const ui = await game.ui.state();
  const element = ui.strip?.elements.find(
    (candidate: UiElement) => candidate.entity_id === entityId,
  );
  assert.ok(element, `strip should expose carried item ${entityId}`);

  await clickUiElement(game, element);
  assert.equal(
    (await game.ui.state()).cursor?.entity_id,
    entityId,
    "first click should lift the item onto the cursor",
  );
  await clickUiElement(game, element);
  assert.equal(
    (await game.ui.state()).cursor,
    null,
    "second click should use the item and clear the cursor",
  );
  await game.step({ frames: 5 });
}

function panelTexts(ui: UiState): string[] {
  return (
    ui.active_panel?.elements
      .filter((element) => element.kind === "text")
      .flatMap((element) => (element.text ? [element.text] : [])) ?? []
  );
}

async function carriedNamed(game: GameServer, name: string) {
  return (await game.player.inventory()).items.find((item) => item.name === name);
}

async function carriedItemsNamed(game: GameServer, name: string) {
  return (await game.player.inventory()).items.filter((item) => item.name === name);
}

async function researchPanel(game: GameServer): Promise<UiState> {
  const ui = await game.ui.state();
  assert.ok(ui.active_panel, "Toxin-A should have an open Research MFD");
  assert.ok(
    ui.active_panel.elements.some((element) =>
      element.texture?.toLowerCase().endsWith("research.pcx"),
    ),
    "the panel should use the retail RESEARCH.PCX backdrop",
  );
  return ui;
}

test(
  "Hydro Toxin-A: train, research with chemicals, persist, and use both ACRs",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `hydro_research_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8233),
    });
    await game.step({ frames: 5 });

    // Pick up an authored Hydro 2 vial, then prove Hydro 1's real regulator
    // refuses it while its Dark object state is still Unresearched.
    const authoredToxins = (
      await game.entities.list({ filter: "Anti-Annelid Toxin", limit: 20 })
    ).entities.slice(0, 2);
    assert.equal(
      authoredToxins.length,
      2,
      "Hydro 2 should contain both authored Toxin-A vials",
    );
    for (const authoredToxin of authoredToxins) {
      await game.player.give(authoredToxin.id);
    }
    await game.transitionLevel("hydro1.mis");
    await game.step({ frames: 5 });
    const acr1 = (await game.entities.list({ filter: "ACR1", limit: 30 })).entities.find(
      (entity) => entity.name === "ACR1",
    );
    assert.ok(acr1, "Hydro 1 should contain the authored ACR1 regulator");
    await game.entities.sendMessage(acr1.id, { type: "Frob" });
    await game.step({ frames: 30 });
    assert.ok(
      (await carriedItemsNamed(game, "Anti-Annelid Toxin")).length === 2,
      "ACR1 must not consume either unresearched toxin",
    );
    assert.notEqual(
      await game.quests.get("ACR1"),
      "complete",
      "rejected toxin must not advance the ACR objective",
    );

    await game.transitionLevel("hydro2.mis");
    await game.step({ frames: 5 });
    let toxin = await carriedNamed(game, "Anti-Annelid Toxin");
    assert.ok(toxin, "Toxin-A should remain carried after returning to Hydro 2");

    // Double-click through the production inventory. With no Research skill,
    // the MFD opens but explicitly refuses to begin.
    await useInventoryItem(game, toxin.entity_id);
    let panel = await researchPanel(game);
    assert.ok(
      panelTexts(panel).some((text) => text.includes("Research skill of 1")),
      `untrained panel should explain the requirement; got ${JSON.stringify(panelTexts(panel))}`,
    );

    // Fund and use Hydro 2's authored Tech Trainer, purchasing Research 0->1
    // for the retail WTECHCOST price of ten modules.
    await game.player.setStats({ cyber_modules: 10 });
    const techTrainer = (
      await game.entities.list({ filter: "Tech Trainer", limit: 20 })
    ).entities.find((entity) => entity.template_id === 966);
    assert.ok(techTrainer, "Hydro 2 should contain its authored Tech Trainer");
    const [tx, ty, tz] = (await game.entities.detail(techTrainer.id)).position;
    await teleportVerified(game, { x: tx, y: ty + 0.5, z: tz + 1.2 });
    await game.step({ frames: 20 });
    await game.entities.sendMessage(techTrainer.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const trainer = await game.ui.state();
    const researchRow = trainer.active_panel?.elements.find(
      (element) => element.kind === "button" && element.label === "Research",
    );
    assert.ok(researchRow, "Tech Trainer should expose the Research row");
    await clickUiElement(game, researchRow);
    await game.step({ frames: 3 });
    const trained = (await game.info()).player.stats!;
    assert.equal(trained.skills.research, 1, "real trainer purchase grants Research 1");
    assert.equal(trained.cyber_modules, 0, "Research 1 spends ten cyber modules");

    // Raise only for test-time acceleration after proving the real purchase;
    // the state-machine unit test covers the exact skill multiplier. At skill
    // 6, the retail formula advances 26 authored seconds per real second.
    await game.player.setStats({ skills: { research: 6 } });
    toxin = await carriedNamed(game, "Anti-Annelid Toxin");
    assert.ok(toxin);
    await useInventoryItem(game, toxin.entity_id);
    await game.step({ frames: 75 });
    panel = await researchPanel(game);
    assert.ok(
      panelTexts(panel).some((text) => text.includes("Antimony (Sb)")),
      `first gate should request antimony; got ${JSON.stringify(panelTexts(panel))}`,
    );

    // Carry the actual authored Hydro chemical objects. Wrong Vanadium is
    // refused and remains; Antimony is consumed, reaching the second gate.
    const antimonyWorld = (
      await game.entities.list({ filter: "Chem #4", limit: 30 })
    ).entities.slice(0, 2);
    const vanadiumWorld = (
      await game.entities.list({ filter: "Chem #2", limit: 30 })
    ).entities[0];
    assert.equal(antimonyWorld.length, 2, "Hydro 2 should author two Antimony doses");
    assert.ok(vanadiumWorld, "Hydro 2 should author a Vanadium dose");
    for (const chemical of [...antimonyWorld, vanadiumWorld]) {
      await game.player.give(chemical.id);
    }

    let vanadium = await carriedNamed(game, "Chem #2");
    assert.ok(vanadium);
    await useInventoryItem(game, vanadium.entity_id);
    assert.ok(await carriedNamed(game, "Chem #2"), "wrong chemical must remain carried");

    let antimony = await carriedNamed(game, "Chem #4");
    assert.ok(antimony);
    await useInventoryItem(game, antimony.entity_id);
    await game.step({ frames: 75 });
    panel = await researchPanel(game);
    assert.ok(
      panelTexts(panel).some((text) => text.includes("Vanadium (V)")),
      `second gate should request vanadium; got ${JSON.stringify(panelTexts(panel))}`,
    );

    // Save/load while paused at a chemical gate, then reopen the carried item:
    // active partial progress and the pending chemical must survive.
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    toxin = await carriedNamed(game, "Anti-Annelid Toxin");
    assert.ok(toxin, "Toxin-A survives save/load");
    await useInventoryItem(game, toxin.entity_id);
    panel = await researchPanel(game);
    assert.ok(
      panelTexts(panel).some((text) => text.includes("Vanadium (V)")),
      "the pending second chemical survives save/load",
    );

    vanadium = await carriedNamed(game, "Chem #2");
    assert.ok(vanadium);
    await useInventoryItem(game, vanadium.entity_id);
    await game.step({ frames: 430 });
    panel = await researchPanel(game);
    assert.ok(
      panelTexts(panel).some((text) => text.includes("Antimony (Sb)")),
      "the authored 240-second gate requests the second Antimony dose",
    );

    antimony = await carriedNamed(game, "Chem #4");
    assert.ok(antimony);
    await useInventoryItem(game, antimony.entity_id);
    await game.step({ frames: 850 });
    panel = await researchPanel(game);
    assert.ok(
      panelTexts(panel).some((text) => text.includes("Research complete")),
      `completion should be player-visible; got ${JSON.stringify(panelTexts(panel))}`,
    );
    assert.equal(
      await game.quests.get("Note_3_2"),
      "complete",
      "Toxin-A completion grants its authored research quest bit",
    );

    const report = panel.active_panel!.elements.find(
      (element) => element.label === "Research report",
    );
    assert.ok(report, "completed research unlocks report #5");
    await clickUiElement(game, report);
    await game.step({ frames: 3 });
    let reportPanel = await game.ui.state();
    assert.ok(
      panelTexts(reportPanel).some((text) => text.includes("Summary:")),
      "the report button reveals the authored analysis",
    );

    // The report is longer than the safe text region. Page through the retail
    // scroll gadget and prove the late recommendation is reachable rather
    // than silently truncated or drawn under the report button.
    for (let page = 0; page < 8; page += 1) {
      if (panelTexts(reportPanel).some((text) => text.includes("Recommendation:"))) {
        break;
      }
      const nextPage = reportPanel.active_panel?.elements.find(
        (element) => element.label === "Research report next page",
      );
      assert.ok(nextPage, "a long research report should expose next-page navigation");
      await clickUiElement(game, nextPage);
      await game.step({ frames: 3 });
      reportPanel = await game.ui.state();
    }
    assert.ok(
      panelTexts(reportPanel).some((text) => text.includes("Recommendation:")),
      `late report recommendation should be reachable; got ${JSON.stringify(panelTexts(reportPanel))}`,
    );

    // Both vials predated completion, so both must be normalized to researched
    // and accepted by Hydro's two independent, cross-deck regulators.
    await game.transitionLevel("hydro1.mis");
    await game.step({ frames: 5 });
    const liveAcr1 = (await game.entities.list({ filter: "ACR1", limit: 30 })).entities.find(
      (entity) => entity.name === "ACR1",
    );
    assert.ok(liveAcr1);
    let carriedToxins = await carriedItemsNamed(game, "Anti-Annelid Toxin");
    assert.equal(carriedToxins.length, 2, "both preexisting vials survive research");
    await game.entities.sendMessage(liveAcr1.id, { type: "Frob" });
    await game.step({ frames: 600 });
    carriedToxins = await carriedItemsNamed(game, "Anti-Annelid Toxin");
    assert.equal(
      carriedToxins.length,
      1,
      "ACR1 consumes one researched preexisting vial",
    );
    assert.equal(
      await game.quests.get("ACR1"),
      "incomplete",
      "the authored ACR1-Activate trap marks this regulator active",
    );

    // ACR2 is the second real regulator in Hydro 2, not another Hydro 1
    // object. Follow the authored cross-deck objective before using it.
    await game.transitionLevel("hydro2.mis");
    await game.step({ frames: 5 });
    const liveAcr2 = (await game.entities.list({ filter: "ACR2", limit: 30 })).entities.find(
      (entity) => entity.name === "ACR2",
    );
    assert.ok(liveAcr2, "Hydro 2 should contain the authored ACR2 regulator");
    await game.entities.sendMessage(liveAcr2.id, { type: "Frob" });
    await game.step({ frames: 600 });
    assert.equal(
      (await carriedItemsNamed(game, "Anti-Annelid Toxin")).length,
      0,
      "ACR2 consumes the other researched preexisting vial",
    );
    assert.equal(
      await game.quests.get("ACR2"),
      "incomplete",
      "the authored ACR2-Activate trap marks the second regulator active",
    );

    // After its authored model tweq reaches the used frame, another researched
    // vial is left untouched by an already-used regulator.
    const secondToxin = await game.player.spawnItem(-1341);
    await game.step({ frames: 5 });
    await game.entities.sendMessage(liveAcr2.id, { type: "Frob" });
    await game.step({ frames: 30 });
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === secondToxin.entity_id,
      ),
      "an already-used ACR must not consume another vial",
    );
  },
);
