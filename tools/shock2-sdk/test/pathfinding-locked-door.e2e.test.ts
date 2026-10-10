import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { AiPathEntry, Vec3 } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const GOAL: Vec3 = [21.6, 0.4, -44];

// The real security door is axis-aligned. Check the route's actual crossing
// of its closed aperture, rather than requiring no route: MedSci1 offers a
// legitimate longer detour while this door is locked.
function crossesDoor(path: AiPathEntry, bounds: [Vec3, Vec3]): boolean {
  const [min, max] = bounds;
  const plane = (min[2] + max[2]) / 2;
  return path.waypoints.some((to, index) => {
    if (index === 0) return false;
    const from = path.waypoints[index - 1];
    const dz = to[2] - from[2];
    if (Math.abs(dz) < 1e-6) return false;
    const t = (plane - from[2]) / dz;
    if (t < 0 || t > 1) return false;
    const x = from[0] + t * (to[0] - from[0]);
    const y = from[1] + t * (to[1] - from[1]);
    return x >= min[0] && x <= max[0] && y >= min[1] - 0.05 && y <= max[1] + 0.05;
  });
}

for (const vr of [false, true]) {
  test(
    `medsci1: a pursuing hybrid reroutes when a real door locks (${vr ? "VR" : "flat"})`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        experimental: ["nav_bridges"],
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 2 });
      // Positive template_id values identify authored mission objects;
      // runtime entity IDs are rediscovered for every launch.
      const [door] = await game.entities.byTemplate(276);
      const [hybrid] = await game.entities.byTemplate(596);
      assert.ok(door && hybrid, "expected MedSci1 security door 276 and hybrid 596");
      await game.entities.sendMessage(door.id, { type: "TurnOff" });
      await game.step({ frames: 90 });
      const bounds = (await game.entities.detail(door.id)).selection_bounds;
      assert.ok(bounds, "the real door must expose its closed physical aperture");
      await game.entities.sendMessage(door.id, { type: "SetLocked", locked: true });
      await game.player.teleport({ x: GOAL[0], y: GOAL[1], z: GOAL[2] });
      await game.input.trigger("DebugForceChase");

      async function expectCrossing(expected: boolean, phase: string): Promise<void> {
        let latest: AiPathEntry | undefined;
        for (let frames = 0; frames < 300; frames += 6) {
          await game.step({ frames: 6 });
          latest = (await game.pathfinding.aiPaths()).find((p) => p.entity_id === hybrid.id);
          if (
            latest?.outcome === "Full" &&
            (latest.live_path_len ?? 0) > 1 &&
            Math.hypot(latest.goal[0] - GOAL[0], latest.goal[2] - GOAL[2]) < 0.5 &&
            crossesDoor(latest, bounds!) === expected
          ) return;
        }
        const doorNow = await game.entities.detail(door.id);
        const gateRoute = await game.pathfinding.route(hybrid.position, [22.11, -1.6, -47.6]);
        assert.fail(`${phase}: expected door crossing=${expected}, door=${JSON.stringify(doorNow.position)}, gateRoute=${JSON.stringify(gateRoute)}, latest=${JSON.stringify(latest)}`);
      }

      await expectCrossing(false, "locked door must be avoided");
      await game.entities.sendMessage(door.id, { type: "SetLocked", locked: false });
      await expectCrossing(true, "unlock must restore the shorter crossing");
      // Keep the same live pursuit: no reload, teleport, alert reset or direct
      // path-service mutation can hide a stale door-state sync.
      await game.entities.sendMessage(door.id, { type: "SetLocked", locked: true });
      await expectCrossing(false, "relocking must remove the crossing again");
    },
  );
}
