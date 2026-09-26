import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("live audio loops retire on scene changes and out-of-radius movement", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "main_menu", port: 0 });
  await game.step({ frames: 10 });
  const menu = (await game.audio.loops()).loops;
  assert.equal(menu.length, 1);
  assert.match(menu[0].sample.toLowerCase(), /mloop1/);
  assert.equal(menu[0].owner, "scene");
  assert.equal(menu[0].entity_id, null);
  assert.ok(menu[0].elapsed_secs >= 0);

  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 2 });
  assert.ok(!(await game.audio.loops()).loops.some(loop => loop.handle === menu[0].handle));

  // Discover the authored cryo hum by stable mission object ID, never runtime ID.
  const [cryo] = await game.entities.byTemplate(349);
  assert.ok(cryo);
  await game.player.teleport({ x: -13.824831, y: -6.240317, z: -65.7749 });
  await game.step({ frames: 2 });
  const near = (await game.audio.loops()).loops;
  const emitter = near.find(loop => loop.entity_id === cryo.id);
  assert.ok(emitter, JSON.stringify(near));
  assert.equal(emitter.owner, "ambient_emitter");
  assert.ok(emitter.sample.length > 0);
  await game.step({ frames: 120 });
  assert.ok((await game.audio.loops()).loops.some(loop => loop.handle === emitter.handle));

  await game.player.teleport({ x: 1000, y: 1000, z: 1000 });
  await game.step({ frames: 2 });
  assert.deepEqual((await game.audio.loops()).loops, [], "no beds outside every authored radius");

  // Two non-overlapping parts of the authored environmental spheres (929/935).
  await game.player.teleport({ x: -8, y: -0.244, z: -54.334 });
  await game.step({ frames: 2 });
  const first = (await game.audio.loops()).loops.find(loop => loop.owner === "environmental");
  assert.ok(first, "ship hum region should have an environmental bed");
  await game.player.teleport({ x: 11, y: -0.244, z: -54.334 });
  await game.step({ frames: 2 });
  const swapped = (await game.audio.loops()).loops.filter(loop => loop.owner === "environmental");
  assert.equal(swapped.length, 1);
  assert.notEqual(swapped[0].handle, first.handle);
  assert.notEqual(swapped[0].sample, first.sample);

  await game.player.teleport({ x: 1000, y: 1000, z: 1000 });
  await game.step({ frames: 2 });
  assert.deepEqual((await game.audio.loops()).loops, [], "leaving an environmental region stops its bed");
  await game.player.teleport({ x: 11, y: -0.244, z: -54.334 });
  await game.step({ frames: 2 });
  assert.ok((await game.audio.loops()).loops.some(loop => loop.owner === "environmental"),
    "re-entering the same cue restarts the stopped bed");
  await game.input.trigger("TogglePauseMenu");
  await game.step({ frames: 10 });
  // SIMR.BIN row 4: Quit to Main Menu.
  await game.input.set("pointer.position", [489.5 / 640, 426 / 480]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 10 });
  assert.equal((await game.info()).mission, "main_menu");
  const final = (await game.audio.loops()).loops;
  assert.equal(final.length, 1, JSON.stringify(final));
  assert.equal(final[0].owner, "scene", "scene replacement retires all mission beds");
});
