import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas, drawPersonalCard, equipRightHand } from "./helpers/vr-hand.js";

import { ammoOf, fireOnce } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const hand of ["left", "right"] as const) {
  test(`MFD device ${hand}: explicit scan, free-hand utilities, return and fresh draw`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", port: 0,
      debugFlags: hand === "left" ? ["--vr", "--experimental", "mfd_device"] : ["--vr"] });
    if (hand === "right") await game.devParams.set("vr_mfd_device", 1);
    await game.devParams.set("vr_mfd_focus_scan", 0);
    await game.step({ frames: 30 });
    const [reader] = await game.entities.byTemplate(262);
    assert.ok(reader);
    const [x, , z] = reader.position;
    await game.player.teleport({ x: x - 1.59, y: 21.404, z: z - 2.23 });
    await game.step({ frames: 120 });
    const aim = await game.player.aimAt(reader.id, { hitbox: "center", visibility: "required" });
    assert.ok(aim.target_confirmed);
    await drawPersonalCard(game, hand);
    await aimVrHandAt(game, aim.world_point, .55, 1, 0, { hand });
    await game.step({ frames: 12 });
    assert.equal((await game.ui.state()).active_panel, null, "aiming cannot open a shop");
    assert.equal((await game.scene.fromSource("mfd_hologram")).length, 0);
    const before = (await game.info()).player.stats?.nanites;
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 12 });
    assert.equal((await game.ui.state()).active_panel?.template_id, 262);
    const miniature = await game.scene.fromSource("mfd_hologram");
    assert.ok(miniature.length > 0, "scanning creates a mesh preview");
    assert.ok(miniature.every(o => (Math.abs(o.transparency! - .45) < .001 || Math.abs(o.transparency! - .80) < .001) && !o.depth_write));
    const count = (await game.info()).player.hand_feedback!.body_gear!.personal_card.scans;

  await game.step({ frames: 30 });
    assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.scans, count);
    assert.equal((await game.info()).player.stats?.nanites, before, "scan cannot purchase");
    await game.input.set(`${hand}_hand.trigger`, 0);
    // Bring the instrument to reading height, then use the other hand's ray.
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.2 : .2, .3, -.6]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 4 });
    const panel = (await game.ui.state()).panel_pose!;
    assert.deepEqual(panel.canvas, [268, 376]);
    const free = hand === "left" ? "right" : "left";
    await aimVrHandAtCanvas(game, panel, [172, 341], { hand: free });
    await game.step({ frames: 3 });
    await game.input.set(`${free}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    await game.input.set(`${free}_hand.trigger`, 0);
    await game.step({ frames: 3 });
    assert.ok((await game.ui.state()).utilities.some(e => e.label === "access_cards"));
    assert.equal((await game.ui.state()).active_panel, null);
    // The newly mapped MAP control opens the whole wide canvas on the device.
    await aimVrHandAtCanvas(game, (await game.ui.state()).panel_pose!, [66, 350], { hand: free });
    await game.step({ frames: 2 });
    await game.input.set(`${free}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    await game.input.set(`${free}_hand.trigger`, 0);
    await game.step({ frames: 2 });
    const map = (await game.ui.state()).active_panel;
    assert.ok(map?.elements.some(e => e.texture?.toLowerCase().endsWith("mapback.pcx")), "MAP opens from the handheld host");
    const mapFrame = map!.elements.find(e => e.texture?.toLowerCase().endsWith("mapback.pcx"))!;
    assert.ok(mapFrame.rect[0] >= 8 && mapFrame.rect[0] + mapFrame.rect[2] <= 260.1, "the complete map fits the main screen");
    await aimVrHandAtCanvas(game, (await game.ui.state()).panel_pose!, [136, 341], { hand: free });
    await game.step({ frames: 2 });
    await game.input.set(`${free}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    await game.input.set(`${free}_hand.trigger`, 0);
    await game.step({ frames: 2 });
    assert.equal((await game.ui.state()).active_panel, null, "MFD replaces the compact map");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 3 });
    assert.equal((await game.ui.state()).panel_pose, null);
    assert.equal((await game.scene.fromSource("mfd_hologram")).length, 0);
    assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.hand, null);
    await drawPersonalCard(game, hand);
    assert.equal((await game.ui.state()).active_panel, null);
    if (hand === "right") {
      await game.devParams.set("vr_mfd_device", 0);
      await game.step({ frames: 3 });
      assert.equal((await game.ui.state()).panel_pose, null, "live toggle releases device UI ownership");
    }
  });
}

for (const [held, focus] of [[false, false], [true, false], [true, true]]) {
  test(`MFD device scans a needed ${held ? "held" : "world"} chemical ${focus ? "on focus" : "on trigger"} once and preserves an unneeded chemical`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
      debugFlags: ["--vr", "--experimental", "mfd_device"] });
    await game.devParams.set("vr_mfd_focus_scan", focus ? 1 : 0);
    await game.step({ frames: 30 });
    await game.player.setStats({ skills: { research: 6 } });
    const specimen = await game.player.spawnItem(-1341);
    // Provision an active project; the behavior under test is the real scanner
    // accepting/refusing physical chemicals, not the inventory's research shortcut.
    await game.entities.sendMessage(specimen.entity_id, { type: "Frob" });
    await game.step({ frames: 75 });
    for (const [name, consumed] of [["Chem #2", false], ["Chem #4", true]] as const) {
      const chemical = await game.player.spawnItem(name);
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      let ui = await game.ui.state();
      const slot = ui.strip!.elements.find(e => e.kind === "button" && e.entity_id === chemical.entity_id)!;
      assert.ok(slot, "provisioned chemical must be on the shared inventory strip");
      await aimVrHandAtCanvas(game, ui.panel_pose!, [slot.rect[0] + slot.rect[2] / 2, slot.rect[1] + slot.rect[3] / 2], { hand: "right" });
      await game.step({ frames: 2 });
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 3 });
      assert.equal((await game.info()).player.right_hand_entity_id, chemical.entity_id);
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 3 });
      await game.input.set("right_hand.position", [.2, .5, -.7]);
      await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 3 });
      if (!held) await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 10 });
      await drawPersonalCard(game, "left");
      const target = await game.entities.detail(chemical.entity_id);
      await aimVrHandAt(game, target.position, .25, 1, 0, { hand: "left" });
      await game.step({ frames: 3 });
      if (!focus) await game.input.set("left_hand.trigger", 1);
      await game.step({ frames: focus ? 35 : 5 });
      assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.last_scan, chemical.entity_id,
        "the real scanner must identify the chemical being tested");
      const remaining = (await game.entities.list({ limit: 1000 })).entities.some(e => e.id === chemical.entity_id);
      assert.equal(remaining, !consumed, `${name}: only the requested chemical may be consumed`);
      await game.step({ frames: 5 });
      assert.equal((await game.entities.list({ limit: 1000 })).entities.some(e => e.id === chemical.entity_id), !consumed);
      await game.input.set("left_hand.trigger", 0);
      await game.input.set("left_hand.squeeze", 0);
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 3 });
    }
  });
}

test("scanned world weapons offer state-specific repair and modify boards", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
    debugFlags: ["--vr", "--experimental", "mfd_device"] });
  await game.devParams.set("vr_mfd_focus_scan", 0);
  await game.step({ frames: 30 });
  const [gun] = await game.entities.byTemplate(-17);
  assert.ok(gun);
  await game.player.teleport({ x: gun.position[0], y: .9, z: 0 });
  await game.step({ frames: 120 });
  const aim = await game.player.aimAt(gun.id, { hitbox: "center", visibility: "required" });
  assert.ok(aim.target_confirmed);
  await game.input.set("right_hand.position", [.6, .7, -.8]);
  await drawPersonalCard(game, "left");
  async function scan() {
    await aimVrHandAt(game, aim.world_point, 1.1, 1, 0, { hand: "left", lookAtTarget: false });
    await game.step({ frames: 2 });
    await game.input.set("left_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("left_hand.trigger", 0);
    await game.input.set("left_hand.position", [-.25, 1, -.8]);
    await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 3 });
  }
  async function click(label: string) {
    const ui = await game.ui.state();
    const button = ui.active_panel?.elements.find(e => e.label === label);
    assert.ok(button, `missing ${label}`);
    const [x, y, w, h] = button.rect;
    await aimVrHandAtCanvas(game, ui.panel_pose!, [x + w / 2, y + h / 2]);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
  }
  const art = async (texture: string) => (await game.ui.state()).active_panel?.elements.some(e => e.texture === texture);
  await scan();
  await game.player.setStats({ skills: { modify: 6, repair: 6 }, cyber_affinity: 6 });
  await click("modify");
  assert.ok(await art("modify.pcx"), "the scanned world gun opens its Modify board");
  await game.entities.sendMessage(gun.id, { type: "SetObjectState", state: "Broken" });
  await game.entities.sendMessage(gun.id, { type: "SetGunCondition", condition: 30 });
  await game.step({ frames: 2 });
  await scan();
  assert.ok(!(await game.ui.state()).active_panel?.elements.some(e => e.label === "modify"));
  await click("repair");
  assert.ok(await art("iface/repair.pcx"), "the same broken gun opens its Repair board");
});


test("MFD screen ignores a gun hand while its trigger and face button still work", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
    debugFlags: ["--vr", "--experimental", "mfd_device"] });
  await game.devParams.set("vr_mfd_focus_scan", 0);
  await game.step({ frames: 30 });
  const [gun] = await game.entities.byTemplate(-17);
  assert.ok(gun);
  await game.player.teleport({ x: gun.position[0], y: .9, z: 0 });
  await game.step({ frames: 120 });
  await equipRightHand(game, true, gun.id, "EquipPistol");
  await drawPersonalCard(game, "left");
  await game.input.set("left_hand.position", [-.25, .4, -.7]);
  await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 3 });
  const ui = await game.ui.state();
  assert.ok(ui.panel_pose);
  // Aim the gun directly at MFD: it must not act as a UI pointer or click it.
  await aimVrHandAtCanvas(game, ui.panel_pose, [136, 341], { hand: "right", squeeze: 1 });
  await game.step({ frames: 3 });
  const ammo = ammoOf(await game.entities.detail(gun.id));
  assert.ok(ammo > 0);
  await fireOnce(game);
  assert.equal(ammoOf(await game.entities.detail(gun.id)), ammo - 1);
  assert.ok(!(await game.ui.state()).utilities.some(e => e.label === "character_close"));
  assert.equal((await game.ui.state()).pointer?.canvas ?? null, null);
  const mode = (await game.info()).player.wielded_gun_setting;
  await game.input.trigger("RightHandUpperButton");
  await game.step({ frames: 5 });
  assert.notEqual((await game.info()).player.wielded_gun_setting, mode);
  assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
  await aimVrHandAt(game, (await game.entities.detail(gun.id)).position, .3, 1, 0, { hand: "left", lookAtTarget: false });
  await game.input.set("left_hand.trigger", 1);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.last_scan, gun.id,
    "the held gun can be scanned without becoming a screen pointer");
});


test("drawing the device replaces a world quad and hand frobs stay on its screen", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0,
    debugFlags: ["--vr", "--experimental", "mfd_device"] });
  await game.devParams.set("vr_mfd_focus_scan", 0);
  await game.step({ frames: 30 });
  const [crate] = await game.entities.byTemplate(307);
  assert.ok(crate);
  await game.player.teleport({ x: crate.position[0] - 1.2, y: 21.404, z: crate.position[2] });
  await game.step({ frames: 60 });
  const aim = await game.player.aimAt(crate.id, { hitbox: "center", visibility: "required" });
  assert.ok(aim.target_confirmed);
  const uiBodies = async () => (await game.physics.bodies()).bodies.filter(b => b.collision_groups.includes("ui")).length;
  async function frob() {
    await aimVrHandAt(game, aim.world_point, .3, 0, 0, { hand: "right" });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 8 });
  }
  await frob();
  assert.ok(await uiBodies() > 0, "ordinary hand frob opens a world quad");
  await drawPersonalCard(game, "left");
  await game.input.set("left_hand.position", [-.4, .1, -.4]);
  await game.step({ frames: 3 });
  assert.equal(await uiBodies(), 0, "drawing removes the existing quad");
  await frob();
  assert.equal((await game.ui.state()).active_panel?.entity_id, crate.id);
  assert.equal(await uiBodies(), 0, "hand frob routes to the device with no duplicate quad");
});

test("a trigger held before drawing cannot click the device until released", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
    debugFlags: ["--vr", "--experimental", "mfd_device"] });
  await game.devParams.set("vr_mfd_focus_scan", 0);
  await game.step({ frames: 30 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await drawPersonalCard(game, "left");
  await game.input.set("left_hand.position", [-.25, .4, -.7]);
  await game.step({ frames: 3 });
  const ui = await game.ui.state();
  await aimVrHandAtCanvas(game, ui.panel_pose!, [136, 341], { hand: "right", trigger: 1 });
  await game.step({ frames: 5 });
  assert.ok((await game.ui.state()).utilities.every(e => e.rect[1] >= 300), "held trigger cannot open the character page");
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 3 });
  assert.ok((await game.ui.state()).utilities.some(e => e.rect[1] < 300), "fresh press opens the page");
});

for (const hand of ["left", "right"] as const) {
  test(`landscape map ${hand}: sideways and wide retain working close and footer controls`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", port: 0,
      debugFlags: ["--vr", "--experimental", "mfd_device"] });
    await game.devParams.set("vr_mfd_focus_scan", 0);
    await game.step({ frames: 30 });
    await drawPersonalCard(game, hand);
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.25 : .25, .4, -.7]);
    await game.step({ frames: 3 });
    const free = hand === "left" ? "right" : "left";
    async function aim(label: string, pressed = 0) {
      const ui = await game.ui.state();
      const target = [...ui.utilities, ...(ui.active_panel?.elements ?? [])].find(e => e.label === label);
      assert.ok(target, `missing ${label}`);
      const [x,y,w,h] = target.rect;
      await aimVrHandAtCanvas(game, ui.panel_pose!, [x+w/2,y+h/2], { hand: free, trigger: pressed });
      await game.step({ frames: 2 });
    }
    async function click(label: string) {
      await aim(label);
      await game.input.set(`${free}_hand.trigger`, 1);
      await game.step({ frames: 3 });
      await game.input.set(`${free}_hand.trigger`, 0);
      await game.step({ frames: 3 });
    }
    await click("map");
    let ui = await game.ui.state();
    let frame = ui.active_panel!.elements.find(e => e.texture?.toLowerCase().endsWith("mapback.pcx"))!;
    assert.ok(frame.rect[3] > frame.rect[2], "default map is rotated into portrait screen bounds");
    // Hold on the bezel while changing geometry, then move to the new close
    // button. A parameter change must not manufacture a fresh UI press.
    await aimVrHandAtCanvas(game, ui.panel_pose!, [2,2], { hand: free, trigger: 1 });
    await game.step({ frames: 2 });
    await game.devParams.set("vr_mfd_map_wide", 1);
    await game.step({ frames: 3 });
    await aim("close", 1);
    assert.ok((await game.ui.state()).active_panel, "held trigger cannot click after mode change");
    await game.input.set(`${free}_hand.trigger`, 0);
    await game.step({ frames: 2 });
    ui = await game.ui.state();
    frame = ui.active_panel!.elements.find(e => e.texture?.toLowerCase().endsWith("mapback.pcx"))!;
    assert.ok(frame.rect[2] > frame.rect[3], "wide mode presents an upright landscape panel");
    assert.ok(ui.utilities.find(e => e.label === "map")!.rect[1] > frame.rect[1] + frame.rect[3]);
    await click("close");
    assert.equal((await game.ui.state()).active_panel, null);
    await click("map");
    await click("character_stats");
    assert.equal((await game.ui.state()).active_panel, null, "footer still replaces the wide map");
    assert.deepEqual((await game.ui.state()).panel_pose!.canvas, [268,376]);
    await game.devParams.set("vr_mfd_map_wide", 0);
    await game.step({ frames: 2 });
    await click("map");
    await click("close");
    assert.equal((await game.ui.state()).active_panel, null, "rotated close button maps back to the host");
  });
}

test("focus scan opens a held weapon without trigger and does not repeat", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
    debugFlags: ["--vr", "--experimental", "mfd_device"] });
  await game.step({ frames: 30 });
  const [gun] = await game.entities.byTemplate(-17);
  await game.player.teleport({ x: gun.position[0], y: .9, z: 0 });
  await game.step({ frames: 120 });
  await equipRightHand(game, true, gun.id, "EquipPistol");
  await drawPersonalCard(game, "left");
  await aimVrHandAt(game, (await game.entities.detail(gun.id)).position, .3, 1, 0,
    { hand: "left", lookAtTarget: false });
  await game.step({ frames: 12 });
  const card = async () => (await game.info()).player.hand_feedback!.body_gear!.personal_card;
  assert.notEqual((await card()).last_scan, gun.id, "a glance does not commit");
  await game.step({ frames: 25 });
  assert.equal((await card()).last_scan, gun.id);
  const scans = (await card()).scans;
  await game.step({ frames: 90 });
  assert.equal((await card()).scans, scans, "steady focus scans only once");
  assert.ok((await game.ui.state()).active_panel, "focus opens the weapon panel");
});
