// Using a medsci1 info terminal unlocks the help topic in its `P$HelpText`
// mask. Negative-first: the InfoComputer script did nothing, so no topic was
// ever unlocked.
import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`info terminal unlocks its help topic (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 10 });
    // Mission object 178 carries P$HelpText = 1: topic 0.
    const { entities } = await game.entities.list({ filter: "Info Comp", limit: 50 });
    assert.equal(entities.length, 17, "medsci1's info terminals");
    const terminal = entities.find((e) => e.template_id === 178);
    assert.ok(terminal, "info terminal 178");
    assert.deepEqual((await game.quests.list()).help_topics, []);

    const [x, , z] = terminal.position;
    await game.player.teleport({ x: x + 1.5, y: 0.9, z });
    await game.step({ frames: 10 });
    const aim = await game.player.aimAt(terminal.id, { visibility: "required" });
    assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
    if (vr) await aimVrHandAt(game, aim.world_point, 0.35);
    const use = vr ? "right_hand.trigger" : "right_hand.squeeze";
    await game.input.set(use, 1);
    await game.step({ frames: 2 });
    await game.input.set(use, 0);
    await game.step({ frames: 4 });

    assert.deepEqual((await game.quests.list()).help_topics, [0]);
  });
}
