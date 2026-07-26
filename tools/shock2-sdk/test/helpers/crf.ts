import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { inflateRawSync } from "node:zlib";

// Minimal read-one-entry access to a .crf resource archive (a plain zip),
// so tests can make archive-aware assertions about the shipped art without
// pulling in a zip dependency.

/** Resolve the game data root the way the runtime does: DARK_ASSET_PATH, else
 * the repo's Data/ folder relative to the SDK cwd.
 *
 * A classic install has a `res/` directory of `.crf` archives; a 25th
 * Anniversary install keeps everything inside `sshock2.kpf` instead, so both
 * sentinels count. */
export function dataRoot(): string {
  const candidates = [
    process.env.DARK_ASSET_PATH,
    path.resolve(process.cwd(), "../../Data"),
    path.resolve(process.cwd(), "Data"),
  ].filter((p): p is string => !!p);
  for (const c of candidates) {
    if (existsSync(path.join(c, "res")) || existsSync(path.join(c, "sshock2.kpf")))
      return c;
  }
  throw new Error(
    `game data root not found (tried: ${candidates.join(", ")}) - set DARK_ASSET_PATH`,
  );
}

/** Whether the resolved data root keeps its resources in loose `.crf` archives.
 *
 * Assertions that read a `.crf` straight off disk can only run on that layout -
 * on a 25th Anniversary install the same resources live inside KPF archives, so
 * such assertions should be skipped rather than failed. */
export function hasLooseCrfArchives(): boolean {
  return existsSync(path.join(dataRoot(), "res", "iface.crf"));
}

/** Extract one entry (case-insensitive name match) from a .crf/zip archive. */
export function readCrfEntry(crfPath: string, entryName: string): Buffer {
  const buf = readFileSync(crfPath);
  // Scan back for the end-of-central-directory record.
  let eocd = -1;
  for (let i = buf.length - 22; i >= 0; i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error(`no zip central directory in ${crfPath}`);
  const count = buf.readUInt16LE(eocd + 10);
  let off = buf.readUInt32LE(eocd + 16);
  const want = entryName.toLowerCase();
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(off) !== 0x02014b50) {
      throw new Error(`bad central directory entry in ${crfPath}`);
    }
    const method = buf.readUInt16LE(off + 10);
    const compressedSize = buf.readUInt32LE(off + 20);
    const nameLen = buf.readUInt16LE(off + 28);
    const extraLen = buf.readUInt16LE(off + 30);
    const commentLen = buf.readUInt16LE(off + 32);
    const localOff = buf.readUInt32LE(off + 42);
    const name = buf.toString("latin1", off + 46, off + 46 + nameLen);
    if (name.toLowerCase() === want) {
      const localNameLen = buf.readUInt16LE(localOff + 26);
      const localExtraLen = buf.readUInt16LE(localOff + 28);
      const dataStart = localOff + 30 + localNameLen + localExtraLen;
      const data = buf.subarray(dataStart, dataStart + compressedSize);
      return method === 0 ? Buffer.from(data) : inflateRawSync(data);
    }
    off += 46 + nameLen + extraLen + commentLen;
  }
  throw new Error(`${entryName} not found in ${crfPath}`);
}

/** Width/height from a PCX header. */
export function pcxSize(pcx: Buffer): { width: number; height: number } {
  const xmin = pcx.readInt16LE(4);
  const ymin = pcx.readInt16LE(6);
  const xmax = pcx.readInt16LE(8);
  const ymax = pcx.readInt16LE(10);
  return { width: xmax - xmin + 1, height: ymax - ymin + 1 };
}
