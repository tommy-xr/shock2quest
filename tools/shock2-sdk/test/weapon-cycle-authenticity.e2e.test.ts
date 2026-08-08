import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "spawn-and-wield weapon cycling is explicitly debug-only",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8379),
    });
    await game.step({ frames: 5 });

    const actions = await game.input.actions();
    assert.ok(
      !actions.includes("CycleWeapon"),
      "the production-sounding CycleWeapon action must not expose a spawning cheat",
    );
    assert.ok(
      actions.includes("DebugCycleWeapon"),
      "the spawn-and-wield tool should be explicitly named as a debug action",
    );

    const before = await game.player.inventory();
    await assert.rejects(
      game.input.trigger("CycleWeapon"),
      /unknown action/i,
      "the legacy ambiguous action name should be rejected",
    );
    assert.deepEqual(
      await game.player.inventory(),
      before,
      "rejecting the ambiguous action must preserve the exact inventory",
    );

    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 2 });
    const after = await game.player.inventory();
    assert.equal(after.count, before.count + 1);
    assert.equal(after.items.at(-1)?.name, "Pistol");
  },
);
