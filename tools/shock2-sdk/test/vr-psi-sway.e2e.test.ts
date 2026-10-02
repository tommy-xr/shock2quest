import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const hand of ["left", "right"] as const) {
  test(`VR psi amp ${hand}: hand motion sways only the cable and settles`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    await game.input.set(`${hand}_hand.squeeze`, 1);
    const amp = await game.player.spawnItem(-247, { hand });
    const sign = hand === "left" ? -1 : 1;
    await game.input.set(`${hand}_hand.position`, [sign * .35, 1.35, -.65]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 120 });
    const pose = async () => { const p = await game.entities.animation(amp.entity_id); assert.ok(p); return p; };
    const rest = await pose();
    const angle = (p: typeof rest) => {
      const a = p.joint_axes[1][1], b = rest.joint_axes[1][1];
      const dot = a.reduce((sum, v, i) => sum + v * b[i], 0) / (Math.hypot(...a) * Math.hypot(...b));
      return Math.acos(Math.max(-1, Math.min(1, dot))) * 180 / Math.PI;
    };
    let peak = 0;
    for (let f = 1; f <= 60; f++) {
      await game.input.set(`${hand}_hand.position`, [sign * (.35 + .22 * Math.sin(f / 30 * Math.PI * 2)), 1.35, -.65]);
      await game.step({ frames: 1 });
      const p = await pose(); peak = Math.max(peak, angle(p));
      assert.deepEqual(p.joint_axes[0], rest.joint_axes[0], "amp/hand geometry must remain rigid");
    }
    assert.ok(peak > 2 && peak <= 18.01, `bounded cable sway must be visible, got ${peak} degrees`);
    await game.step({ frames: 240 });
    assert.ok(angle(await pose()) < .1, "cable settles when the hand stops");
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 1 });
    assert.equal((await game.info()).paused, true);
    await game.input.set(`${hand}_hand.position`, [sign * .45, 1.35, -.65]);
    await game.step({ frames: 30 });
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 1 });
    assert.equal((await game.info()).paused, false);
    for (let f = 0; f < 20; f++) {
      await game.step({ frames: 1 });
      assert.ok(angle(await pose()) < .1, "resume must resample hands moved while paused");
    }
    const beforeTeleport = await pose();
    await game.player.teleport({ x: 10, y: 1, z: -5 });
    await game.step({ frames: 1 });
    assert.ok(angle(await pose()) < .1, "player teleport must not become cable velocity");
    assert.deepEqual((await pose()).joint_axes[0], beforeTeleport.joint_axes[0]);
  });
}
