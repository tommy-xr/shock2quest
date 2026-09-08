import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import {
  aimVrHandAt,
  aimVrHandAtCanvas,
  quatConjugate,
  quatRotate,
  sub,
} from "./helpers/vr-hand.js";
import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
const owner = (hand: "left" | "right") =>
  hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
async function reach(
  game: GameServer,
  hand: "left" | "right",
  slot: number,
) {
  const player = (await game.info()).player;
  const center = player.hand_feedback?.holsters?.centers?.[slot];
  assert.ok(center, "tracked holster zones must be published");
  await game.input.set(
    `${hand}_hand.position`,
    quatRotate(quatConjugate(player.rotation), sub(center, player.position)),
  );
  await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  assert.equal(
    (await game.info()).player.hand_feedback?.holsters?.near[
      hand === "left" ? 0 : 1
    ],
    slot,
  );
}

for (const hand of ["left", "right"] as const) {
  test(`${hand} holster preserves the exact weapon and ammunition`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const item = (await game.entities.list()).entities.find(e => e.template_id === -17)!;
    assert.ok(item);
    await aimVrHandAt(game, item.position, 0.2, 1, 0, { hand });
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player[owner(hand)], item.id);
    await game.entities.sendMessage(item.id, { type: "SetGunCondition", condition: 57 });
    await game.step({ frames: 1 });
    const ammo = ammoOf(await game.entities.detail(item.id));
    if (hand === "right") {
      // Holsters are dedicated capacity even when the largest backpack is full.
      for (let i = 0; i < 45; i++) await game.player.spawnItem(-1221);
    }
    await reach(game, hand, 0);
    assert.equal((await game.info()).player[owner(hand)], item.id, "reaching is not releasing");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player[owner(hand)], null);
    assert.deepEqual((await game.info()).player.hand_feedback?.holsters?.items, [item.id, null]);
    assert.equal((await game.player.inventory()).items.find(i => i.entity_id === item.id), undefined);
    assert.equal((await game.physics.bodies({ entityId: item.id })).bodies.length, 0);
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 90 });
    assert.equal((await game.info()).player[owner(hand)], item.id);
    assert.deepEqual((await game.info()).player.hand_feedback?.holsters?.items, [null, null]);
    assert.equal(ammoOf(await game.entities.detail(item.id)), ammo, "drawing with trigger held must not fire");
    assert.equal((await game.info()).player.wielded_gun_condition, 57);
  });
}

test("occupied holster retains the refused weapon until a deliberate regrip", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const entities = (await game.entities.list()).entities;
  const pistol = entities.find(e => e.template_id === -17)!;
  const wrench = entities.find(e => e.template_id === -928)!;
  assert.ok(pistol && wrench);
  await aimVrHandAt(game, pistol.position, 0.2, 1);
  await game.step({ frames: 5 });
  await reach(game, "right", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  await aimVrHandAt(game, wrench.position, 0.2, 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);
  await reach(game, "right", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);
  assert.equal((await game.info()).player.hand_feedback?.holsters?.retained[1], true);
  assert.deepEqual((await game.info()).player.hand_feedback?.holsters?.items, [pistol.id, null]);
  await game.input.set("right_hand.position", [0.3, 1, -0.4]);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, null);
});

test("holstered weapon survives real mission save, load, and transition", { skip: !enabled, timeout: 240_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  const existing = new Set((await game.entities.list()).entities.map(e => e.id));
  await game.player.spawnItem(-17);
  const item = (await game.entities.list()).entities.find(e => e.template_id === -17 && !existing.has(e.id))!;
  assert.ok(item);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const ui = await game.ui.state();
  const cell = ui.strip?.elements.find(e => e.entity_id === item.id);
  assert.ok(cell);
  await aimVrHandAtCanvas(game, ui.panel_pose!, [cell.rect[0] + cell.rect[2] / 2, cell.rect[1] + cell.rect[3] / 2], { hand: "right", squeeze: 1 });
  await game.step({ frames: 5 });
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, item.id);
  const ammo = ammoOf(await game.entities.detail(item.id));
  await reach(game, "right", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  const name = `holster-${Date.now()}`;
  await game.save(name);
  await game.load(name);
  await game.step({ frames: 8 });
  const saved = (await game.info()).player.hand_feedback?.holsters?.items[0];
  assert.ok(saved != null);
  assert.equal(ammoOf(await game.entities.detail(saved)), ammo);
  assert.equal((await game.physics.bodies({ entityId: saved })).bodies.length, 0);
  await reach(game, "left", 0);
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.wielded_entity_id, saved);
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.hand_feedback?.holsters?.items[0], saved);
  await game.transitionLevel("medsci2.mis");
  await game.step({ frames: 8 });
  const moved = (await game.info()).player.hand_feedback?.holsters?.items[0];
  assert.ok(moved != null);
  assert.equal(ammoOf(await game.entities.detail(moved)), ammo);
  await game.save(name);
  await using flat = await GameServer.launch({ mission: "medsci1.mis" });
  await flat.load(name);
  await flat.step({ frames: 8 });
  assert.ok((await flat.info()).player.hand_feedback == null, "flat presentation has no VR hand diagnostics");
  const inventory = await flat.player.inventory();
  assert.ok(inventory.items.length > 0, "flat load returns body storage to accessible backpack cells");
  const carried = (await flat.entities.list()).entities.find(e => e.template_id === -17 && inventory.items.some(i => i.entity_id === e.id));
  assert.ok(carried, "the stowed pistol must be in the backpack after flat load");
  assert.equal(ammoOf(await flat.entities.detail(carried.id)), ammo);
  let filled = 0;
  for (let i = 0; i < 45; i++) {
    try { await game.player.spawnItem(-1221); filled++; }
    catch (error) {
      // Real-mission spawn reports a failed capacity check as this legacy error.
      assert.match(String(error), /could not add item to inventory/);
      break;
    }
  }
  assert.ok(filled > 0);
  await game.save(name);
  await flat.load(name);
  await flat.step({ frames: 8 });
  const packed = new Set((await flat.player.inventory()).items.map(i => i.entity_id));
  const overflow = (await flat.entities.list()).entities.find(e => e.template_id === -17 && !packed.has(e.id));
  assert.ok(overflow, "full backpack exposes the holstered pistol as a world pickup");
  assert.equal(ammoOf(await flat.entities.detail(overflow.id)), ammo);
  assert.ok((await flat.physics.bodies({ entityId: overflow.id })).bodies.length > 0);
  assert.ok(Math.hypot(...sub(overflow.position, (await flat.info()).player.position)) < 3,
    "overflow must drop beside the player, not at its saved old-world position");
});


test("drawing a holstered wrench restores its physical melee body", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const wrench = (await game.entities.list()).entities.find(e => e.template_id === -928)!;
  assert.ok(wrench);
  await aimVrHandAt(game, wrench.position, 0.2, 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);
  await reach(game, "right", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.hand_feedback?.holsters?.items[0], wrench.id);
  assert.equal((await game.physics.bodies({ entityId: wrench.id })).bodies.length, 0);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);
  assert.equal((await game.info()).player.hand_feedback?.holsters?.items[0], null);
  assert.ok((await game.physics.bodies({ entityId: wrench.id })).bodies.length > 0);
});
