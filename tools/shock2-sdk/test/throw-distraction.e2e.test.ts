import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

// A real VR release, scenery collision, resolved audio sample, and investigating
// hybrid. The player leaves earshot/eyesight before impact; no injected noise or
// direct hit can account for the response.
test("a thrown mug distracts a hybrid toward its audible scenery impact", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({
    mission: "debug_interactions", debugFlags: ["--vr"],
  });
  await game.step({ frames: 60 });
  const mug = (await game.entities.list({ filter: "Mug" })).entities.find(
    entity => entity.template_id === -1221,
  );
  assert.ok(mug);
  await game.input.set("right_hand.position", [-1.2, 0.05, -1.2]);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.right_hand_entity_id, mug.id);

  // Spawn in the open aisle outside the rack, clear of the cup trajectory.
  await game.player.teleport({ x: 0, y: 1.244, z: 4 });
  await game.input.trigger("SpawnDebugMonster");
  await game.step({ frames: 1 });
  const hybrid = (await game.entities.list({ filter: "OG-Pipe" })).entities.find(
    entity => entity.template_id === -397,
  );
  assert.ok(hybrid);
  const pawn = (await game.info()).player.position;
  for (let frame = 1; frame <= 24; frame++) {
    const world = [-1.2 - 0.6 * frame / 24, 1.29 + 0.71 * frame / 24, 2.8 + 1.2 * frame / 24];
    await game.input.set("right_hand.position", world.map((value, axis) => value - pawn[axis]));
    await game.step({ frames: 1 });
  }
  await game.step({ frames: 10 });
  for (let frame = 1; frame <= 6; frame++) {
    await game.input.set("right_hand.position", [-1.8 - pawn[0], 2 - pawn[1], 4 + frame * 0.04 - pawn[2]]);
    await game.step({ frames: 1 });
  }
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 1 });
  const bodies = (await game.physics.bodies({ entityId: mug.id })).bodies;
  assert.equal(bodies.length, 1, "release creates a world body instead of storing the cup");
  assert.ok(bodies[0].velocity[2] > 2.5, "the actual controller swing propels the cup");
  // Hide the test pawn below the solid floor; distance alone cannot rule out
  // later sight along this long open aisle. The released cup stays in the scene.
  await game.player.teleport({ x: -30, y: -5, z: 7 });
  await game.input.trigger("DebugCalmAll");
  await game.step({ frames: 6 });
  const initial = await game.entities.detail(hybrid.id);
  assert.equal(initial.properties.find(p => p.name === "AIAlertness")?.value, "Lowest");
  const initialHp = initial.properties.find(p => p.name === "HitPoints")?.value;
  const soundSequence = Math.max(0, ...(await game.audio.recent()).sounds.map(sound => sound.sequence));
  const messageSequence = Math.max(0, ...(await game.messages.recent()).messages.map(message => message.sequence));
  await game.step({ frames: 90 });
  const collisions = (await game.audio.recent()).sounds.filter(sound =>
    sound.sequence > soundSequence &&
    sound.tags.some(([tag, value]) => tag === "event" && value === "collision") &&
    sound.tags.some(([tag, value]) => tag === "material" && value === "glass"));
  assert.ok(collisions.length > 0, "the cup impact resolves and plays a collision sample");
  const impact = collisions[0].position;
  assert.ok(Math.hypot(impact[0] + 30, impact[2] - 7) > 20, "noise originates at the cup, far from the player");
  const alerted = await game.entities.detail(hybrid.id);
  assert.equal(alerted.properties.find(p => p.name === "AIAlertness")?.value, "Moderate");
  assert.ok(!["MeleeAttack", "RangedAttack"].includes(
    alerted.properties.find(p => p.name === "AIBehavior")?.value ?? ""),
  "hearing a location cannot start an attack without seeing a target");
  assert.equal(alerted.properties.find(p => p.name === "AITargetVisible")?.value, "false",
    "the listener has no line of sight to the player when hearing the impact");
  const known = alerted.properties.find(p => p.name === "AILastKnown")?.value;
  assert.ok(known);
  const target = JSON.parse(known) as number[];
  assert.ok(collisions.some(sound => Math.hypot(...target.map((value, axis) => value - sound.position[axis])) < 0.1),
    `investigation target is an actual cup impact: ${known}`);
  await game.step({ frames: 180 });
  const moved = await game.entities.detail(hybrid.id);
  assert.equal(moved.properties.find(p => p.name === "HitPoints")?.value, initialHp);
  assert.ok(!["MeleeAttack", "RangedAttack"].includes(
    moved.properties.find(p => p.name === "AIBehavior")?.value ?? ""),
  "arrival at the sound location does not attack an imaginary player");
  const messages = (await game.messages.recent()).messages.filter(message =>
    message.sequence > messageSequence && message.to.entity_id === hybrid.id);
  assert.ok(messages.some(message => message.payload === "HeardNoise"));
  assert.ok(!messages.some(message => message.payload === "Damage"), "the throw never strikes the hybrid");
  const distance = (position: number[]) => Math.hypot(position[0] - target[0], position[2] - target[2]);
  assert.ok(distance(moved.position) < distance(initial.position) - 0.5,
    `hybrid approaches the remembered impact: ${initial.position} -> ${moved.position}, target ${target}`);
  assert.equal(moved.properties.find(p => p.name === "AIBehavior")?.value, "Search",
    "arrival changes pursuit into a search of the sound location");
  await game.step({ frames: 60 });
  const scanning = await game.entities.detail(hybrid.id);
  assert.notEqual(scanning.properties.find(p => p.name === "AITargetVisible")?.value, "true",
    "the hidden player never becomes visible during the investigation");
  assert.ok(Math.hypot(scanning.position[0] - moved.position[0], scanning.position[2] - moved.position[2]) < 0.25,
    "searching at the sound location holds position instead of walking past it");
});
