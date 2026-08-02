import assert from "node:assert/strict";
import { type ChildProcess, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { findRepoRoot, reapStaleRuntimeOnPort } from "../src/server.js";

// Verifies the real (unmocked) reap path end-to-end: a genuinely orphaned
// debug_runtime - one whose process outlives the harness that spawned it,
// e.g. a crashed agent that never reached /v1/shutdown (#786) - gets killed
// and its port reclaimed by the next launch(), rather than launch() quietly
// walking up to a different port and leaving the orphan running.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const PORT = Number(process.env.SHOCK2_E2E_PORT ?? 8580);

interface Orphan {
  child: ChildProcess;
  instanceId: string;
}

/**
 * Spawns a debug_runtime directly (bypassing GameServer.launch entirely) so
 * it is untracked by the SDK's own in-process port bookkeeping
 * (`reservedPorts`) - a genuine orphan is a process from a *different* Node
 * invocation, so going through the SDK's own launch() here would exercise a
 * same-process bookkeeping guard instead of the real reap path.
 */
function spawnOrphan(port: number, repoRoot: string): Orphan {
  const instanceId = randomUUID();
  const child = spawn(
    "cargo",
    [
      "run",
      "-p",
      "debug_runtime",
      "--",
      "--mission",
      "medsci1.mis",
      "--port",
      String(port),
      "--instance-id",
      instanceId,
    ],
    { cwd: repoRoot, stdio: "ignore", detached: true },
  );
  return { child, instanceId };
}

/**
 * Waits for our own orphan to come up, identified by `/v1/health`'s
 * `instance_id` - not merely "some healthy service answered on this port".
 * On a shared dev host, this port could already be held by an unrelated
 * runtime (another agent's session); if the caller proceeded to treat that
 * as "our fixture is ready" it could reap or otherwise mistreat a foreign
 * process later in the test. Fails loudly instead of guessing.
 */
async function waitUntilOwnedHealthy(
  port: number,
  expectedInstanceId: string,
  timeoutMs: number,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/v1/health`);
      if (res.ok) {
        const body = (await res.json()) as { instance_id?: string | null };
        if (body.instance_id === expectedInstanceId) return;
        throw new Error(
          `port ${port} is held by a different instance (expected ${expectedInstanceId}, got ${body.instance_id ?? "none"}) - refusing to proceed against an unrelated runtime`,
        );
      }
    } catch (error) {
      if (error instanceof Error && error.message.startsWith("port ")) throw error;
      // Connection refused / not up yet - keep waiting.
    }
    if (Date.now() >= deadline) {
      throw new Error(`orphan debug_runtime on port ${port} never became healthy`);
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

/** Kill our directly-spawned orphan by pid (belt-and-suspenders cleanup). */
function killOrphanProcessGroup(orphan: Orphan): void {
  if (orphan.child.pid === undefined) return;
  try {
    process.kill(-orphan.child.pid, "SIGKILL");
  } catch {
    // Already gone.
  }
}

test(
  "launch() reaps a genuinely orphaned debug_runtime and rebinds its exact port",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    const repoRoot = findRepoRoot(process.cwd());
    assert.ok(repoRoot, "expected to find the cargo workspace root");

    const orphan = spawnOrphan(PORT, repoRoot);
    let successor: GameServer | undefined;
    try {
      await waitUntilOwnedHealthy(PORT, orphan.instanceId, 300_000);

      // Default reapStale: true should kill the orphan and reclaim PORT
      // exactly, rather than the pre-existing find-a-free-port walk-up
      // landing on PORT + 1.
      successor = await GameServer.launch({ mission: "medsci1.mis", port: PORT });
      assert.equal(
        new URL(successor.baseUrl).port,
        String(PORT),
        "expected the successor to reclaim the exact orphaned port, not walk up",
      );
      await successor.info(); // confirm it's genuinely up, not just bound
    } finally {
      await successor?.shutdown();
      // Belt-and-suspenders: if the assertion above failed (reap didn't
      // work), don't leave the orphan running on the dev machine.
      killOrphanProcessGroup(orphan);
      await reapStaleRuntimeOnPort(PORT);
    }
  },
);

test(
  "launch() with reapStale: false leaves an orphan running and walks up to a free port",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    const repoRoot = findRepoRoot(process.cwd());
    assert.ok(repoRoot, "expected to find the cargo workspace root");

    const orphanPort = PORT + 10;
    const orphan = spawnOrphan(orphanPort, repoRoot);
    let successor: GameServer | undefined;
    try {
      await waitUntilOwnedHealthy(orphanPort, orphan.instanceId, 300_000);

      successor = await GameServer.launch({
        mission: "medsci1.mis",
        port: orphanPort,
        reapStale: false,
      });
      assert.notEqual(
        new URL(successor.baseUrl).port,
        String(orphanPort),
        "reapStale: false must not touch the orphan - launch should walk up instead",
      );
      // The orphan itself must still be alive and still OUR instance.
      const stillUp = await fetch(`http://127.0.0.1:${orphanPort}/v1/health`);
      assert.ok(stillUp.ok, "the orphan should be untouched and still serving");
      const body = (await stillUp.json()) as { instance_id?: string | null };
      assert.equal(body.instance_id, orphan.instanceId, "the untouched orphan should still be ours");
    } finally {
      await successor?.shutdown();
      killOrphanProcessGroup(orphan);
      await reapStaleRuntimeOnPort(orphanPort);
    }
  },
);
