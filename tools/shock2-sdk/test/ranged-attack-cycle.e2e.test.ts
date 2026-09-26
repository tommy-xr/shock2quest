import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("a shotgun hybrid repeats shots without a chase clip while the target remains reachable", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "eng1.mis" });
  // Mission object 728 is an ordinary corridor encounter, unlike the
  // off-map staging hybrids in MedSci. Discover its runtime ID on every run.
  const [hybrid] = await game.entities.byTemplate(728);
  assert.ok(hybrid && hybrid.name.includes("OG-Shotgun"));
  const [x, y, z] = hybrid.position;
  await game.player.teleport({ x: x!, y: y!, z: z! + 1.1 });
  const hp = (await game.info()).player.hit_points!;
  let attacks = 0;
  let chases = 0;
  for (let i = 0; i < 40; i++) {
    await game.step({ frames: 30 });
    const actor = await game.entities.detail(hybrid.id);
    // Allow initial perception/alertness to settle before checking the cycle.
    if (i < 10) continue;
    const behavior = actor.properties.find(p => p.name === "AIBehavior")?.value ?? "";
    if (behavior.includes("RangedAttack")) attacks++;
    if (behavior.includes("Chase")) chases++;
  }
  assert.ok(attacks >= 20, `expected sustained firing, got ${attacks} ranged samples`);
  assert.equal(chases, 0, "a completed firing clip must not insert a walk cycle");
  assert.ok((await game.info()).player.hit_points! < hp, "the repeated shots must actually hit");
});
