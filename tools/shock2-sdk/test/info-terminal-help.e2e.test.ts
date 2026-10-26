// Using a medsci1 info terminal unlocks the help topic in its `P$HelpText`
// mask and opens it in the help reader, whose return lands on the PDA Help
// tab, in a fresh mission and in a release save written before that property
// was parsed; the VR tricorder does the same by scanning. Negative-first: with
// InfoComputer unregistered no topic unlocks, without the save backfill the
// release-save terminal opens the map, and before the Help tab the frob only
// unlocked (no reader) and the tricorder queried the terminal.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { gunzipSync } from "node:zlib";
import { GameServer, findRepoRoot } from "../src/index.js";
import type { UiElement, UiState } from "../src/types.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { aimMfdAt, aimVrHandAt, aimVrHandAtCanvas, drawPersonalCard } from "./helpers/vr-hand.js";

const cases = [
  { name: "flat", vr: false },
  { name: "VR", vr: true },
  { name: "v0.0.3 save", vr: false, save: "v0.0.3/medsci1-kit" },
];

for (const { name, vr, save } of cases) {
  test(`info terminal unlocks its help topic (${name})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async (t) => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    if (save) {
      const assetRoot = process.env.DARK_ASSET_PATH;
      assert.ok(assetRoot, "DARK_ASSET_PATH selects the save directory");
      mkdirSync(join(assetRoot, "saves"), { recursive: true });
      const saveName = `info_terminal_help_${Date.now()}`;
      const savePath = join(assetRoot, "saves", `${saveName}.sav`);
      const fixture = join(findRepoRoot(process.cwd())!, "shock2vr", "tests", "fixtures", "saves", `${save}.sav.gz`);
      writeFileSync(savePath, gunzipSync(readFileSync(fixture)));
      t.after(() => rmSync(savePath, { force: true }));
      const loaded = await game.load(saveName);
      assert.ok(loaded.success, `load failed: ${loaded.message}`);
    }
    await game.step({ frames: 10 });
    // Mission object 178 carries P$HelpText = 1: topic 0.
    const { entities } = await game.entities.list({ filter: "Info Comp", limit: 50 });
    assert.equal(entities.length, 17, "medsci1's info terminals");
    const terminal = entities.find((e) => e.template_id === 178);
    assert.ok(terminal, "info terminal 178");
    assert.deepEqual((await game.quests.list()).help_topics, []);

    const [x, , z] = terminal.position;
    await game.player.teleport({ x: x + 1.5, y: 0.9, z });
    await game.step({ frames: 10 });
    const aim = await game.player.aimAt(terminal.id, { visibility: "required" });
    assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
    if (vr) await aimVrHandAt(game, aim.world_point, 0.35);
    const use = vr ? "right_hand.trigger" : "right_hand.squeeze";
    await game.input.set(use, 1);
    await game.step({ frames: 2 });
    await game.input.set(use, 0);
    await game.step({ frames: 4 });

    assert.deepEqual((await game.quests.list()).help_topics, [0]);
    assertHelpReader(await game.ui.state());

    // Return lands on the PDA Help tab with the topic listed.
    const back = (await game.ui.state()).active_panel!.elements.find((e) => e.label === "Return to list");
    assert.ok(back, "the help reader has a return button");
    if (vr) await clickCanvasWithRay(game, requirePanelPose(await game.ui.state()), canvasCenter(back));
    else await clickUiElement(game, back);
    const ui = await game.ui.state();
    assert.equal(ui.active_panel, null, "return closes the reader");
    assertHelpList(ui.strip!.elements);

    // Re-using an unlocked terminal shows its topic again.
    await game.entities.sendMessage(terminal.id, { type: "Frob" });
    await game.step({ frames: 4 });
    assertHelpReader(await game.ui.state());
    assert.deepEqual((await game.quests.list()).help_topics, [0]);
  });
}

// Topic 0's name; the SCP mod's string tables hyphenate it.
const CYBER_INTERFACE = /^The Cyber.Interface$/;

function assertHelpReader(ui: UiState) {
  const reader = ui.active_panel;
  assert.ok(reader, "a reader is open");
  assert.ok(reader.elements.some((e) => e.texture === "iface/help.pcx"), "the help reader opens");
  assert.ok(reader.elements.some((e) => CYBER_INTERFACE.test(e.text ?? "")), "infocomp.str HelpName1 header");
  assert.ok(reader.elements.some((e) => e.text?.includes("cyber")), "HelpText1 body");
}

function assertHelpList(elements: UiElement[]) {
  assert.ok(elements.some((e) => e.texture === "iface/pvideo1.pcx"), "the Help tab is active");
  assert.ok(elements.some((e) => e.label === "pda_header" && /^Help (Text|Topics)$/.test(e.text ?? "")),
    "misc.str LevelBase3");
  const row = elements.find((e) => e.label === "pda_entry:0");
  assert.match(row?.text ?? "", CYBER_INTERFACE);
}

test("tricorder scan of an info terminal opens its help on the device", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", port: 0, debugFlags: ["--vr"] });
  await game.devParams.set("vr_mfd_focus_scan", 0);
  await game.step({ frames: 10 });
  const { entities } = await game.entities.list({ filter: "Info Comp", limit: 50 });
  const terminal = entities.find((e) => e.template_id === 178);
  assert.ok(terminal, "info terminal 178");
  const [x, , z] = terminal.position;
  await game.player.teleport({ x: x + 1.5, y: 0.9, z });
  await game.step({ frames: 10 });
  const aim = await game.player.aimAt(terminal.id, { visibility: "required" });
  assert.ok(aim.target_confirmed, JSON.stringify(aim));
  await game.input.set("right_hand.position", [.6, .7, -.8]);
  await drawPersonalCard(game, "left");
  const card = async () => (await game.info()).player.hand_feedback!.body_gear!.personal_card;
  const scans = (await card()).scans;
  const since = (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
  await aimMfdAt(game, aim.world_point, .55, 1, 0, { hand: "left" });
  await game.step({ frames: 2 });
  await game.input.set("left_hand.trigger", 1);
  await game.step({ frames: 6 });
  await game.input.set("left_hand.trigger", 0);
  await game.step({ frames: 3 });

  assert.equal((await card()).last_scan, terminal.id);
  assert.equal((await card()).scans, scans + 1, "one scan");
  const frobs = (await game.messages.recent()).messages.filter((m) =>
    m.sequence > since && m.payload === "Frob" && m.to.entity_id === terminal.id);
  assert.equal(frobs.length, 1, "the scan uses the terminal once");
  assert.deepEqual((await game.quests.list()).help_topics, [0]);
  const ui = await game.ui.state();
  assert.equal(ui.mode, "device", "on the tricorder, not the cyber interface");
  assertHelpReader(ui);

  // Read at reading height; the free hand returns to the device's Help tab.
  await game.input.set("left_hand.position", [-.2, .3, -.6]);
  await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 4 });
  const panel = (await game.ui.state()).panel_pose!;
  assert.deepEqual(panel.canvas, [268, 376], "the tricorder screen");
  const back = (await game.ui.state()).active_panel!.elements.find((e) => e.label === "Return to list")!;
  const [bx, by, bw, bh] = back.rect;
  for (const trigger of [0, 1, 0]) {
    await aimVrHandAtCanvas(game, panel, [bx + bw / 2, by + bh / 2], { hand: "right", trigger });
    await game.step({ frames: 3 });
  }
  const after = await game.ui.state();
  assert.equal(after.active_panel, null, "return closes the reader");
  assertHelpList(after.utilities);

  // Focus scanning (the default) must not rescan the terminal over the list.
  await game.devParams.set("vr_mfd_focus_scan", 1);
  await game.step({ frames: 60 });
  const settled = await game.ui.state();
  assert.equal(settled.active_panel, null, "the list stays up under the device's gaze");
  assertHelpList(settled.utilities);
});
