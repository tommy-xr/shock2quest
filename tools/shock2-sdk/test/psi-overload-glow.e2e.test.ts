import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { equipRightHand } from "./helpers/vr-hand.js";

// An overload cast heats the amp's device parts red for the meter's 0.6s
// result flash - never the baked hand sharing its atlas; a normal cast does
// not. Cryokinesis (the default) is tier 1:
// a 120-frame bar with the overload zone from 85%.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) test(
  `overload cast heats the amp, not the hand (${vr ? "VR" : "flat"})`,
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 10 });
    const [amp] = await game.entities.byTemplate(-247);
    await equipRightHand(game, vr, amp.id, "EquipPsiAmp");
    const parts = async () => (await game.scene.objects({ entityId: amp.id })).objects;
    const glows = async () => (await parts()).map((o) => o.glow);

    // A quick click casts normally: no glow.
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
    const normal = await glows();
    assert.ok(normal.length > 0, "the amp is drawn");
    assert.ok(normal.every((g) => g === null), `normal cast: ${JSON.stringify(normal)}`);
    await game.step({ frames: 60 });

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 110 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
    const flash = await parts();
    const atlas = flash.find((o) => o.material_name === "ND-amp_h.psd");
    // The 25AE atlas's skin lies left of u=0.68 (checked against the art, not
    // here): the shared atlas is limited to the device to its right.
    assert.ok(Math.abs(atlas!.glow!.uv_rect[0] - 0.68) < 1e-6, JSON.stringify(atlas?.glow));
    assert.ok(
      flash.every(({ glow: g }) => g !== null && g.color[0] > 0.5 && g.color[0] > 3 * g.color[1]),
      `every part glows red-hot: ${JSON.stringify(flash.map((o) => o.glow))}`,
    );
    await game.step({ frames: 40 });
    assert.ok((await glows()).every((g) => g === null), "the flash fades out");
  },
);
