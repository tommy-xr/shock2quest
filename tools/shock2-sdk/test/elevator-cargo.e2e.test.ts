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
    await teleportVerified(game, { x: marker.position[0], y: marker.position[1] + 1, z: marker.position[2] });
    const dropped = await dropItem(game, "Wrench");
    cargoTemplate = dropped.template_id;
    stashPosition = (await game.entities.detail(stash.entity_id)).position;
    await ride(game, /engineering/i);
    assert.equal((await game.info()).mission, "eng1.mis");
    const carried = await game.entities.byTemplate(cargoTemplate);
    assert.equal(carried.length, 1, "loose elevator cargo must arrive exactly once");
    const player = await game.player.position();
    assert.ok(Math.hypot(carried[0].position[0] - player.x, carried[0].position[1] - player.y,
      carried[0].position[2] - player.z) < 5, "cargo must arrive in the destination elevator");
    assert.equal((await game.entities.byTemplate(stashTemplate)).length, 0, "other rooms must stay behind");
    await game.save(save);
  }
  // A new process proves the inactive level snapshot really was written to disk.
  {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.load(save);
    assert.equal((await game.info()).mission, "eng1.mis");
    assert.equal((await game.entities.byTemplate(cargoTemplate)).length, 1);
    await ride(game, /med\s*\/?\s*sci/i);
    assert.equal((await game.info()).mission, "medsci1.mis");
    assert.equal((await game.entities.byTemplate(cargoTemplate)).length, 1,
      "return trip must not resurrect the departure copy");
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
      await ride(game, floor);
      assert.equal((await game.info()).mission, mission);
      assert.equal((await game.entities.byTemplate(cargoTemplate)).length, 1,
        `cargo must travel exactly once to ${mission}`);
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
