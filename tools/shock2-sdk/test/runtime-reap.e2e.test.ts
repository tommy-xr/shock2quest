import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { test } from "node:test";

import { HttpClient } from "../src/client.js";
import { GameServer } from "../src/index.js";
import { findRepoRoot, runtimeLeasePath } from "../src/server.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

interface OwnedRuntime {
  baseUrl: string;
  instanceId: string;
  pid: number;
}

async function launchOrphan(port: number): Promise<OwnedRuntime> {
  const source = [
    'import { GameServer } from "./dist/src/index.js";',
    `const game = await GameServer.launch({ mission: "debug_minimal", port: ${port} });`,
    "const health = await fetch(`${game.baseUrl}/v1/health`).then((response) => response.json());",
    'process.stdout.write(JSON.stringify({ baseUrl: game.baseUrl, instanceId: health.instance_id, pid: game.child.pid }) + "\\n", () => process.exit(0));',
  ].join("\n");
  const child = spawn(process.execPath, ["--input-type=module", "-e", source], {
    cwd: process.cwd(),
    env: process.env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => (stdout += chunk.toString()));
  child.stderr.on("data", (chunk) => (stderr += chunk.toString()));
  const exitCode = await new Promise<number | null>((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", resolve);
  });
  assert.equal(exitCode, 0, `orphan launcher failed:\n${stderr}`);
  return JSON.parse(stdout.trim()) as OwnedRuntime;
}

async function health(runtime: OwnedRuntime): Promise<{ instance_id: string }> {
  return new HttpClient(runtime.baseUrl).get<{ instance_id: string }>(
    "/v1/health",
  );
}

async function stopOwnedRuntime(runtime: OwnedRuntime): Promise<void> {
  const client = new HttpClient(runtime.baseUrl);
  try {
    await client.post("/v1/shutdown", { instance_id: runtime.instanceId });
  } catch {
    // The runtime can close the connection while processing the command.
  }
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 50));
    let live: { instance_id: string };
    try {
      live = await health(runtime);
    } catch {
      return;
    }
    assert.equal(
      live.instance_id,
      runtime.instanceId,
      "refusing to clean up a replacement runtime",
    );
  }

  if (process.platform === "win32") {
    const result = spawnSync(
      "taskkill",
      ["/pid", String(runtime.pid), "/T", "/F"],
      { stdio: "ignore" },
    );
    assert.equal(result.status, 0, `failed to stop owned pid ${runtime.pid}`);
  } else {
    process.kill(-runtime.pid, "SIGKILL");
  }
  for (let attempt = 0; attempt < 100; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 50));
    try {
      await client.get("/v1/health");
    } catch {
      return;
    }
  }
  throw new Error(
    `owned runtime ${runtime.instanceId} did not exit after kill`,
  );
}

test(
  "launch can reap the exact prior SDK-owned runtime on its requested port",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const port = Number(process.env.SHOCK2_E2E_PORT ?? 8786);
    const orphan = await launchOrphan(port);
    let replacement: GameServer | undefined;
    try {
      assert.equal((await health(orphan)).instance_id, orphan.instanceId);

      // Use a variable so this negative-first test compiles on the parent SDK,
      // where the option is not declared and is simply ignored at runtime.
      const options = { mission: "debug_minimal", port, reapPrevious: true };
      replacement = await GameServer.launch(options);
      assert.equal(
        replacement.baseUrl,
        orphan.baseUrl,
        "the prior owned runtime should be reaped instead of accumulating on the next port",
      );
      const replacementHealth = await new HttpClient(replacement.baseUrl).get<{
        instance_id: string;
      }>("/v1/health");
      assert.notEqual(replacementHealth.instance_id, orphan.instanceId);
    } finally {
      await replacement?.shutdown();
      await stopOwnedRuntime(orphan);
    }
  },
);

test(
  "launch preserves a runtime whose live instance id does not match the lease",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const port = Number(process.env.SHOCK2_E2E_PORT ?? 8786) + 2;
    const repoRoot = findRepoRoot(process.cwd());
    assert.ok(repoRoot);
    const orphan = await launchOrphan(port);
    const realLease = runtimeLeasePath(repoRoot, port, orphan.instanceId);
    const unrelatedInstanceId = randomUUID();
    const unrelatedLease = runtimeLeasePath(
      repoRoot,
      port,
      unrelatedInstanceId,
    );
    unlinkSync(realLease);
    mkdirSync(dirname(unrelatedLease), { recursive: true });
    writeFileSync(
      unrelatedLease,
      `${JSON.stringify({ port, instanceId: unrelatedInstanceId, pid: orphan.pid })}\n`,
      { mode: 0o600 },
    );

    let replacement: GameServer | undefined;
    try {
      replacement = await GameServer.launch({
        mission: "debug_minimal",
        port,
        reapPrevious: true,
      });
      assert.notEqual(
        replacement.baseUrl,
        orphan.baseUrl,
        "a runtime without an exact matching lease must be preserved",
      );
      assert.equal((await health(orphan)).instance_id, orphan.instanceId);
    } finally {
      await replacement?.shutdown();
      await stopOwnedRuntime(orphan);
      unlinkSync(unrelatedLease);
    }
  },
);
