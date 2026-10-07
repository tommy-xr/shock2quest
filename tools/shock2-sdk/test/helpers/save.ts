import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { findRepoRoot } from "../../src/index.js";

/** Locate a fixture written by game.save, using the runtime's data roots. */
export function savedGamePath(name: string): string {
  const root = findRepoRoot(process.cwd())!;
  const path = [process.env.DARK_ASSET_PATH, join(root, "Data"), join(root, "..", "Data")]
    .filter((p): p is string => Boolean(p))
    .map(p => join(p, "saves", `${name}.sav`)).find(existsSync);
  assert.ok(path, "the runtime must write the fixture save");
  return path;
}
