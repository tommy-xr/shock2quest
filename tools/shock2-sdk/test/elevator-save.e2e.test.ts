import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("Cargo lift preserves parked and mid-travel dispatch across save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "eng2.mis", port: 0 });
  // Wait beside the cargo lift, away from the hostile mission-entry encounter.
  await game.player.teleport({ x: 44, y: -11, z: -161 });
  async function object(missionId: number) {
    const matches = await game.entities.byTemplate(missionId);
    assert.equal(matches.length, 1, `mission object ${missionId}`);
    return matches[0];
  }
  async function height() {
    return (await game.entities.detail((await object(669)).id)).position[1];
  }
  async function button(missionId: number) {
    await game.entities.sendMessage((await object(missionId)).id, { type: "Frob" });
  }
  async function roundTrip(label: string) {
    const name = `elevator_${label}_${Date.now()}`;
    const before = await height();
    await game.save(name);
    await game.load(name);
    // Rediscover the platform after load: runtime entity IDs may change.
    assert.ok(Math.abs(await height() - before) < 0.05, "load preserves position");
  }
  await game.step({ frames: 5 });
  await button(480);
  await game.step({ frames: 600 });
  assert.ok(Math.abs(await height() + 5.1) < 0.1, "bottom button reaches middle");
  await button(481);
  await game.step({ frames: 600 });
  assert.ok(Math.abs(await height() - 2.1) < 0.1, "middle button reaches top");

  await roundTrip("top");
  await button(620);
  await game.step({ frames: 60 });
  const moving = await height();
  assert.ok(moving < 2 && moving > -12.5, `platform is mid-travel: ${moving}`);
  await roundTrip("moving");
  await game.step({ frames: 600 });
  assert.ok(Math.abs(await height() + 12.6) < 0.1, "restored leg reaches bottom");

  await roundTrip("bottom");
  await button(480);
  await game.step({ frames: 600 });
  assert.ok(Math.abs(await height() + 5.1) < 0.1, "restored bottom cycles to middle");
  await roundTrip("middle");
  await button(481);
  await game.step({ frames: 600 });
  assert.ok(Math.abs(await height() - 2.1) < 0.1, "restored middle cycles to top");
});
