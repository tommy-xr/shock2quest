import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { quatConjugate, quatRotate, sub, type Quat } from "./helpers/vr-hand.js";

// The hip holster: one weapon rides the dominant thigh. Opening a hand full of
// weapon there docks it, closing an empty hand there draws it back - the same
// entity, with the magazine it went in with. A second weapon is refused.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** A world point in the hand channel's pawn-local space. */
function toPawnLocal(world: Vec3, pawnPosition: Vec3, pawnRotation: Quat): Vec3 {
  return quatRotate(quatConjugate(pawnRotation), sub(world, pawnPosition));
}

test(
  "VR: the hip holster takes one weapon, keeps it, and gives it back",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    const start = await game.info();
    const frame = start.player.body_frame;
    assert.ok(frame, "VR should report the player's body anchors");
    assert.ok(
      frame.holster.position[1] < frame.pouch.position[1],
      `the holster (${frame.holster.position[1]}) should hang below the hip (${frame.pouch.position[1]})`,
    );
    assert.equal(frame.holster.occupied, null, "the holster starts empty");

    /** The live magazine count of a gun entity. */
    const magazine = async (entityId: number) => {
      const detail = await game.entities.detail(entityId);
      return detail.properties.find((p) => p.name === "Ammo")?.value ?? null;
    };

    /** Take the named bench gun into the right hand; returns its model name. */
    const takeFromBench = async (name: string) => {
      // Exact name: the bench also carries a "Laser Pistol", which a substring
      // filter on "Pistol" would hand back instead.
      const gun = (await game.entities.list({ filter: name })).entities.find(
        (entity) => entity.name === name,
      );
      assert.ok(gun, `debug_weapons benches a ${name}`);
      const here = await game.info();
      // Reach from directly above, pointing down (a -90 deg pitch takes the
      // hand's own -Z forward onto -Y): the bench guns lie side by side at one
      // height, so a level ray can cross a neighbour, while a ray straight down
      // can only find the gun it is over.
      await game.input.set(
        "right_hand.position",
        toPawnLocal(
          [gun.position[0], gun.position[1] + 0.25, gun.position[2]],
          here.player.position as Vec3,
          here.player.rotation as Quat,
        ),
      );
      await game.input.set("right_hand.rotation", [-0.70711, 0, 0, 0.70711]);
      // Long enough for the anchor hysteresis to expire: a hand that has just
      // left a body anchor keeps it for ~12 frames, and a grip inside that
      // window would still belong to the anchor rather than to the bench.
      await game.step({ frames: 20 });
      await game.input.set("right_hand.squeeze", 1.0);
      await game.step({ frames: 5 });
      const took = await game.info();
      assert.equal(
        took.player.right_hand_entity_id,
        gun.id,
        `the squeeze should take the ${name} into the hand`,
      );
      const model = took.player.hand_affordance.right_model;
      assert.ok(model, `the held ${name} should report a model`);
      return model;
    };

    /** Put the right hand at the holster, in this frame's pawn space. */
    const handToHolster = async () => {
      const here = await game.info();
      const holster = here.player.body_frame?.holster.position as Vec3;
      await game.input.set(
        "right_hand.position",
        toPawnLocal(
          holster,
          here.player.position as Vec3,
          here.player.rotation as Quat,
        ),
      );
      await game.step({ frames: 5 });
      return holster;
    };

    const pistolModel = await takeFromBench("Pistol");
    const pistolId = (await game.info()).player.right_hand_entity_id;
    assert.ok(pistolId, "the held pistol should report an entity");
    const magazineBefore = await magazine(pistolId);

    // At the holster with a weapon in hand: green, because the slot is empty.
    await handToHolster();
    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Grabbable",
      "an empty holster should show that opening the hand will dock the pistol",
    );

    // Open the hand: the pistol leaves it and rides the thigh.
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 5 });
    const docked = await game.info();
    assert.equal(
      docked.player.hand_affordance.right_model,
      null,
      "the hand should be empty after the dock",
    );
    assert.equal(
      docked.player.body_frame?.holster.occupied,
      "Pistol",
      "the holster should be holding the pistol",
    );

    // And it is drawn there: a hand-path draw within reach of the anchor.
    const anchor = docked.player.body_frame!.holster.position as Vec3;
    const worn = (await game.scene.fromSource("player_hands")).filter(
      (object) =>
        Math.hypot(...anchor.map((c, i) => c - object.position[i])) < 0.4,
    );
    assert.ok(
      worn.length > 0,
      `the holstered pistol should be drawn at ${JSON.stringify(anchor)}`,
    );

    // A second weapon is refused: the slot holds one.
    const shotgunModel = await takeFromBench("Shotgun");
    assert.notEqual(shotgunModel, pistolModel, "the shotgun is a second weapon");
    await handToHolster();
    const refused = await game.info();
    assert.equal(
      refused.player.hand_affordance.right,
      "Blocked",
      "an occupied holster should refuse a second weapon",
    );
    assert.equal(
      refused.player.body_frame?.holster.occupied,
      "Pistol",
      "and it should still be the pistol on the thigh",
    );

    // Let the shotgun go away from the body, then confirm the refusal stood:
    // the holster is unchanged and the shotgun never entered it.
    await game.input.set("right_hand.position", [0.0, 0.0, -1.5]);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 10 });
    assert.equal(
      (await game.info()).player.body_frame?.holster.occupied,
      "Pistol",
      "the refused shotgun must not have displaced the holstered pistol",
    );

    // (The save/load leg is the `quest_info` unit round-trip instead: QuickLoad
    // panics in any debug scene, including this one - issue #1398.)

    // An empty grip at the thigh draws the pistol back into that hand, with
    // the magazine it went in with.
    await handToHolster();
    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Grabbable",
      "an occupied holster should offer its weapon to an empty hand",
    );
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });
    const drew = await game.info();
    assert.equal(
      drew.player.hand_affordance.right_model,
      pistolModel,
      "the grip should draw the holstered pistol",
    );
    assert.equal(
      drew.player.body_frame?.holster.occupied,
      null,
      "and the slot should be empty again",
    );
    assert.ok(drew.player.right_hand_entity_id, "the pistol is in the hand");
    assert.equal(
      await magazine(drew.player.right_hand_entity_id),
      magazineBefore,
      "the drawn pistol should carry the magazine it was holstered with",
    );
  },
);
