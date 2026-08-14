#!/usr/bin/env node
// Remove saves left behind by the e2e suite.
//
// Every e2e test names its save `<something>${sep}${Date.now()}`, so a 13-digit
// millisecond suffix is a reliable marker for a test-generated file - no human
// types one, and it is the only kind that *accumulates* (a fresh name every
// run). Nothing else is touched: a real `save1.sav`, and fixed-name fixtures
// like `frontier.sav` or `map-e2e.sav` that tests deliberately write once and
// reload, are all left alone.
//
// This matters because the data root is usually the player's *retail install*
// (`DARK_ASSET_PATH`), and saves are written to `<data root>/saves`. Left
// alone the suite accumulates hundreds of files inside the game install, where
// a store client's "verify files" can trip over them.
//
// Runs before the suite (so a crashed run is cleaned up next time) and after
// it, and can be invoked directly with `npm run clean:saves`.

import { existsSync, readdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import { findRepoRoot } from "../dist/src/index.js";

/**
 * `<name><-|_><13-digit epoch ms>.sav` - what every e2e test produces. Both
 * separators occur in the suite (`camera_alert_e2e_...`, `flat-ui-drag-save-...`).
 */
const TEST_SAVE = /[-_]\d{13}\.sav$/;

const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
const roots = [
  process.env.DARK_ASSET_PATH,
  join(repoRoot, "Data"),
  join(repoRoot, "..", "Data"),
].filter(Boolean);

const dryRun = process.argv.includes("--dry-run");

let removed = 0;
const seen = new Set();
for (const root of roots) {
  const dir = join(root, "saves");
  if (seen.has(dir) || !existsSync(dir)) continue;
  seen.add(dir);
  for (const entry of readdirSync(dir)) {
    if (!TEST_SAVE.test(entry)) continue;
    const path = join(dir, entry);
    if (!statSync(path).isFile()) continue;
    if (dryRun) {
      console.log(`would remove ${path}`);
    } else {
      rmSync(path);
    }
    removed += 1;
  }
}

if (removed > 0) {
  const verb = dryRun ? "would remove" : "removed";
  console.log(`clean-test-saves: ${verb} ${removed} test save(s)`);
}
