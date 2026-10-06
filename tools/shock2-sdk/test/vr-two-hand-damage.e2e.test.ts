import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { sub, quatConjugate, quatMultiply, quatRotate } from "./helpers/vr-hand.js";

for (const mode of ["one hand", "two hands", "late support", "released support"] as const) {
  const supported = mode === "two hands";
  test(`physical wrench damage: ${mode}`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_melee", port: 0, debugFlags: ["--vr"] });
      await game.step({ frames: 30 });
      const [wrench] = await game.entities.byTemplate(-928);
      assert.ok(wrench);
      const pawn = (await game.info()).player;
      await game.input.set("right_hand.position", sub(wrench.position, pawn.position));
      await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 10 });
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 15 });
      assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);
      const target = (await game.entities.byTemplate(-397)).sort((a,b) => b.position[0]-a.position[0])[0];
      assert.ok(target);
      const [tx, ty, tz] = target.position;
      await game.player.teleport({ x: tx + 1.1, y: ty - 1, z: tz });
      await game.step({ frames: 20 });
      const stance = (await game.info()).player;
      const start: Vec3 = [0.6, ty + 0.3 - stance.position[1], 0];
      await game.input.set("right_hand.position", start);
      await game.input.set("left_hand.position", [2, 1, 0]);
      await game.input.set("left_hand.squeeze", 0);
      await game.step({ frames: 30 });
      let supportStart: Vec3 | undefined;
      if (mode !== "one hand") {
        const state = (await game.info()).player;
        const socket = state.hand_grips.find(g => g.hand === "right")?.support;
        assert.ok(socket);
        const inverse = quatConjugate(state.rotation);
        supportStart = quatRotate(inverse, sub([socket.controller_position.x, socket.controller_position.y, socket.controller_position.z], state.position));
        const q = socket.controller_rotation;
        await game.input.set("left_hand.position", supportStart);
        await game.input.set("left_hand.rotation", quatMultiply(inverse, [q.v.x, q.v.y, q.v.z, q.s]));
        await game.step({ frames: 2 });
        await game.input.set("left_hand.squeeze", mode === "late support" ? 0 : 1);
        await game.step({ frames: 15 });
        assert.equal((await game.info()).player.hand_grips.find(g => g.hand === "right")?.support?.attached, mode !== "late support");
      }
      const health = async () => Number((await game.entities.detail(target.id)).properties.find(p => p.name === "HitPoints")?.value);
      const before = await health();
      const sequence = (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
      // The existing hitbox fixture sweeps through the torso in <0.4s, so one
      // real contact should bill exactly once. Translate both controllers
      // together to keep a real two-anchor attachment throughout the blow.
      for (let frame = 1; frame <= 20; frame++) {
        const dx = -2.2 * frame / 20;
        if (frame === 4 && (mode === "late support" || mode === "released support")) {
          assert.equal((await game.messages.recent()).messages.filter(m => m.sequence > sequence && m.payload === "Damage" && m.to.entity_id === target.id).length, 0,
            "the attachment edge must happen before the measured strike");
          if (mode === "late support") {
            const player = (await game.info()).player;
            const socket = player.hand_grips.find(g => g.hand === "right")!.support!;
            const at = socket.controller_position;
            const current = quatRotate(quatConjugate(player.rotation), sub([at.x, at.y, at.z], player.position));
            supportStart = [current[0] - dx, current[1], current[2]];
            const q = socket.controller_rotation;
            await game.input.set("left_hand.rotation", quatMultiply(quatConjugate(player.rotation), [q.v.x,q.v.y,q.v.z,q.s]));
          }
        }
        await game.input.set("right_hand.position", [start[0] + dx, start[1], start[2]]);
        if (supportStart) await game.input.set("left_hand.position", [supportStart[0] + dx, supportStart[1], supportStart[2]]);
        if (frame === 4 && mode === "late support") await game.input.set("left_hand.squeeze", 1);
        if (frame === 4 && mode === "released support") await game.input.set("left_hand.squeeze", 0);
        await game.step({ frames: 1 });
        if (frame === 4 && mode !== "one hand") assert.equal(
          (await game.info()).player.hand_grips.find(g => g.hand === "right")?.support?.attached, mode !== "released support",
          "the late-grab/release input must change the real attachment before contact");
        if (supported || (mode === "late support" && frame >= 4)) assert.equal((await game.info()).player.hand_grips.find(g => g.hand === "right")?.support?.attached, true,
          "the support hand must remain actually attached through impact");
      }
      await game.step({ frames: 2 });
      const hits = (await game.messages.recent()).messages.filter(m => m.sequence > sequence && m.payload === "Damage" && m.to.entity_id === target.id);
      assert.equal(hits.length, 1, "the test must measure one actual hitbox strike");
      assert.ok(hits[0]!.impact?.bone != null);
      const point = (await game.entities.detail(target.id)).aim_points?.find(p => p.joint_id === hits[0]!.impact!.bone);
      assert.equal(point?.classification, "limb", "the fixture must strike the same class of hitbox");
      assert.equal(before - await health(), Math.round((6 + 9) * 0.75 * (supported ? 1 : 0.5)),
        "the one-hand factor composes with both inherited authored blows and the existing limb multiplier before HP rounding");
    });
}
