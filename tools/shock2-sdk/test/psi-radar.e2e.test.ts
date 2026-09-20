import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  for (const vr of difficulty === "normal" ? [false, true] : [false]) {
    test(`Radar detects moving enemies through walls (${difficulty}, ${vr ? "VR" : "flat"})`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", difficulty, debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 30 });
      if (vr) {
        const [amp] = await game.entities.byTemplate(-247);
        await aimVrHandAt(game, amp.position, 0.35);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
      }
      assert.deepEqual((await game.info()).player.radar_contacts, []);
      await selectPsiPower(game, "Radar");
      const before = (await game.info()).player;
      await pullTrigger(game);
      assert.equal((await game.info()).player.psi_points, before.psi_points! - 3);
      const [target] = (await game.entities.list({ filter: "OG-Pipe" })).entities;
      assert.ok(target);
      await game.entities.sendMessage(target.id, { type: "Damage", amount: 1 });
      // Backstop wall x=-15 lies between this camera and the pen at x=-9..-11.
      await game.camera.set({ position: [-18, 3, 0], lookAt: target.position });
      let contact;
      for (let i = 0; i < 40; i++) {
        await game.step({ frames: 3 });
        contact = (await game.info()).player.radar_contacts.find(c => c.entity_id === target.id);
        if (contact) break;
      }
      assert.ok(contact, "an alerted moving enemy creates a contact");
      const echoes = await game.scene.fromSource("psi_radar");
      assert.ok(echoes.some(o => o.entity_id === target.id && o.render_layer === "scene_overlay" && !o.depth_write));
      const actual = (await game.entities.detail(target.id)).position;
      assert.ok(Math.hypot(...actual.map((v, i) => v - contact!.position[i])) < 0.2);
      await game.entities.sendMessage(target.id, { type: "Damage", amount: 1000 });
      await game.step({ frames: 3 });
      assert.ok(!(await game.info()).player.radar_contacts.some(c => c.entity_id === target.id), "dead targets disappear immediately");
      await game.player.teleport({ x: 100, y: 2, z: 0 });
      await game.step({ frames: 3 });
      assert.deepEqual((await game.info()).player.radar_contacts, [], "out-of-range targets disappear");
      if (difficulty !== "normal") return;
      await game.player.teleport({ x: 0, y: 2, z: 0 });
      await game.step({ frames: 170 * 60 });
      assert.equal((await game.info()).player.life_state, "alive");
      assert.ok((await game.info()).player.active_psi_powers.includes("Radar"));
      await game.step({ frames: 12 * 60 });
      assert.ok(!(await game.info()).player.active_psi_powers.includes("Radar"));
      assert.deepEqual(await game.scene.fromSource("psi_radar"), []);
    });
  }
}
