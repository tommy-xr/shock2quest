import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { equipRightHand } from "./helpers/vr-hand.js";

// A psi burnout flashes the view's rim white, plays retail's burnout sound and
// kicks the amp up out of the hand, in both presentations, then all of it
// settles. Cryokinesis (the default) is
// tier 1: holding past its 120-frame bar burns out.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) test(
  `psi burnout flashes the rim, plays its sound and kicks the amp (${vr ? "VR" : "flat"})`,
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      // Quest's held-gun physics, which carries VR recoil.
      debugFlags: vr ? ["--vr", "--experimental", "physical_held_items"] : [],
    });
    await game.step({ frames: 10 });
    const [amp] = await game.entities.byTemplate(-247);
    await equipRightHand(game, vr, amp.id, "EquipPsiAmp");
    const flash = async () => (await game.scene.fromSource("psi_burnout"))[0];
    const muzzle = async () => {
      const detail = await game.entities.detail(amp.id);
      const frame = detail.properties.find((p) => p.name === "WeaponMuzzle");
      assert.ok(frame, "the held amp exposes its muzzle frame");
      return (JSON.parse(frame.value) as { forward: number[] }).forward;
    };
    // Degrees the muzzle has turned from its resting direction.
    const turned = async (rest: number[]) => {
      const now = await muzzle();
      const cos = now.reduce((sum, v, i) => sum + v * rest[i], 0);
      return (Math.acos(Math.min(1, cos)) * 180) / Math.PI;
    };
    const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 118 });
    assert.equal(await flash(), undefined, "no flash before the burnout");
    const rest = await muzzle();
    await game.step({ frames: 4 });
    assert.equal((await game.info()).player.psi_charge_phase, "burnout");
    assert.ok(await flash(), "the rim flashes on burnout");
    const sounds = (await game.audio.recent()).sounds.filter((s) => s.sequence > before);
    assert.ok(
      sounds.some((s) => s.sample.toLowerCase() === "psiburno"),
      `burnout sound: ${JSON.stringify(sounds.map((s) => s.sample))}`,
    );
    await game.step({ frames: 6 });
    const kicked = await turned(rest);
    assert.ok(kicked > 8, `the amp kicks: ${kicked} degrees`);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 30 });
    assert.equal(await flash(), undefined, "the flash fades");
    await game.step({ frames: 120 });
    const settled = await turned(rest);
    assert.ok(settled < 1, `the amp settles: ${settled} degrees`);
  },
);
