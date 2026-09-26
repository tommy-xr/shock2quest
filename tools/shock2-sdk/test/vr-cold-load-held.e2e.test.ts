import assert from "node:assert/strict";
import { test } from "node:test";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "VR cold load retains a physical Wrench through neutral startup and deliberate release",
  {
    skip: !e2eEnabled,
    timeout: 600_000,
  },
  async () => {
    const saveName = `vr_cold_load_held_${Date.now()}`;
    try {
      {
        await using game = await GameServer.launch({
          mission: "command2.mis",
          debugFlags: ["--vr"],
        });
        await game.step({ frames: 10 });
        const wrench = (
          await game.entities.list({ filter: "Wrench", limit: 30 })
        ).entities.find((e) => e.template_id === 786);
        assert.ok(wrench, "authored command2 Wrench exists");
        await game.player.teleport({
          x: wrench.position[0],
          y: wrench.position[1] - 1.4,
          z: wrench.position[2] + 1.2,
        });
        await game.step({ frames: 2 });
        await aimVrHandAt(game, wrench.position, 0.2, 1);
        assert.equal(
          (await game.info()).player.right_hand_entity_id,
          wrench.id,
        );
        await game.input.set("right_hand.position", [0.2, 2.3, -0.45]);
        await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
        await game.step({ frames: 20 });
        assert.equal(
          (await game.info()).player.right_hand_entity_id,
          wrench.id,
        );
        assert.equal((await game.save(saveName)).success, true);
      }
      {
        // A new runtime supplies genuinely neutral input; no squeeze is injected.
        await using game = await GameServer.launch({
          mission: "command2.mis",
          debugFlags: ["--vr"],
        });
        assert.equal((await game.load(saveName)).success, true);
        await game.input.set("right_hand.position", [0.2, 2.3, -0.45]);
        await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
        await game.step({ frames: 5 });
        const held = (await game.info()).player.right_hand_entity_id;
        assert.notEqual(
          held,
          null,
          "cold-loaded Wrench must survive neutral startup frames",
        );
        const detail = await game.entities.detail(held!);
        assert.equal(
          detail.properties.find((p) => p.name === "Model")?.value,
          "wrench_h",
        );
        assert.ok(
          (await game.scene.objects({ entityId: held! })).objects.length > 0,
          "restored Wrench is rendered",
        );
        await game.step({ frames: 60 });
        assert.equal((await game.info()).player.right_hand_entity_id, held);
        // Strike the shipped one-HP pane while startup squeeze is still neutral.
        // Same physical sweep as vr-melee-contact: ownership restored from disk
        // must also retain the real WeaponBash damage path.
        const pane = (
          await game.entities.list({ filter: "Window 2", limit: 30 })
        ).entities.find((e) => e.template_id === 82);
        assert.ok(pane);
        await game.player.teleport({
          x: pane.position[0],
          y: pane.position[1] - 1.04,
          z: pane.position[2] - 2.6,
        });
        await game.input.set("right_hand.position", [0.7, 0.45, 0.3]);
        await game.step({ frames: 30 });
        const beforeAttack =
          (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
        for (let frame = 1; frame <= 60; frame++) {
          const t = frame / 60;
          await game.input.set("right_hand.position", [
            0.7 + (0.05 - 0.7) * t,
            0.45,
            0.3 + (3.4 - 0.3) * t,
          ]);
          await game.step({ frames: 1 });
        }
        const impacts = (await game.messages.recent()).messages.filter(
          (m) => m.sequence > beforeAttack && m.to.entity_id === pane.id,
        );
        assert.ok(
          impacts.some((m) => m.payload === "Damage"),
          "restored Wrench deals physical melee damage",
        );
        assert.ok(
          impacts.some((m) => m.payload === "Slay"),
          "authored one-HP pane breaks",
        );
        assert.equal((await game.info()).player.right_hand_entity_id, held);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 5 });
        assert.equal((await game.info()).player.right_hand_entity_id, held);
        await game.input.set("right_hand.squeeze", 0);
        await game.step({ frames: 5 });
        assert.equal((await game.info()).player.right_hand_entity_id, null);
        assert.equal(
          (await game.entities.detail(held!)).properties.find(
            (p) => p.name === "Model",
          )?.value,
          "wrench_w",
        );
        assert.equal(
          (await game.physics.bodies({ entityId: held! })).bodies.length,
          1,
          "release creates exactly one loose item body",
        );
      }
    } finally {
      if (process.env.DARK_ASSET_PATH)
        rmSync(join(process.env.DARK_ASSET_PATH, "saves", `${saveName}.sav`), {
          force: true,
        });
    }
  },
);
