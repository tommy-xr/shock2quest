import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas, normalize, quatConjugate, quatFromTo, quatMultiply, quatRotate, sub } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
const launch = (mission = "debug_interactions") => GameServer.launch({ mission, debugFlags: ["--vr"] });
async function reach(game: GameServer, hand: "left" | "right", slot: number) {
  const p = (await game.info()).player;
  const center = p.hand_feedback?.chest_slots?.centers?.[slot];
  assert.ok(center);
  await game.input.set(`${hand}_hand.position`, quatRotate(quatConjugate(p.rotation), sub(center, p.position)));
  await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.hand_feedback?.chest_slots?.near[hand === "left" ? 0 : 1], slot);
}
async function grab(game: GameServer, template: number, hand: "left" | "right" = "right") {
  const item = (await game.entities.list()).entities.find(e => e.template_id === template);
  assert.ok(item);
  await aimVrHandAt(game, item.position, 0.2, 1, 0, { hand });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player[hand === "left" ? "wielded_entity_id" : "right_hand_entity_id"], item.id);
  return item;
}
async function stow(game: GameServer, hand: "left" | "right", slot: number) {
  await reach(game, hand, slot);
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 8 });
}


test("chest mounts store the exact hypo in the backpack and draw without using it", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const chest = () => game.info().then(info => info.player.hand_feedback?.chest_slots);
  assert.equal((await chest())?.enabled_slots, 2, "two chest slots are standard equipment");
  const item = (await game.entities.list()).entities.find(e => e.template_id === -52);
  assert.ok(item);
  await aimVrHandAt(game, item.position, 0.2, 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, item.id);
  const player = (await game.info()).player;
  const center = (await chest())!.centers![0];
  await game.input.set("right_hand.position", quatRotate(quatConjugate(player.rotation), sub(center, player.position)));
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  assert.equal((await chest())!.near[1], 0);
  assert.equal((await game.info()).player.right_hand_entity_id, item.id, "approach is not release");
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  assert.deepEqual((await chest())!.items, [item.id, null]);
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === item.id));
  assert.equal((await game.physics.bodies({ entityId: item.id })).bodies.length, 0);
  await game.input.set("right_hand.trigger", 1);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 30 });
  assert.equal((await game.info()).player.right_hand_entity_id, item.id, "drawing with trigger held must not consume the hypo");
  assert.deepEqual((await chest())!.items, [null, null]);
});

for (const hand of ["left", "right"] as const) {
  for (const slot of [0, 1]) {
    test(`${hand} hand can dock and retrieve from chest ${slot}`, { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await launch();
      await game.step({ frames: 30 });
      const item = await grab(game, -57, hand);
      await stow(game, hand, slot);
      const p = (await game.info()).player;
      assert.equal(p.hand_feedback?.chest_slots?.items[slot], item.id);
      assert.ok(p.hand_feedback!.haptics!.sequence[hand === "left" ? 0 : 1] > 0, "seat has tactile feedback");
      await game.input.set(`${hand}_hand.squeeze`, 1);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player[hand === "left" ? "wielded_entity_id" : "right_hand_entity_id"], item.id);
      assert.deepEqual((await game.info()).player.hand_feedback?.chest_slots?.items, [null, null]);
    });
  }
}

test("occupied chest mount refuses another item and requires explicit regrip before dropping", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  const hypo = await grab(game, -52);
  await stow(game, "right", 0);
  const other = await grab(game, -57);
  await reach(game, "right", 0);
  assert.deepEqual((await game.info()).player.hand_feedback?.chest_slots?.refusal_flash, [0, 0], "hovering an occupied slot does not flash refusal");
  await stow(game, "right", 0);
  assert.ok((await game.info()).player.hand_feedback!.chest_slots!.refusal_flash[0] > 0, "refused release flashes the targeted slot");
  await game.step({ frames: 30 });
  assert.deepEqual((await game.info()).player.hand_feedback?.chest_slots?.refusal_flash, [0, 0], "refusal flash expires even while the retained item stays near");
  assert.equal((await game.info()).player.right_hand_entity_id, other.id);
  assert.deepEqual((await game.info()).player.hand_feedback?.chest_slots?.items, [hypo.id, null]);
  await game.input.set("right_hand.position", [0.4, 0.4, -0.7]);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, other.id, "leaving a refused mount must not drop");
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  assert.equal((await game.entities.list()).entities.find(e => e.id === other.id)?.location, "world", "a deliberate release away from every body slot returns the refused item to the world");
});

