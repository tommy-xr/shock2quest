// The PDA's Notes tab lists the deck's objective quest bits (`Note_<deck>_<n>`)
// as a read-only checklist: 1 an open box, 2 a checked one. Negative-first:
// before the tab existed there was no `pda_tab_notes` control.
import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { clickUiElement, clickCanvasWithRay, canvasCenter, requirePanelPose } from "./helpers/ui.js";

for (const vr of [false, true]) {
  test(`PDA Notes tab lists open and completed objectives per deck (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    await game.player.spawnItem(-52);
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const click = async (e: UiElement) => {
      if (vr) await clickCanvasWithRay(game, requirePanelPose(await game.ui.state()), canvasCenter(e));
      else await clickUiElement(game, e);
    };
    const strip = async () => (await game.ui.state()).strip!.elements;
    const control = async (label: string) => {
      const e = (await strip()).find((e) => e.kind === "button" && e.label === label);
      assert.ok(e, `${label} is on the PDA list`);
      return e;
    };
    const header = async () => (await strip()).find((e) => e.label === "pda_header")?.text;

    // The authored trap that hands out the Watts and Grassi objectives.
    const trap = (await game.entities.list({ filter: "EmailTrap", limit: 20 })).entities.find((e) => e.template_id === 513);
    assert.ok(trap, "medsci1 EmailTrap 513");
    await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    await game.step({ frames: 5 });
    assert.equal(await game.quests.get("note_2_6"), "incomplete");
    assert.equal(await game.quests.get("note_2_9"), "incomplete");
    await game.quests.set("Note_2_9", "complete");
    // A deck with nothing but a note still gets a deck-arrow stop.
    await game.quests.set("Note_5_1", "incomplete");

    const logs = (await game.ui.state()).readout.find((e) => e.label === "logs");
    assert.ok(logs, "LOGS readout button");
    await click(logs);
    await click(await control("pda_tab_notes"));
    for (let i = 0; i < 8 && (await header()) !== "Notes: Deck 2"; i++) await click(await control("pda_deck_next"));
    assert.equal(await header(), "Notes: Deck 2", "authored misc.str LevelBase2");

    const elements = await strip();
    assert.ok(elements.some((e) => e.texture === "iface/pnotes1.pcx"), "the active notes tab art");
    assert.equal(elements.filter((e) => e.texture === "iface/notebox.pcx").length, 2);
    assert.equal(elements.filter((e) => e.texture === "iface/notechek.pcx").length, 1);
    const rowText = (row: number) =>
      elements.filter((e) => e.label === `pda_note_text:${row}`).map((e) => e.text).join(" ");
    const grassi = [0, 1].find((row) => /Grassi/.test(rowText(row)));
    assert.notEqual(grassi, undefined, `a row names Grassi: ${rowText(0)} | ${rowText(1)}`);
    assert.ok(elements.some((e) => e.label === `pda_note_checked:${grassi}`), "the completed objective is checked");
    assert.match(rowText(1 - grassi!), /Watts/, "the open objective's authored notes.str text");
    assert.ok(!elements.some((e) => e.label?.startsWith("pda_entry:")), "notes are not clickable rows");

    for (let i = 0; i < 8 && (await header()) !== "Notes: Deck 5"; i++) await click(await control("pda_deck_next"));
    assert.equal(await header(), "Notes: Deck 5", "the note-only deck is reachable");
    assert.equal((await strip()).filter((e) => e.texture === "iface/notebox.pcx").length, 1);
  });
}
