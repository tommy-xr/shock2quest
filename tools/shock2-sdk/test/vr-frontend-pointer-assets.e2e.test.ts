import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test(
  "VR frontend hover uses the remaster point hand and a fading textured trail",
  { skip: !enabled, timeout: 120_000 },
  async () => {
    const root = process.env.DARK_ASSET_PATH;
    assert.ok(root && existsSync(join(root, "sshock2.kpf")));
    assert.ok(existsSync(join(root, "mods", "sshock2ee.kpf")));

    await using game = await GameServer.launch({
      mission: "main_menu",
      debugFlags: ["--vr"],
    });
    await game.input.set("left_hand.position", [0, -10, 0]);
    await game.input.set("right_hand.position", [-1.1, 0.9, 0.2]);
    await game.input.set("right_hand.rotation", [0.121373, 0.4217595, 0, 0.89854745]);
    await game.step({ frames: 8 });

    const on = await game.scene.fromSource("frontend_pointer");
    assert.ok(on.some((object) => object.model === "atek_h.bin"));
    assert.equal(
      on.filter((object) => object.transparency !== null && object.transparency > 0).length,
      12,
      "the hand aimed at the panel draws twelve translucent textured wisps",
    );
    assert.equal(
      on.filter((object) => object.model === null && object.transparency === null).length,
      1,
      "the active ray keeps one crisp hit dot",
    );

    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 5 });
    const off = await game.scene.fromSource("frontend_pointer");
    assert.ok(off.some((object) => object.model === "ar15_h.bin"));
    assert.ok(!off.some((object) => object.model === "atek_h.bin"));
    assert.equal(
      off.filter((object) => object.model === null && object.transparency !== null).length,
      0,
      "a ray off the panel must not leave wisps over unrelated menu entries",
    );
    assert.equal(
      off.filter((object) => object.model === null && object.transparency === null).length,
      0,
      "an off-panel ray must not promise a hover with a hit dot",
    );
  },
);
