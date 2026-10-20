import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer, type UiElement } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { winHack } from "./helpers/hack.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { savedGamePath } from "./helpers/save.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { aimMfdAt, drawPersonalCard } from "./helpers/vr-hand.js";

const skip = process.env.SHOCK2_E2E !== "1";
type Click = (game: GameServer, element: UiElement) => Promise<void>;
/** Aim the free right hand's ray at an element of the VR host's canvas. */
const rayClick: Click = async (g, element) =>
  clickCanvasWithRay(g, requirePanelPose(await g.ui.state()), canvasCenter(element));

const elements = async (game: GameServer) => (await game.ui.state()).active_panel!.elements;
async function button(game: GameServer, label: string) {
  const found = (await elements(game)).find(e => e.label === label);
  assert.ok(found, `missing ${label}`);
  return found;
}
const games = async (game: GameServer) =>
  (await elements(game)).filter(e => e.label?.startsWith("game-")).map(e => e.label);
const objectState = async (game: GameServer, id: number) =>
  (await game.entities.detail(id)).properties.find(p => p.name === "ObjectState")?.value;

async function fund(game: GameServer) {
  await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
  for (let i = 0; i < 25; i++) {
    const nanites = await game.player.spawnItem(-89);
    await game.entities.sendMessage(nanites.entity_id, { type: "Frob" });
    await game.step({ frames: 1 });
  }
  return carriedNaniteTotal(game);
}

/** Pay for and win the board from the plug, then check every game unlocked. */
async function hackFromPlug(game: GameServer, entity: number, click: Click) {
  const wallet = await fund(game);
  assert.ok(wallet >= 400);
  await click(game, await button(game, "hack-gamepig"));
  assert.ok((await elements(game)).some(e => e.texture === "hack.pcx"));
  assert.ok(!(await elements(game)).some(e => e.label === "hack-gamepig"), "the plug drops beside the board");
  assert.equal(await carriedNaniteTotal(game), wallet, "opening is free");
  await click(game, await button(game, "start-hack"));
  assert.equal(await carriedNaniteTotal(game), wallet - 50, "START charges the authored 50 nanites");
  assert.notEqual(await objectState(game, entity), "Hacked", "payment is not a win");
  await winHack(game, click);
  assert.equal(await objectState(game, entity), "Hacked");
  await click(game, await button(game, "gamepig-back"));
  assert.ok(!(await elements(game)).some(e => e.label === "hack-gamepig"), "a hacked device cannot charge again");
  for (const label of ["game-2", "game-3", "game-5", "game-8", "game-10"]) await button(game, label);
  assert.ok((await elements(game)).some(e => /overworld/i.test(e.text ?? "")), "unimplemented titles are also unlocked");
}

test("GamePig paid hack from the plug unlocks its library and survives saves (flat)", {
  skip, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  const { handheld } = await openGamePig(game, false, "Swinekeeper");
  const original = await games(game);
  assert.deepEqual(original, ["game-3"]);
  const plug = await button(game, "hack-gamepig");
  assert.ok(plug.texture === "plugh0.pcx" && plug.rect[0] >= 188, "the HRM plug sits beside the 188px body");
  assert.ok(!(await elements(game)).some(e => e.text === "HACK"), "no placeholder HACK row");
  await clickUiElement(game, plug);
  const empty = await carriedNaniteTotal(game);
  assert.ok(empty < 50);
  await clickUiElement(game, await button(game, "start-hack"));
  assert.equal(await carriedNaniteTotal(game), empty, "an unaffordable attempt charges nothing");
  assert.ok((await elements(game)).some(e => e.texture === "payh.pcx"));
  await clickUiElement(game, await button(game, "gamepig-back"));
  assert.deepEqual(await games(game), original, "canceling an unpaid board unlocks nothing");
  await hackFromPlug(game, handheld.entity_id, clickUiElement);

  const name = `gamepig_hack_${Date.now()}`;
  try {
    const bits = () => JSON.parse(readFileSync(savedGamePath(name), "utf8")).global_data.quest_info.mini_games;
    await game.save(name);
    assert.equal(bits(), 0x1ffc, "all eleven actual games are installed without the empty Slots entry");
    const balance = await carriedNaniteTotal(game);
    await game.load(name);
    await game.step({ frames: 2 });
    await game.save(name);
    assert.equal(bits(), 0x1ffc);
    assert.equal(await carriedNaniteTotal(game), balance);
    const [restored] = await game.entities.byTemplate(-3864);
    assert.ok(restored);
    assert.equal(await objectState(game, restored.id), "Hacked");
  } finally { rmSync(savedGamePath(name), { force: true }); }
});

test("VR cyber interface opens a backpack GamePig with the flat canvas and its plug", {
  skip, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const handheld = await game.player.spawnItem("Gameboy");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const ui = await game.ui.state();
  const slot = ui.strip!.elements.find(e => e.entity_id === handheld.entity_id);
  assert.ok(slot, "the GamePig is in the backpack");
  await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(slot));
  await game.step({ frames: 10 });
  const opened = await game.ui.state();
  assert.equal(opened.mode, "use");
  assert.equal(opened.active_panel?.entity_id, handheld.entity_id);
  assert.deepEqual(opened.panel_pose?.canvas, [640, 480], "the cyber interface's shared canvas");
  const body = (await elements(game)).find(e => e.texture === "gameback.pcx")!;
  const plug = await button(game, "hack-gamepig");
  assert.ok(plug.rect[0] >= body.rect[0] + 188, "the plug sits beside the body, as in flat");
  await hackFromPlug(game, handheld.entity_id, rayClick);
});

