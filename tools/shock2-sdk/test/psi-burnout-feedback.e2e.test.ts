import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { equipRightHand } from "./helpers/vr-hand.js";

// A psi burnout flashes the view's rim white and plays retail's burnout sound,
// in both presentations, then the flash fades. Cryokinesis (the default) is
// tier 1: holding past its 120-frame bar burns out.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) test(
  `psi burnout flashes the rim white and plays its sound (${vr ? "VR" : "flat"})`,
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 10 });
    const [amp] = await game.entities.byTemplate(-247);
    await equipRightHand(game, vr, amp.id, "EquipPsiAmp");
    const flash = async () => (await game.scene.fromSource("psi_burnout"))[0];
    const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 118 });
    assert.equal(await flash(), undefined, "no flash before the burnout");
    await game.step({ frames: 4 });
    assert.equal((await game.info()).player.psi_charge_phase, "burnout");
    assert.ok(await flash(), "the rim flashes on burnout");
    const sounds = (await game.audio.recent()).sounds.filter((s) => s.sequence > before);
    assert.ok(
      sounds.some((s) => s.sample.toLowerCase() === "psiburno"),
      `burnout sound: ${JSON.stringify(sounds.map((s) => s.sample))}`,
    );
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 40 });
    assert.equal(await flash(), undefined, "the flash fades");
  },
);
