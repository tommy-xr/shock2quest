import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const [mission, template, name] of [
  ["eng1.mis", 728, "OG-Shotgun"],
  ["ops2.mis", 66, "OG-Grenade"],
] as const) {
  test(`${name} gives ground when approached along a clear walkway`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission });
    const [hybrid] = await game.entities.byTemplate(template);
    assert.ok(hybrid?.name.includes(name));
    const [x, y, z] = hybrid.position;
    await game.player.teleport({ x: x! - 1.1, y: y!, z: z! });
    // Controlled aggro isolates combat positioning from peripheral vision.
    await game.entities.sendMessage(hybrid.id, { type: "Damage", amount: 1 });
    let backoffs = 0;
    let greatestDistance = 0;
    for (let i = 0; i < 24; i++) {
      await game.step({ frames: 30 });
      const actor = await game.entities.detail(hybrid.id);
      const player = await game.player.position();
      greatestDistance = Math.max(greatestDistance,
        Math.hypot(actor.position[0]! - player.x, actor.position[2]! - player.z));
      if (actor.properties.find(p => p.name === "AIBehavior")?.value.includes("BackOff")) backoffs++;
    }
    assert.ok(backoffs > 0, "must select an authored reverse locomotion clip");
    assert.ok(greatestDistance > 3, `must gain separation, reached ${greatestDistance}`);
  });
}
