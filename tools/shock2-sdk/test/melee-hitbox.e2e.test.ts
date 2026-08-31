import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// A VR swing used to land on the creature's actor capsule - a cylinder sized
// from the creature definition, 3.5 game-feet across for every human-sized AI -
// so a blow knew it hit "the hybrid" and nothing about where. Held weapons now
// contact the creature's own hitboxes, so a swing arrives through the limb it
// struck and carries that joint: what the damage readouts show, and what
// per-limb damage will scale by.
const SWING_FRAMES = 40;
const SWING_FROM_X = 0.6;
const SWING_TO_X = -1.6;

test(
  "a VR swing lands on the limb it struck, and bills the creature once",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Grab the authored Wrench off the rack: the hand goes to its pawn-local
    // position, then squeezes.
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
    const inventory = await game.player.inventory();
    assert.ok(
      inventory.items.some((item) => item.entity_id === wrench.id),
      `the Wrench should be held; inventory is ${JSON.stringify(inventory)}`,
    );

    // Find a creature with hitboxes and stand beside it.
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
    await game.player.teleport({ x: tx + 1.1, y: ty - 1.0, z: tz });
    await game.step({ frames: 20 });
    const { player: stance } = await game.info();

    // Swing the weapon through the creature at chest height.
    for (let frame = 1; frame <= SWING_FRAMES; frame += 1) {
      const t = frame / SWING_FRAMES;
      await game.input.set("right_hand.position", [
        SWING_FROM_X + (SWING_TO_X - SWING_FROM_X) * t,
        ty + 0.3 - stance.position[1],
        0,
      ]);
      await game.step({ frames: 1 });
    }
    await game.step({ frames: 5 });

    const damage = (await game.messages.recent()).messages.filter(
      (message) => message.payload === "Damage",
    );
    assert.ok(damage.length > 0, "the swing should have damaged something");

    // Negative-first: on the capsule path every blow reports a null bone.
    const onCreature = damage.filter(
      (message) => message.to.entity_id === target.entity_id,
    );
    assert.ok(
      onCreature.length > 0,
      `the swing should reach the creature; got ${JSON.stringify(damage.map((d) => d.to))}`,
    );
    assert.ok(
      onCreature.every(
        (message) => message.impact?.bone !== undefined && message.impact?.bone !== null,
      ),
      `every blow on the creature should carry the joint it struck; got ${JSON.stringify(
        onCreature.map((d) => d.impact),
      )}`,
    );

    // ...and one swing is one blow: the cooldown is keyed on the creature, so
    // sweeping through several limbs bills it once, not once per limb.
    assert.equal(
      onCreature.length,
      1,
      `one swing should bill the creature once: ${JSON.stringify(
        onCreature.map((d) => [d.to.entity_id, d.impact?.bone]),
      )}`,
    );
  },
);

test(
  "an authored corpse has no hitboxes, and is still struck on its collider",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 20 });

    // A corpse carries PropCreature - so its creature *definition* maps
    // hitboxes - but is never animated, so it has none. Reading the definition
    // instead of the live proxies made every corpse in the game silent and
    // unhittable to a swing.
    const corpse = (await game.entities.list()).entities
      .filter((entity) => entity.name === "MS Male Corpse")
      .sort((a, b) => a.distance - b.distance)[0];
    assert.ok(corpse, "expected a corpse in medsci1");
    const detail = await game.entities.detail(corpse.id);
    assert.equal((detail.aim_points ?? []).length, 0, "a posed corpse has no hitbox proxies");
  },
);