/** A tricorder scan is a query: its panel lives in the utilities, never the games index. */
async function utility(game: GameServer, label: string) {
  return (await game.ui.state()).utilities.find(e => e.label === label);
}

for (const held of [false, true]) {
  test(`VR tricorder scan of a ${held ? "held" : "world"} GamePig queries it, with a hack plug until hacked`, {
    skip, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
      debugFlags: ["--vr", "--experimental", "mfd_device"] });
    await game.devParams.set("vr_mfd_focus_scan", 0);
    await game.step({ frames: 30 });
    const [pig] = await game.entities.byTemplate(-3864);
    assert.ok(pig, "the rack carries a GamePig");
    await game.player.teleport({ x: pig.position[0], y: .9, z: 0 });
    await game.step({ frames: 120 });
    let expected = pig.id;
    if (held) {
      await game.input.set("right_hand.squeeze", 1);
      await game.player.spawnItem("Gameboy", { hand: "right" });
      await game.step({ frames: 3 });
      await game.input.set("right_hand.position", [.15, .9, -.45]);
      await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 5 });
      expected = (await game.info()).player.right_hand_entity_id!;
      assert.ok(expected);
    } else {
      await game.input.set("right_hand.position", [.6, .7, -.8]);
    }
    await drawPersonalCard(game, "left");
    const scan = async (standOff: number) => {
      const target = (await game.entities.detail(expected)).position;
      await aimMfdAt(game, target, standOff, 1, 0, { hand: "left", lookAtTarget: false });
      await game.step({ frames: 2 });
      await game.input.set("left_hand.trigger", 1);
      await game.step({ frames: 3 });
      await game.input.set("left_hand.trigger", 0);
      await game.input.set("left_hand.position", [-.25, 1, -.8]);
      await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 3 });
      assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.last_scan, expected);
      const ui = await game.ui.state();
      assert.equal(ui.active_panel, null, "a scan never opens the games panel");
      const title = ui.utilities.find(e => e.label === "query_title");
      assert.equal(title?.entity_id, expected, "the scan shows the GamePig's query panel");
      assert.ok(!ui.utilities.some(e => e.label?.startsWith("game-")));
    };
    await scan(held ? .25 : 1.1);
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === pig.id),
      "scanning never picks the device up");
    assert.ok(await utility(game, "hack-gamepig"), "the hack plug sits beside the query");
    if (held) {
      // The held hand has no free ray; release it so the pointer can click.
      await game.input.set("right_hand.squeeze", 0);
      await game.input.set("right_hand.position", [.6, .7, -.8]);
      await game.step({ frames: 3 });
    }

    const wallet = await fund(game);
    await rayClick(game, (await utility(game, "hack-gamepig"))!);
    assert.equal((await game.ui.state()).active_panel?.entity_id, expected, "the plug opens its board on the tricorder");
    assert.ok((await elements(game)).some(e => e.texture === "hack.pcx"));
    assert.equal(await carriedNaniteTotal(game), wallet, "opening is free");
    await winHack(game, rayClick);
    assert.equal(await objectState(game, expected), "Hacked");
    await rayClick(game, await button(game, "gamepig-back"));
    let ui = await game.ui.state();
    assert.equal(ui.active_panel, null, "BACK returns to the query, not the games");
    assert.equal(ui.utilities.find(e => e.label === "query_title")?.entity_id, expected);
    assert.ok(!(await utility(game, "hack-gamepig")), "a hacked device offers no plug");

    if (!held) {
      // The released held device lies at the player's feet; the rack one re-scans cleanly.
      await rayClick(game, (await utility(game, "utility_close"))!);
      await game.step({ frames: 60 });
      await scan(1.1);
      assert.ok(!(await utility(game, "hack-gamepig")), "a re-scan of the hacked device shows the query alone");
    }
    // Read the library through the device's own panel (a script frob, not a scan).
    await game.entities.sendMessage(expected, { type: "Frob" });
    await game.step({ frames: 5 });
    for (const label of ["game-2", "game-3", "game-5", "game-8", "game-10"]) await button(game, label);
  });
}

test("VR held GamePig screen lists games without a hack affordance", {
  skip, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await openGamePig(game, true, "Swinekeeper");
  assert.deepEqual(await games(game), ["game-3"]);
  assert.ok(!(await elements(game)).some(e => e.label === "hack-gamepig" || e.text === "HACK"),
    "the held screen windows only the body");
  // Swinekeeper is the only row: down wraps onto it rather than a hack row.
  await game.input.set("left_hand.thumbstick", [0, -1]);
  await game.step({ frames: 2 });
  await game.input.set("left_hand.thumbstick", [0, 0]);
  await game.input.trigger("LeftHandLowerButton");
  await game.step({ frames: 2 });
  assert.ok((await elements(game)).some(e => e.texture === "swinebck.pcx"), "primary starts the game");
  assert.ok(!(await elements(game)).some(e => e.texture === "hack.pcx"));
});
