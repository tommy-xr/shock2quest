import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first for #573: a save taken while a security camera is in its
// alert (red) state was permanently unloadable. The camera derived its alert
// models from its CURRENT model, so a save carrying `camred` re-derived
// `camred_red` on load - a model that does not exist - and the load panicked.
// Before the fix, the load below takes down the game-loop thread and this test
// fails (the runtime never comes back with the restored mission).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// medsci1 object 102 - a Security Camera (`cameraalert` script) with a clear
// line of sight to the vantage point used below. Template ids are stable
// across runs; runtime entity ids are not.
const CAMERA_TEMPLATE_ID = 102;

function prop(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

async function camera(game: GameServer): Promise<EntitySummary> {
  const [cam] = await game.entities.byTemplate(CAMERA_TEMPLATE_ID);
  assert.ok(cam, `medsci1 should contain camera template ${CAMERA_TEMPLATE_ID}`);
  return cam;
}

test(
  "a save taken while a camera is alert reloads with the camera's state intact",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    const saveName = `camera_alert_e2e_${Date.now()}`;

    // --- Session 1: let the camera see the player, then save while it is red.
    {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
      });
      await game.step({ frames: 10 });

      const cam = await camera(game);
      assert.equal(
        prop(await game.entities.detail(cam.id), "Model"),
        "camgrn",
        "the camera should start on its idle (green) model",
      );

      // Stand in the camera's sweep, in line of sight, and hold still: the
      // camera escalates on its own (Moderate ~3s, High ~6s of visibility).
      const [cx, cy, cz] = cam.position;
      await game.player.teleport({ x: cx - 2.83, y: cy - 1.9, z: cz + 2.83 });

      let model: string | undefined;
      for (let i = 0; i < 20 && model !== "camred"; i++) {
        await game.step({ frames: 120 });
        model = prop(await game.entities.detail(cam.id), "Model");
      }
      assert.equal(model, "camred", "the camera should reach its alert model");

      const detail = await game.entities.detail(cam.id);
      assert.equal(prop(detail, "AIAlertness"), "High");

      const saveResult = await game.save(saveName);
      assert.equal(saveResult.success, true, "save should report success");
    }

    // --- Session 2: a FRESH process loads that save. This is where the bug
    // --- bit: the alert model persisted, and re-deriving from it panicked.
    {
      await using game = await GameServer.launch({
        mission: "eng1.mis",
      });
      await game.step({ frames: 2 });

      const loadResult = await game.load(saveName);
      assert.equal(loadResult.success, true, "load should report success");
      assert.equal(loadResult.mission, "medsci1.mis");

      // Runtime is alive and on the restored mission (a panic in the load
      // would have taken the game loop with it).
      assert.equal((await game.info()).mission, "medsci1.mis");

      const cam = await camera(game);
      const detail = await game.entities.detail(cam.id);
      assert.equal(
        prop(detail, "Model"),
        "camred",
        "the camera's alert model should round-trip through the save",
      );
      assert.equal(
        prop(detail, "AIAlertness"),
        "High",
        "the camera's alert state should round-trip through the save",
      );

      // And it stays a real model as the restored camera keeps running - the
      // derivation must not compound into a nonexistent model.
      await game.step({ frames: 60 });
      assert.equal(prop(await game.entities.detail(cam.id), "Model"), "camred");
    }
  },
);
