import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas, drawPersonalCard } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const hand of ["left", "right"] as const) {
  test(`MFD device ${hand}: explicit scan, free-hand utilities, return and fresh draw`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", port: 0,
      debugFlags: hand === "left" ? ["--vr", "--experimental", "mfd_device"] : ["--vr"] });
    if (hand === "right") await game.devParams.set("vr_mfd_device", 1);
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

test("MFD device scans a needed world chemical once and preserves an unneeded chemical", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", port: 0,
    debugFlags: ["--vr", "--experimental", "mfd_device"] });
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
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 10 });
    await drawPersonalCard(game, "left");
    const target = await game.entities.detail(chemical.entity_id);
    await aimVrHandAt(game, target.position, .25, 1, 0, { hand: "left" });
    await game.step({ frames: 3 });
    await game.input.set("left_hand.trigger", 1);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.last_scan, chemical.entity_id,
      "the real scanner must identify the chemical being tested");
    const remaining = (await game.entities.list({ limit: 1000 })).entities.some(e => e.id === chemical.entity_id);
    assert.equal(remaining, !consumed, `${name}: only the requested chemical may be consumed`);
    await game.step({ frames: 5 });
    assert.equal((await game.entities.list({ limit: 1000 })).entities.some(e => e.id === chemical.entity_id), !consumed);
    await game.input.set("left_hand.trigger", 0);
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 3 });
  }
});
