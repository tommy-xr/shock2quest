import { mkdtempSync, readdirSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { dataRoot } from "./crf.js";

/** A private data root sharing the retail assets by symlink, pointed to by
 * DARK_ASSET_PATH until disposed. `exclude` keeps writable entries (saves,
 * recordings, ...) out, so a test can neither read nor overwrite the user's. */
export function isolatedDataRoot(exclude: string[]): { root: string; [Symbol.dispose](): void } {
  const previous = process.env.DARK_ASSET_PATH;
  const source = resolve(dataRoot());
  const root = mkdtempSync(join(tmpdir(), "shock2-data-root-"));
  for (const entry of readdirSync(source, { withFileTypes: true })) {
    if (!exclude.includes(entry.name)) {
      symlinkSync(join(source, entry.name), join(root, entry.name), entry.isDirectory() ? "dir" : "file");
    }
  }
  process.env.DARK_ASSET_PATH = root;
  return {
    root,
    [Symbol.dispose]() {
      if (previous === undefined) delete process.env.DARK_ASSET_PATH;
      else process.env.DARK_ASSET_PATH = previous;
      rmSync(root, { recursive: true, force: true });
    },
  };
}
