import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { lookQuat } from "../src/game.js";
import { setHandWorldPose } from "../src/vr-pose.js";
import { add, sub } from "../src/vec.js";

for (const slot of ["pouch", 0, 1] as const) {
  for (const hand of ["left", "right"] as const) {
    test(`${hand} hand picks up a world item through empty ${slot === "pouch" ? "ammo pouch" : `holster ${slot}`}`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
      await game.step({ frames: 30 });
      const item = (await game.entities.byTemplate(-17))[0];
      assert.ok(item);
      await game.player.teleport({ x: item.position[0], y: 2, z: item.position[2] + 1.2 });
      await game.step({ frames: 10 });
      const { player } = await game.info();
      assert.equal(player.wielded_entity_id, null);
      assert.equal(player.right_hand_entity_id, null);
      assert.deepEqual(player.hand_feedback?.holsters?.items, [null, null]);
      const center = slot === "pouch"
        ? player.hand_feedback?.ammo_pouch?.center
        : player.hand_feedback?.holsters?.centers?.[slot];
      assert.ok(center);
      await game.input.set(`${hand}_hand.squeeze`, 0);
      const i = hand === "left" ? 0 : 1;
      // Body targets use the calibrated palm, not the controller origin.
      // Re-aim after centering so the nearby belt card cannot claim this grip.
      let position = center;
      for (let attempt = 0; attempt < 4; attempt++) {
        await setHandWorldPose(game, player, hand, position, lookQuat(sub(item.position, position)));
        await game.step({ frames: 3 });
        const palm = (await game.info()).player.hand_feedback?.glove_contacts?.centers[i];
        assert.ok(palm);
        position = add(position, sub(center, palm));
      }
      const reached = (await game.info()).player;
      const palm = reached.hand_feedback?.glove_contacts?.centers[i];
      assert.ok(palm);
      const radius = slot === "pouch" ? reached.hand_feedback!.ammo_pouch!.radius : reached.hand_feedback!.holsters!.radius;
      assert.ok(Math.hypot(...sub(palm, center)) < radius * 0.1, "palm must be centered inside the empty body's grab target");
      assert.equal(slot === "pouch" ? reached.hand_feedback?.ammo_pouch?.near[i] : reached.hand_feedback?.holsters?.near[i], slot === "pouch" ? false : null);
      assert.equal(reached.hand_feedback?.[hand]?.target, item.id, JSON.stringify(reached.hand_feedback));
      await game.input.set(`${hand}_hand.squeeze`, 1);
      await game.step({ frames: 5 });
      const after = (await game.info()).player;
      assert.equal(after[hand === "left" ? "wielded_entity_id" : "right_hand_entity_id"], item.id, JSON.stringify(after.hand_feedback));
    });
  }
}
