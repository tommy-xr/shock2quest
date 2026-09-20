import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  for (const vr of difficulty === "normal" ? [false, true] : [false]) {
    test(`Seekersense reveals world loot and clears carried items (${difficulty}, ${vr ? "VR" : "flat"})`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", difficulty, debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 30 });
      const [amp] = await game.entities.byTemplate(-247);
      if (vr) {
        await aimVrHandAt(game, amp.position, 0.35);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
      }
      const [hypo] = await game.entities.byTemplate(-52);
      const [clip] = await game.entities.byTemplate(-1358);
      const [hidden] = await game.entities.byTemplate(-928);
      const [crate] = await game.entities.byTemplate(-941);
      const [far] = await game.entities.byTemplate(-57);
      assert.ok(hypo && clip && hidden && crate && far);
      assert.deepEqual((await game.info()).player.seekersense_contacts, []);
      await selectPsiPower(game, "Seekersense");
      const before = (await game.info()).player.psi_points!;
      await pullTrigger(game);
      await game.step({ frames: 3 });
      const active = (await game.info()).player;
      assert.equal(active.psi_points, before - 4);
      const ids = active.seekersense_contacts.map(c => c.entity_id);
      for (const item of [hypo, clip, hidden, crate]) assert.ok(ids.includes(item.id), `missing ${item.name}`);
      assert.ok(!ids.includes(far.id));
      assert.ok(!ids.includes(amp.id), "held amp is not world loot");
      for (const enemy of (await game.entities.list({ filter: "OG-Pipe" })).entities) assert.ok(!ids.includes(enemy.id));
      await game.camera.set({ position: [-13, 2, 0], lookAt: hidden.position });
      await game.step({ frames: 2 });
      assert.ok((await game.scene.fromSource("psi_seekersense")).some(o => o.entity_id === hidden.id && o.render_layer === "scene_overlay"));
      await game.camera.attach();
      if (vr) {
        await game.input.set("right_hand.squeeze", 0);
        await game.step({ frames: 3 });
        await aimVrHandAt(game, (await game.entities.detail(clip.id)).position, 0.25);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, clip.id);
      } else {
        await game.player.give(clip.id);
        await game.step({ frames: 3 });
      }
      assert.ok(!(await game.info()).player.seekersense_contacts.some(c => c.entity_id === clip.id));
      await game.player.teleport({ x: 100, y: 2, z: 0 });
      await game.step({ frames: 3 });
      assert.deepEqual((await game.info()).player.seekersense_contacts, []);
      if (difficulty !== "normal") return;
      await game.player.teleport({ x: 0, y: 2, z: 0 });
      await game.step({ frames: 345 * 60 });
      assert.equal((await game.info()).player.life_state, "alive");
      assert.ok((await game.info()).player.active_psi_powers.includes("Seekersense"));
      await game.step({ frames: 17 * 60 });
      assert.ok(!(await game.info()).player.active_psi_powers.includes("Seekersense"));
      assert.deepEqual((await game.info()).player.seekersense_contacts, []);
      assert.deepEqual(await game.scene.fromSource("psi_seekersense"), []);
    });
  }
}
