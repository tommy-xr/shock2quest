import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const SHOT = -3496;
const HEAD = 298;
const SHIELDS = [270, 272, 274, 275, 277, 278, 279, 280];

for (const vr of [false, true]) {
  test(`SHODAN's authored projectile survives a flight save and damages the player (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async (t) => {
    // Declared isolated fixture, not campaign traversal. The live head fires its
    // own stock projectile; no Damage, Slay, projectile spawn, or healing calls.
    await using game = await GameServer.launch({ mission: "shodan.mis:32,-90.3,70", debugFlags: vr ? ["--vr"] : [] });
    await game.input.lookAtWorldPoint([32, -88, 72]);
    let hpBefore = (await game.info()).player.hit_points;
    assert.ok(hpBefore !== null && hpBefore > 0);
    let saved = false;
    let observedShot = false;
    let restoredShotId: number | undefined;
    let traceStart = 0;
    for (let frame = 0; frame < 600; frame++) {
      await game.step({ frames: 1 });
      const shots = await game.entities.byTemplate(SHOT);
      if (shots.length > 0) {
        observedShot = true;
        if (!saved) {
          hpBefore = (await game.info()).player.hit_points;
          assert.ok(hpBefore !== null && hpBefore > 0);
          const file = `shodan_shot_flight_${Date.now()}`;
          assert.equal((await game.save(file)).success, true);
          assert.equal((await game.load(file)).success, true);
          const restored = await game.entities.byTemplate(SHOT);
          assert.equal(restored.length, 1, "same-build load must preserve the in-flight projectile");
          restoredShotId = restored[0]!.id;
          traceStart = (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
          hpBefore = (await game.info()).player.hit_points;
          assert.ok(hpBefore !== null && hpBefore > 0);
          saved = true;
        }
      }
      const info = await game.info();
      const impacted = saved && !shots.some(shot => shot.id === restoredShotId)
        && (await game.entities.byTemplate(-2752)).length > 0;
      if (impacted) {
        // Contact queues its Damage message after this frame's script pass.
        await game.step({ frames: 2 });
        const impactInfo = await game.info();
        assert.ok(impactInfo.player.hit_points !== null && impactInfo.player.hit_points <= hpBefore - 4,
          `authored corpse splash must hurt beyond the one-point aura tick: ${hpBefore}→${impactInfo.player.hit_points}`);
        const damage = (await game.messages.recent()).messages.filter(message =>
          message.sequence > traceStart && message.to.entity_id === impactInfo.player.entity_id
          && message.payload === "Damage");
        assert.ok(damage.length > 0, "the projectile corpse delivers radius damage to the player");
        assert.ok(damage.every(message => message.impact === null),
          "the dormant Null ShodanStim must not become an extra contact hit");
        assert.ok(observedShot && saved, "damage follows a real, restored projectile");
        assert.ok((await game.entities.byTemplate(HEAD)).length > 0, "the firing head stays alive");
        for (const template of SHIELDS) {
          const [shield] = await game.entities.byTemplate(template);
          assert.ok(shield, "the stock shield perimeter is intact");
        }
        t.diagnostic(`Natural restored shot exploded: HP ${hpBefore} → ${impactInfo.player.hit_points}; all eight shields intact`);
        return;
      }
    }
    assert.fail("the live head must fire inside its intact perimeter and hit the player");
  });
}
