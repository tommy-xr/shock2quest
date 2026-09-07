import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer, findRepoRoot } from "../src/index.js";
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
  async (t) => {
    // The quicksave leg writes save1.sav into the repo root; put it back
    // exactly as found so the suite stays order-independent.
    const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
    const quicksave = join(repoRoot, "save1.sav");
    const saved = existsSync(quicksave) ? readFileSync(quicksave) : null;
    t.after(() => {
      if (saved === null) rmSync(quicksave, { force: true });
      else writeFileSync(quicksave, saved);
    });

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
      const [gun] = (await game.entities.list({ filter: name })).entities;
      assert.ok(gun, `debug_weapons benches a ${name}`);
      const here = await game.info();
      await game.input.set(
        "right_hand.position",
        toPawnLocal(
          gun.position as Vec3,
          here.player.position as Vec3,
          here.player.rotation as Quat,
        ),
      );
      await game.step({ frames: 3 });
      await game.input.set("right_hand.squeeze", 1.0);
      await game.step({ frames: 5 });
      const model = (await game.info()).player.hand_affordance.right_model;
      assert.ok(model, `the squeeze should take the ${name} into the hand`);
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

    // The slot survives a save and a load.
    await game.input.trigger("QuickSave");
    await game.step({ frames: 30 });
    await game.input.trigger("QuickLoad");
    await game.step({ frames: 60 });
    assert.equal(
      (await game.info()).player.body_frame?.holster.occupied,
      "Pistol",
      "the holster should still be occupied after a quicksave/quickload",
    );

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
