import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import type { GameServer, Vec3 } from "../../src/index.js";

/// With SHOCK2_TRAIL_SHOTS=<dir>, scenarios record the player trail and save a
/// side-view screenshot of it (plus the samples as JSON) under <dir>.
const shotDir = process.env.SHOCK2_TRAIL_SHOTS;

/// Turn the trail on (a no-op unless SHOCK2_TRAIL_SHOTS is set).
export async function startTrail(game: GameServer): Promise<void> {
  if (!shotDir) return;
  await game.devParams.set("player_trail_seconds", 120);
  await game.devParams.set("player_trail", 1);
}

/// Save the trail as `<name>.json`, then frame it side-on (from +z, far
/// enough back to fit its bounding box) and save `<name>.png`. Call before
/// asserting, so a failing run still leaves its picture.
export async function shootTrail(game: GameServer, name: string): Promise<void> {
  if (!shotDir) return;
  const dir = resolve(shotDir);
  mkdirSync(dir, { recursive: true });
  const trail = await game.player.trail();
  writeFileSync(join(dir, `${name}.json`), JSON.stringify(trail));
  if (trail.length === 0) return;
  const axis = (i: number) => trail.map((s) => s.pos[i]);
  const lo = [0, 1, 2].map((i) => Math.min(...axis(i)));
  const hi = [0, 1, 2].map((i) => Math.max(...axis(i)));
  const centre = lo.map((v, i) => (v + hi[i]) / 2) as Vec3;
  const extent = Math.max(hi[0] - lo[0], hi[1] - lo[1], 2);
  await game.devParams.set("free_camera_cull", 1);
  await game.camera.set({
    position: [centre[0], centre[1], hi[2] + 1.5 * extent + 2],
    lookAt: centre,
  });
  // No step: a paused runtime still renders each loop, so the screenshot
  // sees the new camera without advancing the simulation under test.
  await game.screenshot(join(dir, `${name}.png`));
  await game.camera.attach();
}
