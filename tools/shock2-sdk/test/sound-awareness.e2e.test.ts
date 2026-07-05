import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

test(
  "firing a weapon alerts a nearby AI that never saw the player",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8107),
    });

    await game.step({ frames: 10 });

    // Wield a pistol (spawns and auto-wields in front of the empty-handed
    // player), then spawn a monster - both appear along the player's aim.
    const start = await game.player.position();
    await game.input.trigger("SpawnDebugItem");
    await game.step({ frames: 10 });

    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");
    const m = monster.position;

    // Step to the side of the monster (perpendicular to the spawn axis) so a
    // shot fired straight ahead MISSES it - the alert must come from the
    // gunshot noise, not a bullet hit. Stay close enough to be in earshot.
    const fx = m[0] - start.x;
    const fz = m[2] - start.z;
    const flen = Math.hypot(fx, fz) || 1;
    const px = fz / flen; // perpendicular in XZ
    const pz = -fx / flen;
    await game.player.teleport({
      x: m[0] + px * 5,
      y: start.y,
      z: m[2] + pz * 5,
    });
    await game.step({ frames: 5 });

    // Force it unaware and confirm it stays that way for a couple of frames.
    // Escalation by sight needs ~1.5s of continuous visibility, so within a
    // few frames the only thing that can jump it to a combat level is the
    // gunshot noise - the negative case (no noise raised) leaves it Lowest.
    await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 3 });
    let detail = await game.entities.detail(monster.id);
    assert.equal(
      aiProp(detail, "AIAlertness"),
      "Lowest",
      "monster should be unaware right before the shot",
    );

    // Fire the pistol. The gunshot noise reaches the nearby monster.
    await fireOnce(game);
    await game.step({ frames: 5 });
    detail = await game.entities.detail(monster.id);
    assert.equal(
      aiProp(detail, "AIAlertness"),
      "Moderate",
      `the gunshot should alert the monster to Moderate within a few frames (too fast for sight); got ${aiProp(detail, "AIAlertness")}`,
    );
    // It investigates toward the shot (the player's position).
    assert.ok(
      aiProp(detail, "AILastKnown") !== undefined,
      "a noise-alerted monster should have a last-known position to investigate",
    );
  },
);