test("chest mounts reject weapons and full-backpack deposits without losing them", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  const weapon = await grab(game, -17, "left");
  await stow(game, "left", 1);
  assert.equal((await game.info()).player.wielded_entity_id, weapon.id);
  const hypo = await grab(game, -52);
  for (let i = 0; i < 45; i++) {
    try { await game.player.spawnItem(-1221); }
    catch (error) { assert.match(String(error), /could not add item to inventory/); break; }
  }
  await reach(game, "right", 0);
  assert.equal((await game.info()).player.hand_feedback?.chest_slots?.can_store[1], false);
  await stow(game, "right", 0);
  assert.equal((await game.info()).player.right_hand_entity_id, hypo.id);
  assert.deepEqual((await game.info()).player.hand_feedback?.chest_slots?.items, [null, null]);
});

test("simultaneous chest draws never duplicate the stored item", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  const hypo = await grab(game, -52);
  await stow(game, "right", 0);
  await reach(game, "left", 0);
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 2 });
  await game.input.set("left_hand.squeeze", 1);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  const player = (await game.info()).player;
  assert.equal([player.wielded_entity_id, player.right_hand_entity_id].filter(id => id === hypo.id).length, 1);
  assert.deepEqual(player.hand_feedback?.chest_slots?.items, [null, null]);
});

test("chest assignments survive save and transition while flat inventory retains access", { skip: !enabled, timeout: 240_000 }, async () => {
  await using game = await launch("medsci1.mis");
  await game.step({ frames: 8 });
  await game.player.spawnItem(-52);
  await game.player.spawnItem(-52);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const inventory = (await game.player.inventory()).items;
  const entities = (await game.entities.list()).entities;
  const hypo = entities.find(e => e.template_id === -52 && inventory.some(i => i.entity_id === e.id));
  assert.ok(hypo);
  const count = (id: number) => game.entities.detail(id).then(d => Number(d.properties.find(p => p.name === "StackCount")?.value));
  const originalCount = await count(hypo.id);
  assert.ok(originalCount >= 2);
  const ui = await game.ui.state();
  const cell = ui.strip?.elements.find(e => e.entity_id === hypo.id);
  assert.ok(cell);
  await aimVrHandAtCanvas(game, ui.panel_pose!, [cell.rect[0] + cell.rect[2] / 2, cell.rect[1] + cell.rect[3] / 2], { hand: "right", squeeze: 1 });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, hypo.id);
  await stow(game, "right", 1);
  assert.equal((await game.info()).player.hand_feedback?.chest_slots?.items[1], hypo.id);
  const name = `chest-${Date.now()}`;
  await game.save(name);
  await game.load(name);
  await game.step({ frames: 8 });
  let stored = (await game.info()).player.hand_feedback?.chest_slots?.items[1];
  assert.ok(stored != null);
  assert.equal(await count(stored), originalCount);
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === stored));
  await game.transitionLevel("medsci2.mis");
  await game.step({ frames: 8 });
  stored = (await game.info()).player.hand_feedback?.chest_slots?.items[1];
  assert.ok(stored != null);
  assert.equal(await count(stored), originalCount);
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === stored));
  await game.save(name);
  await using flat = await GameServer.launch({ mission: "medsci1.mis" });
  await flat.load(name);
  await flat.step({ frames: 8 });
  const flatInventory = (await flat.player.inventory()).items;
  assert.ok((await flat.entities.list()).entities.some(e => e.template_id === -52 && flatInventory.some(i => i.entity_id === e.id)));
});

