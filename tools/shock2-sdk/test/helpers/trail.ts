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

/// Frame the trail from `camera` and save `<name>.png` + `<name>.json`.
/// Call before asserting, so a failing run still leaves its picture.
export async function shootTrail(
  game: GameServer,
  name: string,
  camera: { position: Vec3; lookAt: Vec3 },
): Promise<void> {
  if (!shotDir) return;
  const dir = resolve(shotDir);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${name}.json`), JSON.stringify(await game.player.trail()));
  await game.devParams.set("free_camera_cull", 1);
  await game.camera.set(camera);
  await game.step({ frames: 1 });
  await game.screenshot(join(dir, `${name}.png`));
}
