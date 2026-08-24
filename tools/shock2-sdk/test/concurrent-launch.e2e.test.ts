import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Two launches at once, neither naming a port, must each get their own runtime.
//
// This is the case the SDK used to guard rather than prevent: it probed for a
// free port and only then let the child bind it, so a sibling launch (or any
// other process) could take the port in between. Now the child binds an
// OS-assigned port and reports it, so there is no window to lose.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "two concurrent launches with no port get separate runtimes",
  { skip: !e2eEnabled, timeout: 420_000 },
  async () => {
    // Genuinely simultaneous: both launches are in flight at once, which is
    // exactly when the old probe-then-bind window could be lost.
    const launches = await Promise.allSettled([
      GameServer.launch({ mission: "debug_minimal" }),
      GameServer.launch({ mission: "debug_minimal" }),
    ]);
    // allSettled, not all: if one launch fails the other still needs shutting
    // down, or the test leaks a runtime.
    const servers = launches
      .filter((result) => result.status === "fulfilled")
      .map((result) => result.value);

    try {
      const failures = launches.filter((result) => result.status === "rejected");
      assert.equal(
        failures.length,
        0,
        `both launches should succeed: ${failures.map((f) => f.reason).join("\n")}`,
      );
      const [first, second] = servers;
      assert.ok(first && second);

      const firstPort = Number(new URL(first.baseUrl).port);
      const secondPort = Number(new URL(second.baseUrl).port);
      assert.ok(firstPort, "first runtime should report a bound port");
      assert.ok(secondPort, "second runtime should report a bound port");
      assert.notEqual(firstPort, secondPort, "each runtime needs its own port");

      // The port came from the OS at bind time, not from the SDK guessing: the
      // old probe-then-bind path handed out 8080/8081 here, so this assertion
      // is the one that fails against it.
      for (const port of [firstPort, secondPort]) {
        assert.ok(
          port !== 8080 && port !== 8081,
          `expected an ephemeral port, got the old hardcoded default ${port}`,
        );
      }

      // ...and each client is talking to its OWN runtime, not the other one.
      const firstHealth = (await first.health()) as { instance_id?: string };
      const secondHealth = (await second.health()) as { instance_id?: string };
      assert.ok(firstHealth.instance_id, "health should echo the instance id");
      assert.notEqual(
        firstHealth.instance_id,
        secondHealth.instance_id,
        "the two clients must not be pointed at the same runtime",
      );

      // Both are genuinely live and independently steppable.
      const [firstStep, secondStep] = await Promise.all([
        first.step({ frames: 2 }),
        second.step({ frames: 2 }),
      ]);
      assert.equal(firstStep.frames_advanced, 2);
      assert.equal(secondStep.frames_advanced, 2);
    } finally {
      await Promise.all(servers.map((server) => server.shutdown()));
    }
  },
);
