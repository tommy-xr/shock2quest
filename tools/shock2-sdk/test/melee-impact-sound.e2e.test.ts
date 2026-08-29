import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";
import {
  collisionSoundsSince,
  describeSounds as describe,
  tagValue,
} from "./helpers/audio.js";

// A VR melee contact with something that takes no authored damage - a wall, a
// bench, a crate - used to be completely silent, because the impact sound was
// emitted only from the damage path. It also checks the surface the contact
// names: the material comes from the level trimesh triangle the manifold
// touched, so a deck sounds like its own material rather than the default. This exercises the production path on a
// real mission's static world geometry (not a debug scene's synthetic
// collider) and observes the played-sound log, the only headless way to hear
// anything.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const WRENCH_MISSION_ID = 786;

async function byMissionId(
  game: GameServer,
  name: string,
  missionId: number,
): Promise<EntitySummary> {
  const entity = (
    await game.entities.list({ filter: name, limit: 20 })
  ).entities.find((candidate) => candidate.template_id === missionId);
  assert.ok(entity, `expected ${name} mission object ${missionId}`);
  return entity;
}

// Swing the held weapon down into the floor, advancing the tracked hand target
// one simulation frame at a time like a real controller sample.
async function swingIntoTheFloor(game: GameServer, frames = 24): Promise<void> {
  const start: Vec3 = [0.0, 1.5, 0.6];
  const end: Vec3 = [0.0, 0.1, 0.6];
  for (let frame = 1; frame <= frames; frame += 1) {
    const t = frame / frames;
    const hand: Vec3 = [start[0], start[1] + (end[1] - start[1]) * t, start[2]];
    await game.input.set("right_hand.position", hand);
    await game.step({ frames: 1 });
  }
}

test(
  "a VR melee contact with world geometry plays an impact sound, without flooding",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    // Pick the shipped Wrench up through the production VR grab (the same
    // staging vr-melee-contact.e2e.test.ts uses).
    const wrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);
    await game.player.teleport({
      x: wrench.position[0],
      y: wrench.position[1] - 1.4,
      z: wrench.position[2] + 1.2,
    });
    await game.input.lookAtWorldPoint(wrench.position);
    await game.input.set("right_hand.position", [0, 1.4, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.id,
      "the shipped Wrench should be grabbed",
    );

    // Swing at the floor - bare world geometry, which takes no authored
    // contact damage and so used to be silent. The trigger stays UP: hitting a
    // wall makes a noise whether or not it is a billable attack.
    await game.input.set("right_hand.position", [0.0, 1.5, 0.6]);
    await game.step({ frames: 20 });
    const beforeSwing = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await swingIntoTheFloor(game);

    const afterSwing = (await game.audio.recent()).sounds;
    // Scope to the held Wrench's own schema: other loose melee props in the
    // room are jostled by the staging and legitimately clink too.
    const impacts = collisionSoundsSince(afterSwing, beforeSwing).filter(
      (sound) => tagValue(sound, "weapontype") === "wrench",
    );
    assert.ok(
      impacts.length > 0,
      `hitting world geometry should play a collision sound: ${describe(afterSwing)}`,
    );
    // The floor names its own material now that world geometry carries one
    // (command2's deck is plasticrete -> `hwrepla*`, where every surface used
    // to resolve the blanket metal schema).
    assert.ok(
      impacts.some((sound) => tagValue(sound, "material") === "plasticrete"),
      `the deck should resolve its own material schema: ${describe(impacts)}`,
    );
    // The swing crosses the floor over ~0.4 s; the per-surface cooldown caps
    // that at a couple of thuds rather than one per contact frame.
    assert.ok(
      impacts.length <= 4,
      `one swing should not machine-gun impact sounds: ${describe(impacts)}`,
    );

    // The weapon now rests on the floor, re-contacting every frame. The speed
    // floor must keep it silent.
    const beforeRest = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.step({ frames: 120 });
    const whileResting = collisionSoundsSince(
      (await game.audio.recent()).sounds,
      beforeRest,
    );
    assert.equal(
      whileResting.length,
      0,
      `a weapon resting on a surface must be silent: ${describe(whileResting)}`,
    );
  },
);
