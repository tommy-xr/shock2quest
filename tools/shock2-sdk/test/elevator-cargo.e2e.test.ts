import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { teleportVerified } from "./helpers/teleport.js";

async function ride(game: GameServer, floor: RegExp) {
  const [button] = (await game.entities.list({ filter: "Master Elevator Button" })).entities;
  assert.ok(button);
  const [x, y, z] = button.position;
  await teleportVerified(game, { x: x + 1, y: y + 0.5, z: z + 1 });
  await game.entities.sendMessage(button.id, { type: "Frob" });
  await game.step({ frames: 2 });
  const target = (await game.ui.state()).active_panel?.elements.find(
    e => e.kind === "button" && floor.test(e.label ?? ""),
  );
  assert.ok(target, `missing floor ${floor}`);
  await clickUiElement(game, target);
}

async function dropItem(game: GameServer, name: string) {
  await game.input.set("head.look", [0, 70]);
  const item = await game.player.spawnItem(name);
  if ((await game.ui.state()).mode !== "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
  }
  const button = (await game.ui.state()).strip?.elements.find(e => e.label === name);
  assert.ok(button);
  await clickUiElement(game, button);
  assert.equal((await game.ui.state()).cursor?.entity_id, item.entity_id);
  await game.input.set("pointer.position", [0.5, 0.6]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 120 });
  assert.ok(!(await game.player.inventory()).items.some(e => e.entity_id === item.entity_id));
  return game.entities.detail(item.entity_id);
}

// Each deck's elevator marker (StartLoc 22 with LandingPoint links), by
// mission object id. Cargo keeps its offset from this marker between decks.
const ELEVATOR_MARKER: Record<string, number> = {
  "medsci1.mis": 169,
  "eng1.mis": 464,
  "hydro2.mis": 1167,
  "ops2.mis": 560,
  "rec1.mis": 273,
};

async function cargoOffset(game: GameServer, template: number) {
  const { mission } = await game.info();
  const [marker] = await game.entities.byTemplate(ELEVATOR_MARKER[mission]);
  const cargo = await game.entities.byTemplate(template);
  assert.ok(marker, `missing elevator marker on ${mission}`);
  assert.equal(cargo.length, 1, `cargo must exist exactly once on ${mission}`);
  return cargo[0].position.map((v, i) => v - marker.position[i]);
}

// The arrival point sits ~1.2 units from the marker, so cargo that fell back
// there fails this; the vertical slack covers the arrival lift and settling.
function assertKeptOffset(before: number[], after: number[], mission: string) {
  const horizontal = Math.hypot(after[0] - before[0], after[2] - before[2]);
  assert.ok(horizontal < 0.5 && Math.abs(after[1] - before[1]) < 0.5,
    `cargo must keep its elevator offset on ${mission}: ${before} -> ${after}`);
}

