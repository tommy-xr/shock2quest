import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { equipRightHand } from "./helpers/vr-hand.js";

// The amp's device parts heat in the overload zone - teal entering it, red
// toward burnout - and an overload release flares that colour over the
// meter's 0.6s result flash; never the baked hand sharing their atlas.
// Cryokinesis (the default) is tier 1: a 120-frame bar with the overload zone
// from 85% (frame 102).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) test(
  `the amp heats through the overload zone, not the hand (${vr ? "VR" : "flat"})`,
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

    type Glow = { color: [number, number, number]; uv_rect: number[] } | null;
    const red = (g: Glow) => g!.color[0];
    const blue = (g: Glow) => g!.color[2];
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 98 });
    assert.ok((await glows()).every((g) => g === null), "no heat below the zone");
    await game.step({ frames: 7 });
    const entering = await parts();
    const atlas = entering.find((o) => o.material_name === "ND-amp_h.psd");
    // The 25AE atlas's skin lies left of u=0.68 (checked against the art, not
    // here): the shared atlas is limited to the device to its right.
    assert.ok(Math.abs(atlas!.glow!.uv_rect[0] - 0.68) < 1e-6, JSON.stringify(atlas?.glow));
    assert.ok(
      entering.every(({ glow: g }) => g !== null && blue(g) > red(g)),
      `teal entering the zone: ${JSON.stringify(entering.map((o) => o.glow))}`,
    );
    await game.step({ frames: 12 });
    const late = (await glows())[0];
    assert.ok(red(late) > 2 * blue(late), `red near burnout: ${JSON.stringify(late)}`);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
    const flare = (await glows())[0];
    const green = (g: Glow) => g!.color[1];
    assert.ok(
      red(flare) > 1.1 * red(late) && green(flare) > 2 * green(late),
      `the release flares white-hot: ${JSON.stringify(late)} -> ${JSON.stringify(flare)}`,
    );
    await game.step({ frames: 40 });
    assert.ok((await glows()).every((g) => g === null), "the flash fades out");
  },
);
