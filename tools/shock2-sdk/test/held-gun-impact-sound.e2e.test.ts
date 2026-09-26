import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { collisionSoundsSince, tagValue } from "./helpers/audio.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// A gun held under `--experimental physical_held_items` is stopped by the level
// but takes part in no collision at all (`CollisionGroup::held_inert`), so the
// narrow phase has nothing to say about it: pushing it into a bulkhead was
// completely silent, where a wrench thuds. The block the held-item sweep finds
// is now reported as a collision instead, and this is the production wiring for
// that - a real mission, the real VR grab, the real drive.
//
// Stock guns have no collision schema. The authored gun query is preferred;
// the material-sensitive wrench impact supplies the VR fallback.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const PISTOL = -17;

// Push the held weapon down through the floor line, advancing the tracked hand
// one simulation frame at a time like a real controller sample. The target ends
// well below the floor: the gun's collider is small and sits near the grip, so
// a hand stopping at ankle height never brings it into contact with anything.
async function pushThroughTheFloor(
  game: GameServer,
  frames = 90,
): Promise<void> {
  const start: Vec3 = [0.0, 1.5, 0.6];
  const end: Vec3 = [0.0, -2.2, 0.6];
  for (let frame = 1; frame <= frames; frame += 1) {
    const t = frame / frames;
    const hand: Vec3 = [start[0], start[1] + (end[1] - start[1]) * t, start[2]];
    await game.input.set("right_hand.position", hand);
    await game.step({ frames: 1 });
  }
}

test(
  "a physically-held gun reports the level block that stops it, exactly once",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr", "--experimental", "physical_held_items"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    // In VR `wield` is a no-op, so DebugCycleWeapon drops the roster's first
    // weapon - the Pistol - in front of the player as a world pickup.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });
    const pistol = (await game.entities.list()).entities.find(
      (e) => e.template_id === PISTOL,
    );
    assert.ok(pistol, "DebugCycleWeapon must spawn the Pistol");

    await aimVrHandAt(game, pistol.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol.id,
      "the VR right hand must hold the pistol",
    );

    // The flag's own precondition: held, and in no collision group at all.
    // Without this the rest of the test would be about nothing.
    const held = (await game.physics.bodies({ entityId: pistol.id })).bodies[0];
    assert.ok(held, "the held gun must have a body under physical_held_items");
    assert.deepEqual(held.collision_groups, [], "the held gun must be inert");

    // Push it down through the floor. The trigger stays UP - a gun meeting a
    // bulkhead is not an attack.
    await game.input.set("right_hand.position", [0.0, 1.5, 0.6]);
    await game.step({ frames: 20 });
    const beforePush = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await pushThroughTheFloor(game);

    const blockedY = (await game.physics.bodies({ entityId: pistol.id }))
      .bodies[0]?.position[1];
    assert.ok(blockedY !== undefined, "the held gun kept its body");
    const handY = (await game.info()).player.position[1] - 2.2;
    assert.ok(
      blockedY > handY + 0.5,
      `the level should have held the gun back: gun y=${blockedY}, hand y=${handY}`,
    );

    const impacts = () =>
      game.audio
        .recent()
        .then((recent) =>
          collisionSoundsSince(recent.sounds, beforePush).filter(
            (sound) => tagValue(sound, "weapontype") === "wrench",
          ),
        );
    assert.equal(
      (await impacts()).length,
      1,
      "one resolved metal-weapon impact must play",
    );
    await game.step({ frames: 120 });
    assert.equal(
      (await impacts()).length,
      1,
      "resting contact must remain silent",
    );
    await game.input.set("right_hand.position", [0.0, 1.5, 0.6]);
    await game.step({ frames: 60 });
    await pushThroughTheFloor(game);
    assert.equal(
      (await impacts()).length,
      2,
      "a second deliberate tap must play again",
    );
  },
);
