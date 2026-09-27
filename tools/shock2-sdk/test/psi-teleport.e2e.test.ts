import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { fireOnce } from "./helpers/weapon.js";
const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) {
  test(`Quantum Relocation marks current position and recalls (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      if (vr) {
        const amp = (await game.entities.list({ filter: "Psi Amp" })).entities[0]!;
        await aimVrHandAt(game, amp.position, 0.35);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 5 });
        assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
      }
      await game.input.set("head.look", [0, 0]);
      await selectPsiPower(game, "Teleport");
      const before = (await game.info()).player;
      await fireOnce(game);
      const marked = (await game.info()).player;
      assert.equal(marked.psi_points, before.psi_points! - 5, "marking costs five psi");
      const markers = (await game.entities.list({ filter: "Teleport Marker" })).entities;
      assert.equal(markers.length, 1, "first activation creates the authored marker");
      await game.input.set("right_hand.thumbstick", [0, -1]);
      await game.step({ frames: 60 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      const moved = (await game.info()).player.position;
      assert.ok(Math.hypot(...moved.map((v, i) => v - before.position[i]!)) > 2, `movement ${before.position} -> ${moved}`);
      // Switching powers does not erase a recall destination.
      await selectPsiPower(game, "Cryokinesis");
      await selectPsiPower(game, "Teleport");
      await fireOnce(game);
      const returned = (await game.info()).player.position;
      assert.ok(Math.hypot(...returned.map((v, i) => v - before.position[i]!)) < 0.1, `returned ${returned} instead of ${before.position}`);
      assert.equal((await game.info()).player.psi_points, before.psi_points! - 10, "recall costs five psi");
      assert.equal((await game.entities.list({ filter: "Teleport Marker" })).entities.length, 0, "recall consumes the marker");
      await fireOnce(game);
      const beforeClear = (await game.info()).player;
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 2 });
      await game.input.trigger("SelectPsiPower");
      await game.step({ frames: 8 });
      const ui = await game.ui.state();
      const clear = ui.active_panel?.elements.find(e => e.label === "CLEAR MARKER");
      assert.ok(clear, "the shared psi MFD offers explicit clear");
      if (vr) await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(clear), "left");
      else await clickUiElement(game, clear);
      assert.equal((await game.entities.list({ filter: "Teleport Marker" })).entities.length, 0);
      const afterClear = (await game.info()).player;
      assert.equal(afterClear.psi_points, beforeClear.psi_points, "clearing is free");
      assert.deepEqual(afterClear.position, beforeClear.position, "clearing never relocates the player");
      assert.ok(!(await game.ui.state()).active_panel?.elements.some(e => e.label === "CLEAR MARKER"), "clear is unavailable without a marker");
    });
}

test("Quantum Relocation survives same-level saves but not a level departure", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  await selectPsiPower(game, "Teleport");
  await fireOnce(game);
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 30 });
  assert.equal((await game.entities.byTemplate(-1109)).length, 0);
  await game.player.setStats({ psi_tier: 5, cyber_modules: 100 });
  const trainer = (await game.entities.list({ limit: 5000 })).entities.find(e => e.template_id === 1355)!;
  assert.ok(trainer);
  const [x, y, z] = trainer.position;
  await game.player.teleport({ x: x + 1, y: y + 0.8, z });
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 2 });
  await game.entities.sendMessage(trainer.id, { type: "Frob" });
  await game.step({ frames: 1 });
  async function click(label: string) {
    const element = (await game.ui.state()).active_panel?.elements.find(e => e.label === label);
    assert.ok(element, label);
    await clickUiElement(game, element);
  }
  await click("psi_tier_5");
  await click("psi_power_35");
  await game.input.set("pointer.position", [0.5, 0.5]);
  await game.step({ frames: 5 });
  assert.ok((await game.info()).player.stats!.cyber_modules < 100, "real trainer purchase spends modules");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 3 });
  await game.input.trigger("EquipPsiAmp");
  await game.step({ frames: 10 });
  await selectPsiPower(game, "Teleport");
  // Walk clear of the trainer's wall recess before storing a destination.
  // The debug placement used to open its MFD is not a proven standing pose.
  async function walkBack(frames: number) {
    await game.input.set("head.look", [0, 0]);
    await game.input.set("right_hand.thumbstick", [0, -1]);
    await game.step({ frames });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });
  }
  const byTrainer = (await game.info()).player.position;
  await walkBack(60);
  const clearFloor = (await game.info()).player.position;
  assert.ok(Math.hypot(...clearFloor.map((v, i) => v - byTrainer[i]!)) > 1, "walk onto clear floor");
  await fireOnce(game);
  await walkBack(30);
  await fireOnce(game);
  assert.equal((await game.entities.byTemplate(-1109)).length, 0, "this destination permits recall before saving");
  await fireOnce(game);
  const [marker] = await game.entities.byTemplate(-1109);
  assert.ok(marker);
  const particles = (await game.entities.list({ filter: "TeleportPt" })).entities;
  assert.equal(particles.length, 5, "all five authored marker particle attachments spawn");
  await game.input.trigger("TogglePauseMenu");
  await game.step({ frames: 10 });
  await game.input.trigger("TogglePauseMenu");
  await game.step({ frames: 10 });
  assert.equal((await game.entities.byTemplate(-1109)).length, 1, "pause/resume retains marker");
  const save = `e2e_psi_teleport_${Date.now()}`;
  await game.save(save);
  await game.input.trigger("ClearPsiTeleport");
  await game.step({ frames: 2 });
  assert.equal((await game.entities.byTemplate(-1109)).length, 0);
  assert.equal((await game.entities.list({ filter: "TeleportPt" })).entities.length, 0, "clear removes attachments");
  await game.load(save);
  await game.step({ frames: 10 });
  const [restored] = await game.entities.byTemplate(-1109);
  assert.ok(restored, "the same build restores its marker");
  assert.deepEqual(restored.position, marker.position);
  assert.equal((await game.entities.list({ filter: "TeleportPt" })).entities.length, 5, "save/load preserves exactly one set of particles");
  await game.load(save);
  await game.step({ frames: 10 });
  assert.equal((await game.entities.list({ filter: "TeleportPt" })).entities.length, 5, "repeated load does not duplicate attachments");
  await walkBack(30);
  const psi = (await game.info()).player.psi_points!;
  await fireOnce(game);
  assert.equal((await game.info()).player.psi_points, psi - 5);
  assert.equal((await game.entities.byTemplate(-1109)).length, 0);
  assert.equal((await game.entities.list({ filter: "TeleportPt" })).entities.length, 0, "recall removes attachments");
  await fireOnce(game);
  await game.transitionLevel("earth.mis");
  await game.step({ frames: 10 });
  assert.equal((await game.entities.byTemplate(-1109)).length, 0);
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 10 });
  assert.equal((await game.entities.byTemplate(-1109)).length, 0, "returning to the cached level cannot resurrect a marker");
  assert.equal((await game.entities.list({ filter: "TeleportPt" })).entities.length, 0);
});

// A deliberately obstructed marker isolates the safety boundary: the debug
// teleport provisions an invalid pose; the power itself must never recall there.
test("Quantum Relocation refuses an obstructed marker without consuming it", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  await selectPsiPower(game, "Teleport");
  await game.player.teleport({ x: -15, y: 2, z: 0 });
  await fireOnce(game);
  const [marker] = await game.entities.byTemplate(-1109);
  assert.ok(marker);
  await game.player.teleport({ x: 0, y: 1.24, z: 0 });
  await game.step({ frames: 30 });
  const before = (await game.info()).player;
  await fireOnce(game);
  const after = (await game.info()).player;
  assert.equal(after.psi_points, before.psi_points);
  assert.deepEqual(after.position, before.position);
  assert.equal((await game.entities.byTemplate(-1109))[0]?.id, marker.id);

});

test("Quantum Relocation is edge-triggered and an unaffordable recall preserves its marker", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  await selectPsiPower(game, "Teleport");
  for (let cast = 0; cast < 18; cast++) await fireOnce(game);
  assert.equal((await game.info()).player.psi_points, 5);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 180 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  const before = (await game.info()).player;
  assert.equal(before.psi_points, 0);
  assert.equal(before.psi_charge, null, "Teleport does not start an overload meter");
  const [marker] = await game.entities.byTemplate(-1109);
  assert.ok(marker, "one held trigger creates exactly one marker");
  await fireOnce(game);
  assert.equal((await game.entities.byTemplate(-1109))[0]?.id, marker.id);
  assert.deepEqual((await game.info()).player.position, before.position);
  assert.equal((await game.info()).player.psi_points, 0);
  await game.input.trigger("ClearPsiTeleport");
  await game.step({ frames: 2 });
  assert.equal((await game.entities.byTemplate(-1109)).length, 0, "clearing needs no psi");
});
