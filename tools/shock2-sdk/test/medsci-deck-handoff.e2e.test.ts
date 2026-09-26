import assert from "node:assert/strict";
import test from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { vrHandLocal, vrHandLocalDelta } from "./helpers/vr-climb.js";

async function grab(game: GameServer, hand: "left" | "right", world: Vec3) {
  const local = await vrHandLocal(game, world);
  await game.input.set(`${hand}_hand.position`, local);
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 1 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 1 });
  return local;
}

test("medsci1 (VR): replace the ladder hand with a second low deck hold and pull slowly", {
  skip: process.env.SHOCK2_E2E !== "1",
  timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  const rungs = (await game.entities.list({ filter: "Rick Ladder", limit: 100 })).entities;
  // Mission-file identity, not a launch-dependent runtime entity ID.
  const rung = rungs.find(entity => entity.template_id === 1481);
  assert.ok(rung, "the cryo shaft's top rung exists");
  const [x, , z] = rung.position;
  const lipY = -1.6;
  // Stage the reported top-of-ladder posture; all subsequent motion uses
  // squeezed hands. The lip is above the feet but below the normal step gate.
  await game.player.teleport({ x, y: -0.85, z: z + 0.596278 });
  for (const hand of ["left", "right"] as const) {
    await game.input.set(`${hand}_hand.squeeze`, 0);
  }

  await grab(game, "right", [x, -1.8, z + 0.146278]);
  await grab(game, "left", [x - 0.16, lipY + 0.05, z - 0.853722]);
  const supported = (await game.info()).player;
  assert.deepEqual(supported.climb.grips.map(grip => grip.kind).sort(), ["ladder", "ledge"]);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 30 });
  const oneDeck = (await game.info()).player;
  assert.equal(oneDeck.climb.grips[0]?.kind, "ledge");
  assert.ok(Math.abs(oneDeck.position[1] - supported.position[1]) < 0.02, "the squeezed deck hand supports the player after releasing the ladder");

  const secondDeck: Vec3 = [x + 0.14, lipY + 0.05, z - 0.653722];
  assert.equal((await game.physics.grip(secondDeck)).grip, null,
    "the normal grip probe rejects this near-feet deck; the held hand must authorize it");
  const onTop = await grab(game, "right", secondDeck);
  assert.deepEqual((await game.info()).player.climb.grips.map(grip => grip.kind), ["ledge", "ledge"]);
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.climb.anchor_hand, "right");

  const down = await vrHandLocalDelta(game, [0, -0.8, 0]);
  let vaulted = false;
  for (let frame = 1; frame <= 120; frame++) {
    await game.input.set("right_hand.position", onTop.map((value, axis) => value + down[axis] * frame / 120));
    await game.step({ frames: 1 });
    vaulted ||= (await game.info()).player.climb.vaulting;
  }
  assert.ok(vaulted, "a two-second held pull commits the top-out without a release flick");
  await game.step({ frames: 90 });
  const landed = (await game.info()).player;
  assert.equal(landed.climb.vaulting, false);
  assert.ok(Math.abs(landed.position[1] - (lipY + 1.244)) < 0.1,
    `expected to stand on the deck, got ${landed.position}`);
  assert.ok(landed.position[2] < z, "the body clears the ladder onto the landing");
  await game.step({ frames: 60 });
  assert.ok(Math.abs((await game.info()).player.position[1] - landed.position[1]) < 0.05,
    "the completed top-out stays on the deck");
});

test("medsci1 (VR): the cryo bay deck top-out keeps the latest held pull", {
  skip: process.env.SHOCK2_E2E !== "1",
  timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  // This is the OTHER cryo ladder: the fallen duct must be broken before
  // climbing it. Drive normal damage, discovering its runtime ID each launch.
  const duct = (await game.entities.list({ filter: "Air Duct", limit: 100 })).entities
    .find(entity => entity.template_id === 402);
  assert.ok(duct, "the fallen cryo duct exists");
  const damaged = await fetch(`${game.baseUrl}/v1/entities/${duct.id}/message`, {
    method: "POST", body: JSON.stringify({ type: "Damage", amount: 100 }),
  });
  assert.ok(damaged.ok, "damage reaches the duct");
  await game.step({ frames: 30 });
  // Pose recorded on Quest when the player released the ladder hand and
  // slowly pulled with the still-squeezed deck hand. Its 1.12 eye offset is
  // essential: it permits the top-out before the body clears the lip.
  await game.input.set("head.position", [0.031793468, 1.12, 0.17070708]);
  await game.player.teleport({ x: -41.36099, y: -2.201106, z: 17.03226 });
  await grab(game, "right", [-40.968594, -1.6741132, 16.597527]);
  const left = await grab(game, "left", [-41.4958, -1.5342857, 16.272123]);
  assert.deepEqual((await game.info()).player.climb.grips.map(grip => grip.kind).sort(), ["ladder", "ledge"]);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 1 });
  const heldY = (await game.info()).player.position[1];
  assert.equal((await game.info()).player.climb.anchor_hand, "left");

  const down = await vrHandLocalDelta(game, [0.0289, -0.3, 0.05747]);
  let vaulted = false;
  for (let frame = 1; frame <= 180; frame++) {
    await game.input.set("left_hand.position", left.map((value, axis) => value + down[axis] * Math.min(frame, 30) / 30));
    await game.step({ frames: 1 });
    const player = (await game.info()).player;
    vaulted ||= player.climb.vaulting;
    assert.ok(player.position[1] >= heldY - 0.05,
      `the squeezed deck hold must not drop after top-out starts: ${player.position}`);
  }
  assert.ok(vaulted, "the slow held pull starts a top-out");
  const landed = (await game.info()).player;
  assert.equal(landed.climb.vaulting, false);
  assert.ok(Math.abs(landed.position[1] - -0.356) < 0.1, `stand on the deck: ${landed.position}`);
  assert.ok(landed.position[2] < 16.3, "the body clears the lip");
  await game.step({ frames: 60 });
  assert.ok(Math.abs((await game.info()).player.position[1] - landed.position[1]) < 0.05);
});
