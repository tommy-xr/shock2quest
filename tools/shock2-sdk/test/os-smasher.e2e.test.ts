import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

test("Smasher waits for release and adds six base damage before target armor", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await acquireOsUpgrade(game, "Smasher");
  await game.transitionLevel("earth.mis");
  await game.player.spawnItem(-928);
  await game.input.trigger("EquipWrench");
  await game.step({ frames: 5 });
  const [droid] = await game.entities.byTemplate(593);
  assert.ok(droid);
  const [x, y, z] = (await game.entities.detail(droid.id)).position;
  await game.player.teleport({ x: x + 1.2, y: y + 1, z });
  await game.step({ frames: 60 });
  await game.player.aimAt(droid, { hitbox: "torso", visibility: "required" });
  await game.step({ frames: 3 });
  const hp = async () => Number((await game.entities.detail(droid.id)).properties.find(p => p.name === "HitPoints")!.value);
  // The droid's WeaponBash receptron halves the Wrench's authored 9 damage.
  // A charged strike adds six before that reduction, then rounds HP loss.
  for (const [hold, expected] of [[6, 5], [80, 8]]) {
    const before = await hp();
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: hold });
    assert.equal(await hp(), before, "holding the trigger only winds up");
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 120 });
    assert.equal(before - await hp(), expected);
    assert.ok(await hp() > 0, "no zero-HP clamping");
  }
});

test("VR Smasher cues charge and ready once on the owning hand", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  const save = `os_smasher_haptics_${Date.now()}`;
  {
    await using flat = await GameServer.launch({ mission: "medsci2.mis" });
    await acquireOsUpgrade(flat, "Smasher");
    await flat.player.spawnItem(-928);
    await flat.input.trigger("EquipWrench");
    await flat.step({ frames: 5 });
    assert.ok((await flat.save(save)).success);
  }
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.input.set("left_hand.squeeze", 1);
  assert.ok((await game.load(save)).success);
  await game.step({ frames: 30 });
  assert.ok((await game.info()).player.stats!.os_traits.includes(11));
  const pulses = async () => (await game.info()).player.hand_feedback!.haptics!.sequence;
  const renderOffset = async (id: number) => {
    const body = await game.entities.detail(id);
    const draw = (await game.scene.objects({ entityId: id })).objects.find(o => o.source === "entity");
    assert.ok(draw, "held wrench is rendered");
    return Math.hypot(...draw.position.map((v, axis) => v - body.position[axis]));
  };
  for (const hand of ["left", "right"] as const) {
    const i = hand === "left" ? 0 : 1;
    const owner = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
    let id = (await game.info()).player.wielded_entity_id;
    if (hand === "right") {
      const existing = new Set((await game.entities.list()).entities.map(e => e.id));
      await game.player.spawnItem(-928);
      id = (await game.entities.list()).entities.find(e => e.template_id === -928 && !existing.has(e.id))!.id;
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      const ui = await game.ui.state();
      const cell = ui.strip?.elements.find(e => e.entity_id === id);
      assert.ok(cell);
      await aimVrHandAtCanvas(game, ui.panel_pose!, [cell.rect[0] + cell.rect[2] / 2, cell.rect[1] + cell.rect[3] / 2], { hand, squeeze: 1 });
      await game.step({ frames: 5 });
      await game.input.trigger("ToggleUseMode");
    }
    assert.ok(id != null);
    await game.input.set(`${hand}_hand.position`, [i === 0 ? -0.6 : 0.6, 1.2, -0.6]);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player[owner], id, `${hand} holds the wrench`);
    assert.ok(await renderOffset(id) < 1e-5, "idle mesh follows physical pose");
    const before = await pulses();
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    assert.equal((await pulses())[i], before[i] + 1, "light start pulse");
    await game.step({ frames: 30 });
    assert.equal((await pulses())[i], before[i] + 2, "ready pulse after 380 ms");
    const vibrating = await renderOffset(id);
    assert.ok(vibrating > 0.0001 && vibrating < 0.02, "charged mesh trembles relative to its physical pose");
    await game.step({ frames: 60 });
    assert.equal((await pulses())[i], before[i] + 2, "holding ready does not buzz repeatedly");
    await game.input.set(`${hand}_hand.trigger`, 0);
    await game.step({ frames: 2 });
    assert.ok(await renderOffset(id) < 1e-5, "release immediately stops motion while strike is armed");
    await game.step({ frames: 58 });
    assert.equal((await pulses())[i], before[i] + 2, "release and expiry are silent");
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    await game.input.set(`${hand}_hand.trigger`, 0);
    await game.step({ frames: 30 });
    assert.equal((await pulses())[i], before[i] + 3, "short charge has no ready pulse");
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 30 });
    assert.equal((await game.info()).player[owner], null);
    assert.equal((await pulses())[i], before[i] + 4, "dropping cancels the ready pulse");
    assert.equal((await pulses())[1 - i], before[1 - i], "other controller stays silent");
    await game.input.set(`${hand}_hand.trigger`, 0);
    await game.step({ frames: 3 });
  }
});
