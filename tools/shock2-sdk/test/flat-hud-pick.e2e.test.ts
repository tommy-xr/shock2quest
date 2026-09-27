import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "flat reticle picks an undamaged non-frobbable hybrid beyond use reach",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 2 });
    await game.player.teleport({ x: -3, y: 1, z: 0 });
    await game.step({ frames: 10 });
    const hybrid = (await game.entities.byTemplate(-397)).sort(
      (a, b) => b.position[0] - a.position[0],
    )[0];
    assert.ok(hybrid, "the scene must contain its authored hybrid target");
    await game.player.aimAt(hybrid, { hitbox: "torso" });
    await game.step({ frames: 1 });

    // The mini-frame names the same world pick as the selection overlay.
    // Centering the pointer over the world avoids an inventory/UI override.
    await game.input.trigger("ToggleUseMode");
    await game.input.set("pointer.position", [0.5, 0.5]);
    await game.player.aimAt(hybrid, { hitbox: "torso" });
    await game.step({ frames: 2 });
    const aim = await game.player.aimAt(hybrid, { hitbox: "torso" });
    assert.ok(aim.visibility.target_distance > 3, "target must remain beyond retail frob reach");
    assert.equal((await game.ui.state()).name_strip, "A hybrid");
  },
);
