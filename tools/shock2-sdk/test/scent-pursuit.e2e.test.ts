import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const expired of [false, true]) {
  test(`a hybrid ${expired ? "cannot follow an expired" : "follows a recent"} unseen player scent trail`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions" });
    await game.step({ frames: 60 });
    await game.player.teleport({ x: 0, y: 1.244, z: 4 });
    await game.step({ frames: 30 });
    // Lay the trail using ordinary grounded locomotion while no hybrid exists.
    await game.input.set("right_hand.thumbstick", [0, 0.3]);
    const trail: number[][] = [];
    for (let sample = 0; sample < 6; sample++) {
      await game.step({ frames: 30 });
      trail.push((await game.info()).player.position);
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(trail.at(-1)![0] < -8, "the player walked a substantial trail");
    if (expired) {
      await game.player.teleport({ x: -30, y: -5, z: 7 });
      await game.step({ frames: 1260 });
    }
    // Return airborne for the two setup frames, so no fresh scent is deposited
    // back at the start. Damage establishes the ordinary initial awareness cue.
    await game.player.teleport({ x: 0, y: 2, z: 4 });
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 1 });
    const hybrid = (await game.entities.list({ filter: "OG-Pipe" })).entities.find(
      entity => entity.template_id === -397,
    );
    assert.ok(hybrid);
    await game.entities.sendMessage(hybrid.id, { type: "Damage", amount: 1 });
    await game.step({ frames: 1 });
    const alerted = await game.entities.detail(hybrid.id);
    const initialKnown = alerted.properties.find(p => p.name === "AILastKnown")?.value;
    assert.ok(initialKnown);
    assert.ok(Math.abs((JSON.parse(initialKnown) as number[])[0]) < 0.1,
      "the initial damage cue knows only the trail start");
    await game.player.teleport({ x: -30, y: -5, z: 7 });
    const messageSequence = Math.max(0, ...(await game.messages.recent()).messages.map(message => message.sequence));
    const knownPositions: number[][] = [];
    let farthestX = 0;
    for (let sample = 0; sample < 100; sample++) {
      await game.step({ frames: 6 });
      const detail = await game.entities.detail(hybrid.id);
      assert.notEqual(detail.properties.find(p => p.name === "AITargetVisible")?.value, "true",
        "the player is hidden below the solid floor for the entire pursuit");
      const known = detail.properties.find(p => p.name === "AILastKnown")?.value;
      if (known) knownPositions.push(JSON.parse(known) as number[]);
      farthestX = Math.min(farthestX, detail.position[0]);
      assert.equal(detail.properties.find(p => p.name === "HitPoints")?.value, "11");
    }
    const messages = (await game.messages.recent()).messages.filter(message =>
      message.sequence > messageSequence && message.to.entity_id === hybrid.id);
    assert.ok(!messages.some(message => ["HeardNoise", "Damage"].includes(message.payload)),
      "no subsequent sound or hit supplies the unseen positions");
    assert.ok(knownPositions.every(position => position[0] > -12 && position[1] > 0),
      "tracking never reveals the player's true hidden position");
    if (expired) {
      assert.ok(knownPositions.every(position => Math.abs(position[0]) < 0.1),
        "expired scent cannot replace the original known location");
    } else {
      assert.ok(knownPositions.some(position => position[0] < -7),
        `local scent advances awareness along the unseen trail: ${JSON.stringify(knownPositions)}`);
      assert.ok(farthestX < -6, "the hybrid physically follows the trail beyond its spawn point");
    }
  });
}
