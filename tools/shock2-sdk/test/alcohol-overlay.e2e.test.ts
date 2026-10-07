import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`alcohol vital drives a temporary peripheral overlay (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
      debugFlags: vr ? ["--vr"] : [],
    });
    await game.step({ frames: 3 });
    const vital = async () => (await game.info()).player;
    const overlay = () => game.scene.fromSource("alcohol_overlay");
    async function drink(template = -967) {
      const item = await game.player.spawnItem(template);
      await game.entities.sendMessage(item.entity_id, { type: "Frob" });
      await game.step({ frames: 1 });
    }
    assert.equal((await vital()).alcohol_level, 0);
    assert.equal((await overlay()).length, 0);
    await drink();
    await drink();
    assert.ok((await vital()).alcohol_level > 1.9);
    assert.equal((await overlay()).length, 0, "two drinks remain below the visual threshold");
    await drink();
    await game.step({ frames: 120 });
    const drunk = await vital();
    assert.ok(drunk.alcohol_level > 2.9 && drunk.alcohol_level < 3);
    assert.ok(drunk.alcohol_intensity > 0 && drunk.alcohol_intensity < 1);
    const layers = await overlay();
    assert.ok(layers.length > 0, "renderer actually submits the overlay");
    assert.ok(layers.every(layer => !layer.depth_write), "translucent overlay must not write depth");

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 1 });
    const paused = await vital();
    await game.step({ frames: 120 });
    assert.equal((await vital()).alcohol_level, paused.alcohol_level);
    assert.equal((await vital()).alcohol_intensity, paused.alcohol_intensity);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 1 });

    // Non-alcoholic drinks must not add to the vital.
    const beforeJuice = (await vital()).alcohol_level;
    await drink(-966);
    assert.ok((await vital()).alcohol_level <= beforeJuice);

    // Recovery happens in gameplay time; the safe Earth spawn stays stationary.
    await game.step({ frames: 60 * 150 });
    assert.equal((await vital()).alcohol_level, 0);
    assert.equal((await vital()).alcohol_intensity, 0);
    assert.equal((await overlay()).length, 0);

    await drink(); await drink(); await drink();
    await game.step({ frames: 60 });
    const beforeSave = await vital();
    const name = `alcohol-overlay-${vr ? "vr" : "flat"}`;
    assert.equal((await game.save(name)).success, true);
    assert.equal((await game.load(name)).success, true);
    const loaded = await vital();
    assert.ok(Math.abs(loaded.alcohol_level - beforeSave.alcohol_level) < 0.01);
    assert.ok(Math.abs(loaded.alcohol_intensity - beforeSave.alcohol_intensity) < 0.02);
    await game.transitionLevel("medsci1.mis");
    assert.ok(Math.abs((await vital()).alcohol_level - loaded.alcohol_level) < 0.01);
    await game.step({ frames: 2 });
    assert.ok((await overlay()).length > 0, "normal level transitions preserve intoxication");
  });
}
