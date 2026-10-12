import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

// ND-apparitions.dml applies globally to Apparitions; Grassi's authored watch
// drives ApparBegin through the real AI sequence, without injecting a signal.
for (const vr of [false, true]) {
  test(`Grassi speech starts at the ghost (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [],
    });
    await game.step({ frames: 5 });
    const [ghost] = await game.entities.byTemplate(1730);
    assert.ok(ghost);
    const initial = await game.entities.detail(ghost.id);
    assert.equal(initial.properties.find(p => p.name === "HasRefs")?.value, "false");
    assert.equal(initial.properties.find(p => p.name === "Scripts")?.value, "Apparition");
    await game.player.teleport({ x: 31.25, y: -0.88, z: -34.58 });
    await game.step({ frames: 10 });
    const sounds = (await game.audio.recent({ sample: "cs2201" })).sounds
      .filter(s => s.source_entity?.template_id === 1730);
    assert.ok(sounds.length > 0, "authored watch must play Grassi's speech");
    const current = await game.entities.detail(ghost.id);
    assert.equal(current.properties.find(p => p.name === "HasRefs")?.value, "true");
    for (const sound of sounds) {
      assert.equal(sound.pan_applied, false, "3D source positioning must supersede schema pan");
      assert.ok(Math.hypot(...sound.position.map((v, i) => v - initial.position[i])) < 0.01,
        `speech emitter ${sound.position} must be at Grassi ${initial.position}`);
    }
    console.log(JSON.stringify({ vr, ghost: initial.position, sounds }));
    // The normal sequence still ends and hides the apparition.
    await game.step({ frames: 29 * 60 });
    assert.equal((await game.entities.detail(ghost.id)).properties
      .find(p => p.name === "HasRefs")?.value, "false");
  });
}
