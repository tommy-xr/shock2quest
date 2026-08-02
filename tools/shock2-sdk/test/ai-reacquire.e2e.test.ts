import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end regression for #791: a hostile whose alertness decayed back to
// Lowest/Idle never re-acquired the player, because nothing made it look at
// what it could see - it turned away mid-detection and calmed down again.
//
// Opt-in (needs Data/ assets + compiles the runtime): npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

const PURSUING = new Set(["Chase", "MeleeAttack", "RangedAttack"]);

/** Degrees between where `detail` faces and the direction to `target` (XZ plane). */
function facingErrorDeg(
  detail: EntityDetailResult,
  target: { x: number; z: number },
): number {
  const [, y, , w] = detail.rotation;
  const yaw = 2 * Math.atan2(y, w);
  const dx = target.x - detail.position[0];
  const dz = target.z - detail.position[2];
  const len = Math.hypot(dx, dz) || 1;
  const dot = (Math.sin(yaw) * dx + Math.cos(yaw) * dz) / len;
  return (Math.acos(Math.max(-1, Math.min(1, dot))) * 180) / Math.PI;
}

test(
  "a decayed AI re-acquires a player standing in its field of view (#791)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8592),
    });

    await game.step({ frames: 10 });

    // Spawn a monster in front of the player, identifying it by diffing the
    // entity list across the spawn (medsci1 has native OG-Pipes too).
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    // A control: a native AI elsewhere on the deck, behind geometry. It must
    // stay calm throughout - re-acquisition is driven by sight, not by
    // proximity to a fight.
    const control = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && known.has(e.id) && e.distance > 10,
    );
    assert.ok(control, "expected a native OG-Pipe out of sight for the control");

    // The issue's state: a hostile that engaged the player earlier and has
    // since fully calmed down. SetAlertness is scenario setup only - what is
    // asserted below is what sight alone does from there.
    await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "High",
    });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 10 });
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Lowest");
    assert.equal(aiProp(detail, "AIBehavior"), "Idle");

    // Step off the creature's axis - still well inside its 60-degree FOV, so
    // it can plainly see the player, but no longer square-on. A calm AI holds
    // whatever heading it stopped on, so only one that actually looks at what
    // it sees ends up facing the player again.
    const start = await game.player.position();
    const dx = start.x - detail.position[0];
    const dz = start.z - detail.position[2];
    const distance = Math.hypot(dx, dz);
    const offset = distance * Math.tan((30 * Math.PI) / 180);
    let player = start;
    for (const sign of [1, -1]) {
      await game.player.moveTo({
        x: start.x + (sign * -dz * offset) / distance,
        y: start.y,
        z: start.z + (sign * dx * offset) / distance,
      });
      await game.step({ frames: 5 });
      player = await game.player.position();
      if (Math.hypot(player.x - start.x, player.z - start.z) > offset * 0.7) break;
      await game.player.moveTo(start);
      await game.step({ frames: 5 });
    }
    detail = await game.entities.detail(monster.id);
    const offAxis = facingErrorDeg(detail, player);
    assert.ok(
      offAxis > 10,
      `setup: the player should end up off the AI's axis, got ${offAxis.toFixed(0)} degrees`,
    );
    assert.equal(
      aiProp(detail, "AITargetVisible"),
      "true",
      "setup: the player must still be plainly visible to the AI",
    );

    // Sight must now drive it back up the escalation ladder: look at the
    // player, and with the contact held, escalate (Lowest -> Low -> Moderate
    // is 1.5s of continuous sight by default).
    const trace: string[] = [];
    let reacquired: EntityDetailResult | undefined;
    let lookedAway: string | undefined;
    for (let tick = 0; tick < 8 && !reacquired; tick++) {
      await game.step({ frames: 30 }); // 0.5 sim-seconds
      const d = await game.entities.detail(monster.id);
      const visible = aiProp(d, "AITargetVisible");
      const facing = facingErrorDeg(d, player);
      trace.push(
        `${((tick + 1) * 0.5).toFixed(1)}s ${aiProp(d, "AIAlertness")}/${aiProp(d, "AIBehavior")}` +
          ` vis=${visible} facing=${facing.toFixed(0)}deg`,
      );
      if (PURSUING.has(aiProp(d, "AIBehavior") ?? "")) {
        reacquired = d;
        break;
      }
      // Pre-pursuit only: the pursuing behaviors own the heading from here.
      if (visible !== "true" || facing > 10) lookedAway ??= trace[trace.length - 1];
    }
    assert.ok(
      !lookedAway,
      `the AI failed to keep looking at a player it could see: ${trace.join(", ")}`,
    );
    assert.ok(
      reacquired,
      `the decayed AI never re-acquired the visible player: ${trace.join(", ")}`,
    );
    const level = aiProp(reacquired, "AIAlertness");
    assert.ok(
      level === "Moderate" || level === "High",
      `expected a pursuing alertness level, got ${level}`,
    );

    // No over-correction: the out-of-sight AI never noticed a thing.
    assert.equal(
      aiProp(await game.entities.detail(control.id), "AIAlertness"),
      "Lowest",
      "an AI that cannot see the player must stay calm",
    );
  },
);
