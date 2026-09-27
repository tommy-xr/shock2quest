import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer, HttpError } from "../src/index.js";

test("screenshot rejects a failed write and returns a complete PNG after recovery", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  const directory = await mkdtemp(join(tmpdir(), "shock2-screenshot-"));
  try {
    await using game = await GameServer.launch({ mission: "debug_minimal" });
    await game.step({ frames: 1 });
    await assert.rejects(game.screenshot(directory), (error: unknown) =>
      error instanceof HttpError && error.status === 500 && error.body.includes("failed to write screenshot"));
    const path = join(directory, "recovered.png");
    const result = await game.screenshot(path, 320);
    const png = await readFile(path);
    assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
    assert.equal(png.subarray(-8, -4).toString(), "IEND");
    assert.equal(result.size_bytes, (await stat(path)).size);
    assert.deepEqual(result.resolution, [320, 240]);
    assert.equal(png.readUInt32BE(16), 320);
    assert.equal(png.readUInt32BE(20), 240);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
