import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
async function state(game: GameServer, id: number) {
  const props = (await game.entities.detail(id)).properties;
  return {
    pair: JSON.parse(props.find(p => p.name === "PsiAmpSelection")!.value) as { current: number; alternate: number | null },
    menu: props.find(p => p.name === "PsiCarousel"),
  };
}
for (const hand of ["left", "right"] as const) {
  test(`psi carousel previews, confirms and swaps only the ${hand} amp`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const [amp] = await game.entities.byTemplate(-247);
    await aimVrHandAt(game, amp.position, 0.3, 1, 0, { hand });
    await game.step({ frames: 8 });
    const initial = await state(game, amp.id);
    const button = hand === "left" ? "LeftHandUpperButton" : "RightHandUpperButton";
    const stick = `${hand}_hand.thumbstick`;
    const trigger = `${hand}_hand.trigger`;
    const psi = (await game.info()).player.psi_points;
    await game.input.hold(button);
    await game.step({ frames: 20 });
    assert.ok(!(await state(game, amp.id)).menu, "short hold does not open");
    await game.step({ frames: 15 });
    assert.ok((await state(game, amp.id)).menu, "long hold opens");
    await game.input.release(button);
    await game.step({ frames: 2 });
    assert.ok((await state(game, amp.id)).menu, "opening release cannot close");
    const position = (await game.info()).player.position;
    await game.input.set(stick, [1, 0]);
    await game.step({ frames: 3 });
    const preview = (await state(game, amp.id)).menu!.value;
    await game.step({ frames: 25 });
    assert.equal((await state(game, amp.id)).menu!.value, preview, "held stick steps once");
    assert.deepEqual((await state(game, amp.id)).pair, initial.pair, "browsing does not overwrite either power");
    const moved = (await game.info()).player.position;
    assert.ok(Math.hypot(moved[0]-position[0], moved[2]-position[2]) < 0.02, "navigation does not walk or turn");
    await game.input.set(stick, [0, 0]);
    await game.input.hold(button);
    await game.step({ frames: 3 });
    await game.input.set(trigger, 1);
    await game.step({ frames: 3 });
    const selected = await state(game, amp.id);
    assert.ok(!selected.menu);
    assert.notEqual(selected.pair.current, initial.pair.current);
    assert.equal(selected.pair.alternate, initial.pair.current);
    await game.input.release(button);
    await game.step({frames: 2});
    assert.deepEqual((await state(game, amp.id)).pair, selected.pair, "overlapping B release cannot swap back after trigger confirmation");
    await game.step({ frames: 45 });
    assert.equal((await game.info()).player.psi_points, psi, "confirming trigger never casts, including while held");
    await game.input.set(trigger, 0);
    await game.step({ frames: 2 });
    await game.input.trigger(button);
    await game.step({ frames: 2 });
    assert.deepEqual((await state(game, amp.id)).pair, { current: initial.pair.current, alternate: selected.pair.current });
    // Putting the amp down cancels browsing without a commit.
    await game.input.hold(button);
    await game.step({ frames: 35 });
    await game.input.release(button);
    await game.step({ frames: 2 });
    await game.input.set(stick, [0, 1]);
    await game.step({ frames: 2 });
    await game.input.set(stick, [0, 0]);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 5 });
    const dropped = await state(game, amp.id);
    assert.ok(!dropped.menu);
    assert.equal(dropped.pair.current, initial.pair.current);
  });
}

test("flat carousel uses the same preview and confirmation canvas", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  const [amp] = await game.entities.byTemplate(-247);
  const before = await state(game, amp.id);
  await game.input.hold("LeftHandUpperButton");
  await game.step({ frames: 35 });
  await game.input.release("LeftHandUpperButton");
  await game.step({ frames: 3 });
  assert.ok((await state(game, amp.id)).menu);
  await game.input.set("right_hand.thumbstick", [1, 0]);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  assert.ok(!(await state(game, amp.id)).menu);
  assert.equal((await state(game, amp.id)).pair.alternate, before.pair.current);
});


