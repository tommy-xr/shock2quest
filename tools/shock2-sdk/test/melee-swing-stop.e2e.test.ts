import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// A swing is stopped by the limb it strikes - not by the actor capsule around
// the creature, which is a movement volume wider than the creature.
//
// The stop is also what *reports* the blow. Clamped at the limb's surface, the
// weapon never generates a narrow-phase contact while still moving, so the old
// contact-driven rule scored nothing: the shape-cast that stopped the swing is
// the thing that knows which limb, where, and how fast.
const SWING_FRAMES = 30;
const SWING_FROM_X = 0.9;
const SWING_TO_X = -1.7;

async function weaponX(game: GameServer, weaponId: number): Promise<number> {
  const { bodies } = await game.physics.bodies({ entityId: weaponId });
  assert.ok(bodies[0], "the held weapon should have a body");
  return bodies[0].position[0];
}

test(
  "a swing stops on the limb it strikes, and that stop is the blow",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Grab the authored Wrench off the rack.
    const { player } = await game.info();
    const wrench = (await game.entities.list()).entities.find(
      (entity) => entity.name === "Wrench",
    );
    assert.ok(wrench, "expected a Wrench in debug_melee");
    await game.input.set("right_hand.position", [
      wrench.position[0] - player.position[0],
      wrench.position[1] - player.position[1],
      wrench.position[2] - player.position[2],
    ]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 10 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 15 });

    let target;
    for (const entity of (await game.entities.list()).entities) {
      const detail = await game.entities.detail(entity.id);
      if ((detail.aim_points ?? []).length > 1) {
        target = detail;
        break;
      }
    }
    assert.ok(target, "expected a creature with hitboxes in debug_melee");
    const [tx, ty, tz] = target.position;
    await game.player.teleport({ x: tx + 2.0, y: ty - 1.0, z: tz });
    await game.step({ frames: 20 });
    const { player: stance } = await game.info();

    // Swing through the creature. The hand goes all the way; the weapon
    // should not.
    let handX = 0;
    for (let frame = 1; frame <= SWING_FRAMES; frame += 1) {
      const t = frame / SWING_FRAMES;
      handX = SWING_FROM_X + (SWING_TO_X - SWING_FROM_X) * t;
      await game.input.set("right_hand.position", [
        handX,
        ty + 0.35 - stance.position[1],
        0,
      ]);
      await game.step({ frames: 1 });
    }

    // Negative-first: with the sweep passing through limbs the weapon tracks
    // the hand exactly (measured lag 0.00 for the whole swing).
    const stoppedAt = await weaponX(game, wrench.id);
    const separation = Math.abs(stance.position[0] + handX - stoppedAt);
    assert.ok(
      separation > 0.5,
      `the swing should have been stopped by the creature; weapon is ${separation.toFixed(2)} behind the hand`,
    );

    // ...and the stop is the blow: it names the limb it struck.
    const damage = (await game.messages.recent()).messages.filter(
      (message) => message.payload === "Damage" && message.to.entity_id === target.entity_id,
    );
    assert.ok(
      damage.length > 0,
      "stopping on the limb should have damaged the creature",
    );
    assert.ok(
      damage.every(
        (message) => message.impact?.bone !== undefined && message.impact?.bone !== null,
      ),
      `each blow should name its joint; got ${JSON.stringify(damage.map((d) => d.impact))}`,
    );

    // Pulling the hand back frees the weapon: a stop must not be a trap.
    for (let frame = 1; frame <= 24; frame += 1) {
      const t = frame / 24;
      await game.input.set("right_hand.position", [
        SWING_TO_X + (1.0 - SWING_TO_X) * t,
        ty + 0.35 - stance.position[1],
        0,
      ]);
      await game.step({ frames: 1 });
    }
    const recovered = Math.abs(stance.position[0] + 1.0 - (await weaponX(game, wrench.id)));
    assert.ok(
      recovered < 0.2,
      `the weapon should follow the hand back out, got ${recovered.toFixed(2)} behind`,
    );
  },
);
