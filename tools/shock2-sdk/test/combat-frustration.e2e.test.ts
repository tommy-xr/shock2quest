import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("an unreachable target causes a bounded gesture and a combat-mode cooldown", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "eng1.mis" });
  await game.step({ frames: 5 });
  const [hybrid] = await game.entities.byTemplate(728);
  assert.ok(hybrid?.name.includes("OG-Shotgun"));
  // The player is on the lower floor, separated from the balcony by solid
  // geometry. Pin awareness so this tests combat failure, not forgetting.
  await game.player.teleport({ x: 41.3, y: -20, z: -18.5 });
  await game.input.trigger("DebugForceChase");
  const episodes: number[] = [];
  let active = false;
  let longestHold = 0;
  let hold = 0;
  for (let i = 0; i < 80; i++) {
    await game.step({ frames: 15 });
    const actor = await game.entities.detail(hybrid.id);
    const frustrated = actor.properties.find(p => p.name === "AIBehavior")?.value.includes("Frustration") ?? false;
    if (frustrated && !active) episodes.push((i + 1) / 4);
    hold = frustrated ? hold + 0.25 : 0;
    longestHold = Math.max(longestHold, hold);
    active = frustrated;
  }
  assert.ok(episodes.length > 0, "a stalled combatant must express frustration");
  assert.ok(longestHold >= 1 && longestHold <= 2.5,
    `gesture should persist briefly, observed ${longestHold}s`);
  // Exercise the BaseMonster wrapper's nested state dispatch, not only the
  // child AI serializer: every armed monster in the mission uses this path.
  const save = `combat_frustration_e2e_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  const [restored] = await game.entities.byTemplate(728);
  assert.ok(restored);
  await game.step({ frames: 30 });
  assert.ok(!(await game.entities.detail(restored.id)).properties
    .find(p => p.name === "AIBehavior")?.value.includes("Frustration"),
    "loading a cooldown must not replay its completed gesture");
  for (let i = 1; i < episodes.length; i++) {
    assert.ok(episodes[i]! - episodes[i - 1]! >= 9.5,
      `must not repeat during lockout: ${episodes}`);
  }
});
