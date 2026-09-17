import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  collisionSoundsSince,
  describeSounds as describe,
  tagValue,
} from "./helpers/audio.js";

// Flat melee landed damage without a sound. `HeldMeleeWeapon` - the only code
// that emits a melee impact sound - runs on real VR contacts and returns
// nothing when the presentation is not VR, and the flat path resolves its hit
// by raycast, so a swing that connected was silent against everything.
//
// The staging mirrors flat-melee-animation-event.e2e.test.ts: Earth's
// stationary Training Droid, hit with the shipped player Wrench through
// production inventory selection. The droid inherits `Robots` (`Material
// MetalTarget`), so the blow resolves the wrench-on-metal-target schema
// (`hwremet*`); a hybrid's `FleshTarget` resolves `hwrefle*` off the same
// query, since the material tag comes from whatever was struck.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TRAINING_DROID = 593;
const WRENCH = -928;

test(
  "a flat melee swing that lands plays the weapon's impact schema",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 5 });

    await game.player.spawnItem(WRENCH);
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 5 });
    const [droid] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(droid, "Earth should contain its authored Training Droid");

    const [x, y, z] = (await game.entities.detail(droid.id)).position;
    await game.player.teleport({ x: x + 1.2, y: y + 1, z });
    await game.step({ frames: 60 });
    const aim = await game.player.aimAt(droid, {
      hitbox: "torso",
      visibility: "required",
    });
    assert.equal(aim.entity_id, droid.id);
    await game.step({ frames: 3 });

    // The swing's authored impact event (MF_TRIGGER1) lands 41 simulation
    // frames after the trigger edge - see flat-melee-animation-event.
    const beforeSwing = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.trigger", 0);

    const played = (await game.audio.recent()).sounds;
    const impacts = collisionSoundsSince(played, beforeSwing).filter(
      (sound) => tagValue(sound, "weapontype") === "wrench",
    );
    assert.ok(
      impacts.length > 0,
      `a landed flat swing should play an impact sound: ${describe(played)}`,
    );
    assert.ok(
      impacts.some((sound) => tagValue(sound, "material") === "metaltarget"),
      `a droid is MetalTarget, so the blow should resolve hwremet*: ${describe(impacts)}`,
    );
    // One authored swing event is one blow, so one thud - not a stream.
    assert.ok(
      impacts.length <= 2,
      `one swing should not machine-gun impact sounds: ${describe(impacts)}`,
    );
  },
);