test("a portable battery docks without being consumed and draws into either hand", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  await game.input.set("right_hand.squeeze", 1);
  await game.player.spawnItem(-3641, { hand: "right" });
  await game.step({ frames: 5 });
  const id = (await game.info()).player.right_hand_entity_id;
  assert.ok(id != null);
  await stow(game, "right", 1);
  assert.equal((await game.info()).player.hand_feedback?.chest_slots?.items[1], id);
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === id));
  await game.input.set("right_hand.position", [0.4, 0.4, -0.7]);
  await reach(game, "left", 1);
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.wielded_entity_id, id);
});

test("simultaneous chest and shoulder deposits reserve the last backpack cell only once", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  const left = await grab(game, -52, "left");
  const right = await grab(game, -57, "right");
  for (let i = 0; i < 44; i++) await game.player.spawnItem(-1221);
  assert.equal((await game.player.inventory()).items.filter(i => i.entity_id !== left.id && i.entity_id !== right.id).length, 44);
  await reach(game, "left", 0);
  await reach(game, "right", 1);
  assert.deepEqual((await game.info()).player.hand_feedback?.chest_slots?.can_store, [true, false], "preview reserves the last cell only once");
  const p = (await game.info()).player;
  const shoulder = p.hand_feedback?.shoulder_backpack?.centers?.[1];
  assert.ok(shoulder);
  await game.input.set("right_hand.position", quatRotate(quatConjugate(p.rotation), sub(shoulder, p.position)));
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.hand_feedback?.shoulder_backpack?.near[1], true);
  await game.input.set("left_hand.squeeze", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  const after = (await game.info()).player;
  assert.equal(after.hand_feedback?.chest_slots?.items[0], left.id);
  assert.equal(after.right_hand_entity_id, right.id);
  assert.equal((await game.player.inventory()).items.filter(i => i.entity_id !== right.id).length, 45);
});

test("chest counts all matching cells, draws one, refills, and accepts a return", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  const original = await grab(game, -52);
  // Provision the second entity while the first is held: spawnItem otherwise
  // merges into an existing backpack stack before handing that stack over.
  await game.input.set("left_hand.squeeze", 1);
  const extra = await game.player.spawnItem(-52, { hand: "left" });
  await game.step({ frames: 2 });
  await stow(game, "right", 0);
  await game.player.spawnItem(-52); // Merge a second unit into the assigned reserve.
  await stow(game, "left", 1); // A distinct inventory cell, same item kind.
  const chest = () => game.info().then(info => info.player.hand_feedback!.chest_slots!);
  const count = async (id: number) => Number((await game.entities.detail(id)).properties.find(p => p.name === "StackCount")?.value ?? 1);
  assert.notEqual(extra.entity_id, original.id);
  assert.deepEqual((await chest()).counts, [3, 3]);
  const draw = async (hand: "left" | "right", slot: number) => {
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await reach(game, hand, slot);
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 8 });
    const p = (await game.info()).player;
    const id = hand === "right" ? p.right_hand_entity_id : p.wielded_entity_id;
    assert.ok(id != null);
    assert.equal(await count(id), 1, "one use comes out, never the whole stack");
    return id;
  };
  await draw("right", 0);
  assert.deepEqual((await chest()).counts, [2, 2]);
  assert.equal((await chest()).items[0], original.id, "split leaves its reserve assigned");
  await stow(game, "right", 0);
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  assert.deepEqual((await chest()).counts, [3, 3], "return merges into the refilled slot");
  const first = await draw("right", 0);
  await game.input.set("right_hand.position", [0.4, 0.4, -0.7]);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.deepEqual((await chest()).counts, [2, 2], "world pickups are excluded");
  const second = await draw("right", 0);
  assert.deepEqual((await chest()).counts, [1, 1]);
  assert.deepEqual((await chest()).items, [extra.entity_id, extra.entity_id], "both slots can reference the remaining reserve without duplicating it");
  const third = await draw("left", 1);
  assert.equal(new Set([first, second, third]).size, 3);
  assert.deepEqual((await chest()).counts, [0, 0]);
  assert.deepEqual((await chest()).items, [null, null]);
});

