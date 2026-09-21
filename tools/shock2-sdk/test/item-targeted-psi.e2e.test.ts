import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { clickUiElement } from "./helpers/ui.js";
import { ammoOf, fireOnce, pullTrigger } from "./helpers/weapon.js";

async function property(game: GameServer, id: number, name: string): Promise<number> {
  const p = (await game.entities.detail(id)).properties.find(p => p.name === name);
  assert.ok(p, `${id} must expose ${name}`);
  return Number(p.value);
}
async function slot(game: GameServer, id: number) {
  const ui = await game.ui.state();
  const element = ui.strip?.elements.find(e => e.entity_id === id && e.kind === "button");
  assert.ok(element, `inventory must show ${id}`);
  return element;
}
async function shooter(game: GameServer) {
  if ((await game.ui.state()).mode === "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
  }
}
async function castAt(game: GameServer, name: string, target: number, overload = false) {
  await shooter(game);
  await selectPsiPower(game, name);
  const psi = (await game.info()).player.psi_points;
  if (overload) {
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: name === "Fabricate" ? 81 : 67 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
  } else {
    await pullTrigger(game);
    await game.step({ frames: 2 });
  }
  assert.equal((await game.info()).player.psi_points, psi, "choosing a target spends nothing");
  assert.equal((await game.ui.state()).mode, "use");
  assert.match((await game.ui.state()).name_strip ?? "", /Select/);
  await clickUiElement(game, await slot(game, target));
  return psi!;
}

