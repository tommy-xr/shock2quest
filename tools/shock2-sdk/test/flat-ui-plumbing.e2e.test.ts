import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the flat-UI plumbing (projects/flat-ui.md PR 1):
// - GET /v1/ui reports the flat UI mode ("shooter" by default)
// - InputAction::ToggleUseMode flips it to "use" and back (the original
//   game's Tab metagame mode - mode tracking only at this stage)
// - the pointer.position / pointer.pressed input channels are accepted and
//   echoed by GET /v1/control/input (the cursor input the flat panels will
//   consume)
//
// Negative-first: before this plumbing, /v1/ui 404s and the pointer channels
// are rejected as unknown.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "flat UI plumbing: /v1/ui mode toggle + pointer channels",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8111),
    });
    await game.step({ frames: 2 });

    // Default mode is shooter.
    const initial = await game.ui.state();
    assert.equal(initial.mode, "shooter", "flat UI should start in shooter mode");

    // Tab (ToggleUseMode) flips to use mode...
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
    const afterToggle = await game.ui.state();
    assert.equal(afterToggle.mode, "use", "ToggleUseMode should enter use mode");

    // ...and back to shooter.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
    const afterSecondToggle = await game.ui.state();
    assert.equal(
      afterSecondToggle.mode,
      "shooter",
      "a second ToggleUseMode should restore shooter mode",
    );

    // Pointer channels are accepted and echoed.
    await game.input.set("pointer.position", [0.5, 0.25]);
    await game.input.set("pointer.pressed", 1);
    const echoed = (await (
      await fetch(`${game.baseUrl}/v1/control/input`)
    ).json()) as {
      pointer: { position: [number, number]; pressed: boolean } | null;
    };
    assert.ok(echoed.pointer, "pointer state should be echoed once set");
    assert.deepEqual(echoed.pointer.position, [0.5, 0.25]);
    assert.equal(echoed.pointer.pressed, true);

    await game.input.set("pointer.pressed", 0);
    const released = (await (
      await fetch(`${game.baseUrl}/v1/control/input`)
    ).json()) as { pointer: { pressed: boolean } | null };
    assert.equal(released.pointer?.pressed, false);
  },
);
