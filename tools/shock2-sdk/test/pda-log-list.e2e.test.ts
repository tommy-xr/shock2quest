// Issue #2330: the LOGS MFD must reach every collected log and email, not just
// the newest one. Negative-first: before the PDA list, LOGS opened the reader
// straight on the newest log, so the older Amanpour log was unreachable and
// there was no return button.
import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { clickUiElement, clickCanvasWithRay, canvasCenter, requirePanelPose } from "./helpers/ui.js";

for (const vr of [false, true]) {
  test(`LOGS lists every collected log and email and the reader returns to it (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
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
      assert.ok(e, `${label} is on the LOGS list`);
      return e;
    };
    const logsButton = async () => {
      const e = (await game.ui.state()).readout.find((e) => e.label === "logs");
      assert.ok(e, "LOGS readout button");
      return e;
    };
    const reader = async () => {
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, "the reader is open");
      return panel;
    };

    // Collect Amanpour's log (1608) first, then any other medsci1 log.
    const discs = (await game.entities.list({ filter: "Audio Log", limit: 80 })).entities;
    const amanpour = discs.find((e) => e.template_id === 1608);
    assert.ok(amanpour);
    await game.entities.sendMessage(amanpour.id, { type: "Frob" });
    await game.step({ frames: 5 });
    for (const disc of discs.filter((e) => e.template_id !== 1608)) {
      await game.entities.sendMessage(disc.id, { type: "Frob" });
      await game.step({ frames: 5 });
      if ((await game.info()).player.collected_logs.length === 2) break;
    }
    const collected = (await game.info()).player.collected_logs;
    assert.equal(collected.length, 2, "two logs collected");
    assert.ok(collected.every((log) => log.deck === 2));

    await click(await logsButton());
    assert.equal((await game.ui.state()).mode, "use");
    assert.ok((await strip()).some((e) => e.label === "pda_header" && e.text?.includes("2")), "deck 2 header");
    await click(await control("pda_entry:0"));
    assert.ok((await reader()).elements.some((e) => e.text?.includes("45100")), "the older log opens from the list");

    const back = (await reader()).elements.find((e) => e.kind === "button" && e.label === "Return to list");
    assert.ok(back, "the reader has a return button");
    await click(back);
    assert.equal((await game.ui.state()).active_panel, null, "return closes the reader");
    await control("pda_entry:1");

    // Emails list on their own tab and open in the email frame.
    const trap = (await game.entities.list({ filter: "EmailTrap", limit: 20 })).entities.find((e) => e.template_id === 1131);
    assert.ok(trap, "medsci1 EmailTrap 1131 (EM0201)");
    await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    await game.step({ frames: 5 });
    await click(await control("pda_tab_email"));
    await click(await control("pda_entry:0"));
    assert.ok((await reader()).elements.some((e) => e.texture === "iface/email.pcx"), "email reader frame");
    const plays = (await game.audio.recent()).sounds.filter((s) => s.sample.toLowerCase() === "em0201");
    assert.equal(plays.length, 2, "reading the email replays its audio");
    const backToEmails = (await reader()).elements.find((e) => e.label === "Return to list");
    assert.ok(backToEmails);
    await click(backToEmails);
    assert.ok((await strip()).some((e) => e.texture === "iface/pemail1.pcx"), "returns to the email tab");

    await click(await logsButton());
    assert.ok(!(await strip()).some((e) => e.label?.startsWith("pda_")), "LOGS toggles the list closed");
    assert.equal((await game.ui.state()).mode, "use");
  });
}
