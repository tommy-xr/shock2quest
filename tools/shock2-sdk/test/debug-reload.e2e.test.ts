import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const mission of ["debug_gloves", "debug_minimal"]) {
  test(
    `${mission}: reload resets the scene with shipping transition behavior`,
    { skip: !enabled, timeout: 300_000 },
    async () => {
      await using game = await GameServer.launch({
        mission,
        // Exercise Game's reload dispatch, not the debug runtime's usual
        // immediate-transition shortcut. Quest exposed this parser crash.
        debugFlags: ["--vr", "--defer-transitions"],
      });
      await game.step({ frames: 2 });
      const spawn = await game.player.position();
      for (let cycle = 0; cycle < 3; cycle++) {
        await game.player.teleport({
          x: spawn.x + 4,
          y: spawn.y,
          z: spawn.z + 3,
        });
        await game.input.trigger("DebugReloadLevel");
        await game.step({ frames: 5 });
        assert.equal((await game.info()).mission, mission);
        const position = await game.player.position();
        assert.ok(Math.abs(position.x - spawn.x) < 0.01);
        assert.ok(Math.abs(position.z - spawn.z) < 0.01);
        // Continue beyond the deferred loader's checkpoints: a parser-thread
        // panic must not be hidden by checking only its first loading frame.
        await game.step({ frames: 90 });
        assert.equal((await game.info()).mission, mission);
      }
    },
  );
}
