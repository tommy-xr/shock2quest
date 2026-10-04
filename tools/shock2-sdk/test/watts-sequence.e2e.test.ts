import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`Watts performs his scene and triggers the ambush without watchdog delays (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 120 });
    const [watts] = await game.entities.byTemplate(734);
    const [hybrid] = await game.entities.byTemplate(1624);
    assert.ok(watts && hybrid);
    assert.equal(await game.entities.animation(watts.id), null,
      "Watts keeps his authored static pose until the watch triggers");
    const start = (await game.entities.detail(hybrid.id)).position;
    const spawn = (await game.info()).player.position;
    await game.player.teleport({ x: -13.42, y: -5.04, z: 29.52 });
    await game.step({ frames: 2 });
    await game.player.teleport({ x: spawn[0], y: spawn[1], z: spawn[2] });
    let performed = false, ambush = false;
    for (let second = 0; second < 20; second++) {
      await game.step({ frames: 60 });
      const animation = await game.entities.animation(watts.id);
      performed ||= animation?.clip === "humdthtk";
      const current = (await game.entities.detail(hybrid.id)).position;
      ambush ||= Math.hypot(...current.map((v, i) => v - start[i])) > 10;
      if (performed && ambush) break;
    }
    assert.ok(performed, "Watts plays the authored cs43 performance");
    assert.ok(ambush, "the final Frob triggers the ambush before any 30-second watchdog");
    await game.step({ frames: 1200 });
    const resting = await game.entities.animation(watts.id);
    assert.equal(resting?.clip, null, "Watts does not stand up into an idle animation after dying");
    assert.equal(resting?.last_clip, "humdthtk", "hold the performance's terminal pose");
    const speech = await game.audio.recent({ sample: "cs0601" });
    assert.ok(speech.sounds.length > 0, "the opening Frob plays Watts's speech");
  });
}