test("an empty chest mount passes world grabs through while an occupied mount owns its draw", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch();
  await game.step({ frames: 30 });
  const entities = (await game.entities.list()).entities;
  const hypo = entities.find(e => e.template_id === -52);
  const other = entities.find(e => e.template_id === -57);
  assert.ok(hypo && other);
  const aimThroughMount = async (target: typeof hypo) => {
    const before = (await game.info()).player;
    await game.player.teleport({ x: target.position[0] - 0.18, y: before.position[1], z: target.position[2] + 0.6 });
    await game.step({ frames: 3 });
    await reach(game, "right", 0);
    const player = (await game.info()).player;
    const center = player.hand_feedback!.chest_slots!.centers![0];
    const rotation = quatMultiply(quatConjugate(player.rotation), quatFromTo([0, 0, -1], normalize(sub(target.position, center))));
    await game.input.set("right_hand.rotation", rotation);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.hand_feedback!.chest_slots!.near[1], 0, "world grab starts inside the chest target");
  };
  await aimThroughMount(hypo);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, hypo.id, "empty mount must not swallow the world grab");
  await stow(game, "right", 0);
  assert.equal((await game.info()).player.hand_feedback!.chest_slots!.items[0], hypo.id);
  await aimThroughMount(other);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, hypo.id, "occupied mount draws its item instead of the world item behind it");
});

test("shared chest reserves survive saves and refill between simultaneous draws", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await launch("medsci1.mis");
  await game.step({ frames: 30 });
  // Hold both initial items before provisioning the third, so all three are
  // distinct entities rather than spawnItem's normal merged backpack stack.
  for (const hand of ["right", "left"] as const) {
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.player.spawnItem(-52, { hand });
    await game.step({ frames: 3 });
  }
  const reserve = await game.player.spawnItem(-52);
  await stow(game, "right", 0);
  await stow(game, "left", 1);
  // The unassigned reserve is the first backpack cell. Exhaust both assigned
  // singleton sources so both slots refill from that same surviving entity.
  for (const [hand, slot] of [["right", 0], ["left", 1]] as const) {
    await reach(game, hand, slot);
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 5 });
  }
  assert.deepEqual((await game.info()).player.hand_feedback!.chest_slots!.items,
    [reserve.entity_id, reserve.entity_id]);
  const separateId = (await game.info()).player.right_hand_entity_id;
  assert.ok(separateId != null);
  assert.notEqual(separateId, reserve.entity_id, "the returned item is a distinct reserve");
  await game.input.set("left_hand.position", [-0.4, 0.4, -0.7]);
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 5 });
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const ui = await game.ui.state();
  assert.ok(ui.panel_pose);
  await aimVrHandAtCanvas(game, ui.panel_pose, [205, 50], { hand: "right", squeeze: 1 });
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === separateId && i.location === "inventory"), "explicit empty cell keeps a separate reserve");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const chest = () => game.info().then(info => info.player.hand_feedback!.chest_slots!);
  assert.deepEqual((await chest()).counts, [2, 2]);
  assert.equal((await chest()).items[0], (await chest()).items[1]);
  const save = `chest-shared-${Date.now()}`;
  await game.save(save);
  await game.load(save);
  await game.step({ frames: 8 });
  assert.deepEqual((await chest()).counts, [2, 2]);
  assert.equal((await chest()).items[0], (await chest()).items[1], "shared reserve bits round-trip with the remapped entity");
  await game.input.set("left_hand.squeeze", 0);
  await game.input.set("right_hand.squeeze", 0);
  await reach(game, "left", 1);
  await reach(game, "right", 0);
  await game.input.set("left_hand.squeeze", 1);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  const after = (await game.info()).player;
  assert.ok(after.wielded_entity_id != null && after.right_hand_entity_id != null, "both fresh squeezes draw despite the intervening refill");
  assert.notEqual(after.wielded_entity_id, after.right_hand_entity_id);
  assert.deepEqual((await chest()).counts, [0, 0]);
});