test("flat item powers and Recycler use authored values through inventory gestures", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  const implant = (await game.player.spawnItem(-101)).entity_id;
  const patch = (await game.player.spawnItem(-52)).entity_id;
  const recycler = (await game.player.spawnItem(-71)).entity_id;
  assert.equal(await property(game, implant, "Energy"), 100);
  let psi = await castAt(game, "ElectroPsi", implant);
  assert.equal(await property(game, implant, "Energy"), 160, "Maintenance 6 permits 160 charge");
  assert.equal((await game.info()).player.psi_points, psi - 3);
  psi = await castAt(game, "ElectroPsi", implant);
  assert.equal((await game.info()).player.psi_points, psi, "full charge refuses without spending");
  psi = await castAt(game, "ElectroPsi", recycler);
  assert.equal((await game.info()).player.psi_points, psi, "non-rechargeable target refuses");

  const laser = (await game.player.spawnItem(-22)).entity_id;
  await shooter(game);
  await game.input.trigger("EquipLaserPistol"); await game.step({ frames: 3 });
  assert.equal((await game.info()).player.wielded_entity_id, laser);
  await fireOnce(game);
  const drained = ammoOf(await game.entities.detail(laser));
  assert.ok(drained < 100, "firing drains the energy weapon");
  await game.input.trigger("EquipPsiAmp"); await game.step({ frames: 3 });
  psi = await castAt(game, "ElectroPsi", laser);
  assert.equal(ammoOf(await game.entities.detail(laser)), 160);
  assert.equal((await game.info()).player.psi_points, psi - 3);

  const money = (await game.info()).player.stats!.nanites;
  psi = await castAt(game, "Fabricate", patch, true); // PSI 6 + overload 2 => 100% success.
  assert.equal(await property(game, patch, "StackCount"), 2, "adds one authored hypo to the same stack");
  assert.equal((await game.info()).player.stats!.nanites, money - 20);
  assert.equal((await game.info()).player.psi_points, psi - 3);
  psi = await castAt(game, "Alchemy", patch);
  assert.equal(await property(game, patch, "StackCount"), 1);
  assert.equal((await game.info()).player.stats!.nanites, money - 20 + 8, "4 * (0.8 + 0.2 * PSI 6)");
  assert.equal((await game.info()).player.psi_points, psi - 4);

  // Feed the last hypo to the portable Recycler by dragging it onto the device.
  const before = (await game.info()).player.stats!.nanites;
  await clickUiElement(game, await slot(game, patch));
  assert.equal((await game.ui.state()).cursor?.entity_id, patch);
  await clickUiElement(game, await slot(game, recycler));
  assert.equal((await game.info()).player.stats!.nanites, before + 2);
  const items = (await game.player.inventory()).items;
  assert.ok(!items.some(i => i.entity_id === patch));
  assert.ok(items.some(i => i.entity_id === recycler), "the Recycler is reusable");
  assert.equal((await game.ui.state()).cursor, null);

  // Cancel a pending cast with Tab. Opening again must restore ordinary inventory clicks.
  await shooter(game);
  await selectPsiPower(game, "Alchemy");
  const unchanged = (await game.info()).player.psi_points;
  await pullTrigger(game);
  await shooter(game);
  await game.input.trigger("ToggleUseMode"); await game.step({ frames: 3 });
  await clickUiElement(game, await slot(game, recycler));
  assert.equal((await game.ui.state()).cursor?.entity_id, recycler);
  assert.equal((await game.info()).player.psi_points, unchanged);

  // Round-trip the actual outcomes through an authored mission (debug scenes
  // themselves do not have a mission file to reload).
  await shooter(game);
  await game.transitionLevel("earth.mis");
  await game.step({ frames: 3 });
  const savedMoney = (await game.info()).player.stats!.nanites;
  const save = `item_powers_e2e_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  await game.step({ frames: 3 });
  const restored = (await game.player.inventory()).items;
  const restoredImplant = (await game.entities.byTemplate(-101)).find(e => restored.some(i => i.entity_id === e.id));
  assert.ok(restoredImplant, "recharged implant survives save/load");
  assert.equal(await property(game, restoredImplant.id, "Energy"), 160);
  assert.ok((await game.entities.byTemplate(-71)).some(e => restored.some(i => i.entity_id === e.id)), "reusable Recycler survives save/load");
  assert.equal((await game.info()).player.stats!.nanites, savedMoney);
});

for (const toolHand of ["right", "left"] as const) {
  test(`VR ${toolHand}-hand psi targets the opposite item and Recycler accepts a release`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    const { aimVrHandAt } = await import("./helpers/vr-hand.js");
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    const targetHand = toolHand === "right" ? "left" : "right";
    async function grab(template: number, hand: "left" | "right") {
      const [item] = await game.entities.byTemplate(template);
      assert.ok(item, `scene contains ${template}`);
      await aimVrHandAt(game, item.position, .2, 0, 0, { hand });
      const pose = await aimVrHandAt(game, (await game.entities.detail(item.id)).position, .2, 1, 0, { hand });
      await game.step({ frames: 3 });
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === item.id && i.location === `${hand}_hand`));
      return { id: item.id, pose };
    }
    const amp = await grab(-247, toolHand);
    const implant = await grab(-101, targetHand);
    async function cast(name: string, overloaded = false) {
      await selectPsiPower(game, name);
      const before = (await game.info()).player.psi_points!;
      await game.input.set(`${toolHand}_hand.trigger`, 1);
      await game.step({ frames: overloaded ? 81 : 1 });
      await game.input.set(`${toolHand}_hand.trigger`, 0);
      await game.step({ frames: 3 });
      return before;
    }
    let psi = await cast("ElectroPsi");
    assert.equal(await property(game, implant.id, "Energy"), 160);
    assert.equal((await game.info()).player.psi_points, psi - 3);
    psi = await cast("ElectroPsi");
    assert.equal((await game.info()).player.psi_points, psi, "full target costs nothing");
    await game.input.set(`${targetHand}_hand.squeeze`, 0); await game.step({ frames: 3 });
    const patch = await grab(-52, targetHand);
    const money = (await game.info()).player.stats!.nanites;
    psi = await cast("Fabricate", true);
    assert.equal(await property(game, patch.id, "StackCount"), 2);
    assert.equal((await game.info()).player.stats!.nanites, money - 20);
    assert.equal((await game.info()).player.psi_points, psi - 3);
    psi = await cast("Alchemy");
    assert.equal(await property(game, patch.id, "StackCount"), 1);
    assert.equal((await game.info()).player.psi_points, psi - 4);
    assert.ok((await game.player.inventory()).items.some(i => i.entity_id === amp.id && i.location === `${toolHand}_hand`));

    await game.input.set(`${toolHand}_hand.squeeze`, 0); await game.step({ frames: 3 });
    const recycler = await grab(-71, toolHand);
    // Bring the held target to the Recycler. Contact alone must leave it intact.
    await game.input.set(`${targetHand}_hand.position`, recycler.pose.local);
    await game.input.set(`${targetHand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 5 });
    const before = (await game.info()).player.stats!.nanites;
    assert.equal(await property(game, patch.id, "StackCount"), 1);
    await game.input.set(`${toolHand}_hand.trigger`, 1); await game.step({ frames: 2 });
    await game.input.set(`${toolHand}_hand.trigger`, 0); await game.step({ frames: 2 });
    assert.equal((await game.info()).player.stats!.nanites, before, "Recycler trigger explains; it does not consume");
    await game.input.set(`${targetHand}_hand.squeeze`, 0); await game.step({ frames: 5 });
    assert.equal((await game.info()).player.stats!.nanites, before + 2, "deliberate feed earns authored nanites");
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === patch.id));
    assert.ok((await game.player.inventory()).items.some(i => i.entity_id === recycler.id && i.location === `${toolHand}_hand`));
    assert.equal((await game.physics.bodies({ entityId: patch.id })).bodies.length, 0);

    const clip = await grab(-1358, targetHand);
    const originalCount = await property(game, clip.id, "StackCount");
    await game.input.set(`${targetHand}_hand.position`, [
      recycler.pose.local[0] + 1, recycler.pose.local[1], recycler.pose.local[2],
    ]);
    await game.step({ frames: 3 });
    const beforeDrop = (await game.info()).player.stats!.nanites;
    await game.input.set(`${targetHand}_hand.squeeze`, 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.stats!.nanites, beforeDrop, "dropping away from the Recycler awards nothing");
    assert.equal(await property(game, clip.id, "StackCount"), originalCount, "ordinary release preserves the item");
    assert.ok((await game.physics.bodies({ entityId: clip.id })).bodies.length > 0, "the dropped clip remains in the world");

  });
}
