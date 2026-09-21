import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { switchCourtLights } from "./helpers/rec1-lights.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`rec1: switched room lights also shade objects (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({
        mission: "rec1.mis",
        experimental: ["object_lighting"],
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 5 });
      const [button] = await game.entities.byTemplate(77);
      assert.ok(button, "authored auxiliary-light switch");
      await game.player.teleport({ x: -2, y: 0.5, z: -213 });
      await game.camera.set({ position: [-2, 2, -213], lookAt: [2, 0.5, -213] });
      await game.step({ frames: 2 });

      // This stationary corpse is beside the authored court lights. Discover
      // its runtime identity again after loading: only mission id 466 is stable.
      async function corpseLighting() {
        const [corpse] = await game.entities.byTemplate(466);
        assert.ok(corpse, "authored court corpse");
        const { objects } = await game.scene.objects({ entityId: corpse.id });
        const lighting = objects.find(o => o.model !== null)?.lighting;
        assert.ok(lighting, "object lighting must reach the rendered corpse mesh");
        return lighting;
      }

      await switchCourtLights(game, "TurnOn");
      const firstOn = await corpseLighting();
      await switchCourtLights(game, "TurnOff");
      const off = await corpseLighting();
      const saveName = `object-lighting-off-${vr ? "vr" : "flat"}`;
      assert.equal((await game.save(saveName)).success, true);
      await switchCourtLights(game, "TurnOn");
      const on = await corpseLighting();
      assert.ok(Math.abs(on.received - firstOn.received) < 0.0001,
        "on/off/on must recover full brightness without cumulative scaling");
      assert.ok(on.received > off.received + 0.01,
        `restoring court lights must light the corpse: off=${off.received}, on=${on.received}`);
      assert.deepEqual(on.ambient, off.ambient, "switching a lamp must not change mission ambient");

      // The shared level-light control scales authored direct light exactly
      // once. It must not change the independently controlled ambient floor.
      for (const gain of [0.5, 0, 2]) {
        await game.devParams.set("level_light_intensity", gain);
        await game.step({ frames: 1 });
        const scaled = await corpseLighting();
        assert.ok(Math.abs(scaled.received - on.received * gain) < 0.0001,
          `level intensity ${gain} must scale authored object light once`);
        assert.deepEqual(scaled.ambient, on.ambient);
      }
      await game.devParams.reset("level_light_intensity");
      await game.step({ frames: 1 });
      assert.ok(Math.abs((await corpseLighting()).received - on.received) < 0.0001,
        "reset restores authored brightness");

      assert.equal((await game.load(saveName)).success, true);
      await game.camera.set({ position: [-2, 2, -213], lookAt: [2, 0.5, -213] });
      await game.step({ frames: 2 });
      const restored = await corpseLighting();
      assert.ok(Math.abs(restored.received - off.received) < 0.0001,
        "loading the off state must restore object shading as well as wall lightmaps");
      assert.equal(restored.light_count, off.light_count);
      await switchCourtLights(game, "TurnOn");
      assert.ok(Math.abs((await corpseLighting()).received - on.received) < 0.0001,
        "an off/on cycle after loading must recover full brightness without cumulative scaling");
    });
}

for (const [mission, experimental] of [
  ["medsci1.mis", []],
  ["debug_minimal", ["object_lighting"]],
] as const) {
  test(`object lighting leaves the default/no-world-rep path unchanged: ${mission}`,
    { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission, experimental: [...experimental] });
      await game.step({ frames: 5 });
      const { objects } = await game.scene.objects();
      assert.ok(objects.length > 0);
      assert.ok(objects.every(object => object.lighting == null));
    });
}