test("two amps retain independent pairs across a level transition and save/load", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [a] = await game.entities.byTemplate(-247);
  await aimVrHandAt(game, a.position, 0.3, 1);
  await game.step({ frames: 8 });
  await game.input.trigger("CyclePsiPower");
  await game.step({ frames: 2 });
  let firstPair = (await state(game, a.id)).pair;
  const b = await game.player.spawnItem(-247);
  await game.input.set("right_hand.rotation", [0.5, 0, 0, 0.866]);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 10 });
  const ui = await game.ui.state();
  const slot = ui.strip!.elements.find(e => e.entity_id === b.entity_id)!;
  assert.ok(slot);
  await aimVrHandAtCanvas(game, ui.panel_pose!, [slot.rect[0]+slot.rect[2]/2, slot.rect[1]+slot.rect[3]/2], {hand: "left", squeeze: 1});
  await game.step({frames: 5});
  await game.input.trigger("ToggleUseMode");
  await game.step({frames: 8});
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === b.entity_id && i.location === "left_hand"));
  // Confirm the right menu, keeping its trigger held as the left menu opens.
  await game.input.hold("RightHandUpperButton");
  await game.step({frames: 35});
  await game.input.release("RightHandUpperButton");
  await game.step({frames: 2});
  await game.input.set("right_hand.trigger", 1);
  await game.step({frames: 2});
  await game.input.hold("LeftHandUpperButton");
  await game.step({frames: 35});
  await game.input.release("LeftHandUpperButton");
  await game.step({frames: 3});
  await game.input.trigger("RightHandUpperButton");
  await game.step({frames: 2});
  assert.ok((await state(game, b.entity_id)).menu, "other amp swapping does not close this menu");
  const swappedRight = (await state(game, a.id)).pair;
  assert.equal(swappedRight.current, firstPair.alternate);
  firstPair = swappedRight;
  await game.input.set("right_hand.trigger", 0);
  await game.step({frames: 2});
  const beforeCast = (await game.info()).player.psi_points;
  await game.input.set("right_hand.trigger", 1);
  await game.step({frames: 10});
  // Overloadable powers now cast on trigger release in VR too.
  await game.input.set("right_hand.trigger", 0);
  await game.step({frames: 2});
  const afterCast = (await game.info()).player.psi_points;
  assert.ok(beforeCast !== null && afterCast !== null, "psi readings are available");
  assert.ok(afterCast < beforeCast, "right trigger rearms and casts while the left menu remains open");
  assert.ok((await state(game, b.entity_id)).menu);
  await game.input.set("right_hand.trigger", 0);
  await game.step({frames: 2});
  await game.input.set("left_hand.thumbstick", [0,1]);
  await game.step({frames: 2});
  await game.input.set("left_hand.thumbstick", [0,0]);
  await game.input.trigger("LeftHandUpperButton");
  await game.step({frames: 2});
  const secondPair = (await state(game, b.entity_id)).pair;
  assert.notDeepEqual(secondPair, firstPair);
  assert.deepEqual((await state(game, a.id)).pair, firstPair, "left selection never changes right amp");
  await game.transitionLevel("earth.mis");
  await game.step({frames: 90});
  const saved = `e2e_amp_pair_${Date.now()}`;
  await game.save(saved);
  await game.load(saved);
  await game.step({frames: 3});
  const amps = await game.entities.byTemplate(-247);
  const pairs = await Promise.all(amps.map(async amp => (await state(game, amp.id)).pair));
  assert.ok(pairs.some(p => JSON.stringify(p) === JSON.stringify(firstPair)), "right amp pair survives transition and save/load");
  assert.ok(pairs.some(p => JSON.stringify(p) === JSON.stringify(secondPair)), "left amp pair survives transition and save/load");
});


test("opening the selector cancels an existing overload and pause cancels browsing", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({frames: 30});
  const [amp] = await game.entities.byTemplate(-247);
  const psi = (await game.info()).player.psi_points;
  await game.input.set("right_hand.trigger", 1);
  await game.step({frames: 10});
  assert.equal((await game.info()).player.psi_charge_phase, "charging");
  await game.input.hold("LeftHandUpperButton");
  await game.step({frames: 35});
  await game.input.release("LeftHandUpperButton");
  await game.step({frames: 15});
  assert.ok((await state(game, amp.id)).menu, "held trigger on entry cannot confirm");
  assert.equal((await game.info()).player.psi_charge_phase, null);
  assert.equal((await game.info()).player.psi_points, psi, "opening does not release a charged cast");
  await game.input.set("right_hand.trigger", 0);
  await game.step({frames: 2});
  await game.input.trigger("TogglePauseMenu");
  await game.step({frames: 2});
  assert.ok(!(await state(game, amp.id)).menu, "system overlay cancels the transient selector");
});