// Negative-first: the unmodified runtime arrives in eng1 with zero Wrenches
// of the provisioned archetype. Ordinary travel leaves it in medsci1.
// IDs of provisioned archetypes are stable; runtime IDs are rediscovered after
// every load. Mission-authored objects have positive, mission-local identities.
test("elevator transports loose cargo without duplicating it; other levels persist through saves", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  const save = `e2e-elevator-cargo-${Date.now()}`;
  let cargoTemplate: number;
  let stashTemplate: number;
  let stashPosition: number[];
  {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 2 });
    await game.quests.set("ElevState", "complete");
    // Leave an unrelated item in the starting room, outside the elevator.
    const stash = await dropItem(game, "Psi Amp");
    stashTemplate = stash.template_id;
    stashPosition = stash.position;
    // Mission-file marker 169 is both DestLoc 22 and StartLoc 22.
    const [marker] = await game.entities.byTemplate(169);
    assert.ok(marker);
    const start = await game.player.position();
    // Drop from the cabin's middle (its landing point), clear of the wall panel.
    const [landing] = await game.entities.byTemplate(106);
    assert.ok(landing);
    await teleportVerified(game, { x: landing.position[0], y: landing.position[1], z: landing.position[2] });
    const dropped = await dropItem(game, "Wrench");
    cargoTemplate = dropped.template_id;
    // A drop at the feet can land on the player's capsule; step away so it settles.
    await teleportVerified(game, start);
    await game.step({ frames: 120 });
    stashPosition = (await game.entities.detail(stash.entity_id)).position;
    const departed = await cargoOffset(game, cargoTemplate);
    // Cabin walls differ above the floor; a ledge drop would rightly fall back.
    assert.ok(departed[1] < -1.2, `the wrench must rest on the cabin floor: ${departed}`);
    await ride(game, /engineering/i);
    assert.equal((await game.info()).mission, "eng1.mis");
    assertKeptOffset(departed, await cargoOffset(game, cargoTemplate), "eng1.mis");
    assert.equal((await game.entities.byTemplate(stashTemplate)).length, 0, "other rooms must stay behind");
    await game.save(save);
  }
  // A new process proves the inactive level snapshot really was written to disk.
  {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.load(save);
    assert.equal((await game.info()).mission, "eng1.mis");
    const departed = await cargoOffset(game, cargoTemplate);
    await ride(game, /med\s*\/?\s*sci/i);
    assert.equal((await game.info()).mission, "medsci1.mis");
    // cargoOffset also asserts one copy: the return trip must not resurrect the departure copy.
    assertKeptOffset(departed, await cargoOffset(game, cargoTemplate), "medsci1.mis");
    const [stash] = await game.entities.byTemplate(stashTemplate);
    assert.ok(stash, "an item left in an inactive level must survive a cold save/load");
    assert.ok(Math.hypot(...stash.position.map((v, i) => v - stashPosition[i])) < 0.1,
      `the item left behind must remain at its saved location: ${stashPosition} -> ${stash.position}`);
    // Ordinary transitions must not implicitly scoop up the elevator's cargo.
    await game.transitionLevel("eng1.mis", 22);
    assert.equal((await game.entities.byTemplate(cargoTemplate)).length, 0);
    await game.transitionLevel("medsci1.mis", 22);
    assert.equal((await game.entities.byTemplate(cargoTemplate)).length, 1);
    for (const [floor, mission] of [
      [/hydroponics/i, "hydro2.mis"],
      [/operations/i, "ops2.mis"],
      [/recreation/i, "rec1.mis"],
      [/med\s*\/?\s*sci/i, "medsci1.mis"],
    ] as const) {
      const departed = await cargoOffset(game, cargoTemplate);
      await ride(game, floor);
      assert.equal((await game.info()).mission, mission);
      assertKeptOffset(departed, await cargoOffset(game, cargoTemplate), mission);
    }
  }
});

test("saved loose pickups do not gain the initial placement lift on every load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 2 });
  const item = await dropItem(game, "Psi Amp");
  const original = item.position;
  const save = `e2e-pickup-position-${Date.now()}`;
  for (let i = 0; i < 3; i++) {
    await game.save(save);
    await game.load(save);
    await game.step({ frames: 2 });
    const [restored] = await game.entities.byTemplate(item.template_id);
    assert.ok(restored);
    assert.ok(Math.hypot(...restored.position.map((v, axis) => v - original[axis])) < 0.1,
      `load ${i + 1} moved the item: ${original} -> ${restored.position}`);
  }
});

test("a wrench dropped beside the elevator button falls to the cabin floor", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 120_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 2 });
  const start = await game.player.position();
  const [marker] = await game.entities.byTemplate(169);
  assert.ok(marker);
  await teleportVerified(game, { x: marker.position[0], y: marker.position[1], z: marker.position[2] });
  const wrench = await dropItem(game, "Wrench");
  await teleportVerified(game, start);
  await game.step({ frames: 120 });
  const settled = await game.entities.detail(wrench.entity_id);
  assert.ok(settled.position[1] - marker.position[1] < -1.2,
    `wrench must reach the floor beside the button: ${settled.position}`);
});

test("an obstructed flat throw keeps the item carried without a physics body", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 120_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 2 });
  const [landing] = await game.entities.byTemplate(106);
  assert.ok(landing);
  const [x, , z] = landing.position;
  // Aim upward while jumping near the low cabin ceiling. Every candidate
  // remains above that ceiling and must be refused rather than spawned through it.
  await game.player.teleport({ x, y: 1.0, z });
  await game.input.set("head.look", [0, -90]);
  const item = await game.player.spawnItem("Wrench");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 2 });
  const button = (await game.ui.state()).strip?.elements.find(e => e.label === "Wrench");
  assert.ok(button);
  await clickUiElement(game, button);
  await game.input.setJump(true);
  await game.step({ frames: 12 });
  await game.input.setJump(false);
  await game.input.set("pointer.position", [0.5, 0.6]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
  assert.ok((await game.player.inventory()).items.some(e => e.entity_id === item.entity_id),
    JSON.stringify({ player: await game.player.position(), item: await game.entities.detail(item.entity_id) }));
  assert.equal((await game.physics.bodies({ entityId: item.entity_id })).bodies.length, 0);
  assert.ok((await game.ui.state()).strip?.elements.some(e => e.label === "Wrench"));
});
