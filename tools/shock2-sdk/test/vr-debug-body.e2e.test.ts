import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const skip = process.env.SHOCK2_E2E !== "1";

for (const [mission, vr] of [["debug_weapons", true], ["debug_weapons", false], ["medsci1.mis", true]] as const) {
  test(`hacker body is opt-in and spectator-only (${mission}, ${vr ? "VR" : "flat"})`, { skip, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission, debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 5 });
    const bodies = () => game.scene.fromSource("vr_debug_body");
    const backpacks = () => game.scene.fromSource("vr_debug_backpack");
    assert.equal((await bodies()).length, 0);
    assert.equal((await backpacks()).length, 0);
    await game.devParams.set("vr_debug_body", 1);
    await game.step({ frames: 1 });
    assert.equal((await bodies()).length, 0, "enabling the option must not show a first-person body");
    await game.camera.set({ position: [0, 2, -5], lookAt: [0, 1, 0] });
    await game.step({ frames: 1 });
    assert.equal((await bodies()).length > 0, vr, "only the VR spectator gets a body");
    assert.equal((await backpacks()).length > 0, vr, "backpack shares the spectator body gate");
    if (vr) {
      assert.ok((await bodies())[0].lighting?.ambient.some(value => value > 0),
        "body receives player lighting even in an unlit synthetic scene");
      const before = (await bodies())[0].position;
      await game.camera.set({ position: [4, 2, -3], lookAt: [0, 1, 0] });
      await game.step({ frames: 1 });
      const after = (await bodies())[0].position;
      assert.ok(Math.hypot(...after.map((v, i) => v - before[i])) < 1e-3, "body stays with the pawn when camera moves (allow physics settling)");
      await game.devParams.set("vr_debug_body", 0);
      await game.step({ frames: 1 });
      assert.equal((await bodies()).length, 0);
      await game.devParams.set("vr_debug_body", 1);
    }
    await game.camera.attach();
    await game.step({ frames: 1 });
    assert.equal((await bodies()).length, 0, "reattaching always hides the body");
    assert.equal((await backpacks()).length, 0, "reattaching also hides the backpack");
  });
}
