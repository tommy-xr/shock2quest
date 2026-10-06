import { mkdtemp, readdir, symlink, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import type { TestContext } from "node:test";
import { GameServer, type LaunchOptions } from "../../src/index.js";
import { dataRoot } from "./crf.js";

/** Developer UI tests need an enabled sentinel, independent of the user's install.
 * Share read-only assets, but keep saves, settings and the sentinel private. */
export async function launchDeveloperGame(t: TestContext, options: LaunchOptions): Promise<GameServer> {
  const source = resolve(dataRoot());
  const root = await mkdtemp(join(tmpdir(), "shock2-developer-test-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const entry of await readdir(source)) {
    if (["saves", "settings.json", "user-settings.json", "developer-mode", "recordings"].includes(entry)) continue;
    await symlink(join(source, entry), join(root, entry));
  }
  await writeFile(join(root, "developer-mode"), "Developer fixture\n");
  const previous = process.env.DARK_ASSET_PATH;
  const previousSettings = process.env.SHOCK2_SETTINGS_PATH;
  process.env.DARK_ASSET_PATH = root;
  process.env.SHOCK2_SETTINGS_PATH = join(root, "settings.json");
  try {
    return await GameServer.launch(options);
  } finally {
    if (previous === undefined) delete process.env.DARK_ASSET_PATH;
    else process.env.DARK_ASSET_PATH = previous;
    if (previousSettings === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
    else process.env.SHOCK2_SETTINGS_PATH = previousSettings;
  }
}
