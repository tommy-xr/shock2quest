import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { TestContext } from "node:test";
import { GameServer, type LaunchOptions } from "../../src/index.js";

/** Launch with private user settings, e.g. `{ vr: { speed_boosts: "Classic" } }`.
 * The runtime reads the path once at spawn, so the env var is restored at once. */
export async function launchWithSettings(
  t: TestContext,
  settings: object,
  options: LaunchOptions,
): Promise<GameServer> {
  const root = await mkdtemp(join(tmpdir(), "shock2-settings-test-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const path = join(root, "settings.json");
  await writeFile(path, JSON.stringify(settings));
  const previous = process.env.SHOCK2_SETTINGS_PATH;
  process.env.SHOCK2_SETTINGS_PATH = path;
  try {
    return await GameServer.launch(options);
  } finally {
    if (previous === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
    else process.env.SHOCK2_SETTINGS_PATH = previous;
  }
}
