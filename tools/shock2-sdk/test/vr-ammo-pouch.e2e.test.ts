import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { quatConjugate, quatRotate, sub, type Quat } from "./helpers/vr-hand.js";
import { fireOnce } from "./helpers/weapon.js";
import {
  STD_CLIP,
  ammoOf,
  entityById,
  fireDry,
  grabPistol,
  insertClip,
  offHandEntityId,
  spawnClip,
  stackOf,
} from "./helpers/vr-clip.js";

// The right-hip ammo pouch (slice 8): with a gun in one hand, an empty grip at
// the other hip takes a clip for that gun's SELECTED ammo out of the real
// reserve, and opening the hand there puts it back. The clip then loads the gun
// through the existing insert gesture, unchanged.
//
// Negative-first: on the parent there is no pouch anchor at all, so
// `player.body_frame.pouch` does not exist, the hip claims nothing, and a grip
// there leaves the hand empty.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8666);

/** A world point in the hand channel's pawn-local space. */
function toPawnLocal(world: Vec3, pawnPosition: Vec3, pawnRotation: Quat): Vec3 {
  return quatRotate(quatConjugate(pawnRotation), sub(world, pawnPosition));
}

/** Put the LEFT hand exactly on the pouch the runtime reports this frame. */
async function reachIntoPouch(game: GameServer): Promise<void> {
  const info = await game.info();
  const pouch = info.player.body_frame?.pouch;
  assert.ok(pouch, "VR should report the ammo pouch on the body frame");
  await game.input.set(
    "left_hand.position",
    toPawnLocal(
      pouch.position as Vec3,
      info.player.position as Vec3,
      info.player.rotation as Quat,
    ),
  );
  await game.step({ frames: 4 });
}

/** Total reserve rounds the BACKPACK carries in `template` stacks. A held clip
 * is still a carried item, so the hands are excluded - the point of every
 * assertion here is that rounds moved between the reserve and a hand. */
async function reserveRounds(game: GameServer, template: number): Promise<number> {
  const items = (await game.player.inventory()).items;
  let rounds = 0;
  for (const item of items) {
    if (item.location !== "inventory") continue;
    const detail = await game.entities.detail(item.entity_id);
    if (detail.template_id !== template) continue;
    rounds += stackOf(detail);
  }
  return rounds;
}

test(
  "VR: the hip pouch hands out a clip for the gun you hold, and takes it back",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await grabPistol(game);

    // An empty reserve is an empty pouch: nothing drawn, and the hip refuses.
    const empty = (await game.info()).player.body_frame?.pouch;
    assert.ok(empty, "the pouch should be reported even while empty");
    assert.equal(empty.available, false, "no ammo carried means no clip on the hip");
    assert.equal(empty.clip_template, null);

    await reachIntoPouch(game);
    assert.equal(
      (await game.info()).player.hand_affordance.left,
      "Blocked",
      "a gun with nothing compatible in reserve should pre-light amber",
    );
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 5 });
    assert.equal(
      offHandEntityId(await game.info()),
      null,
      "an empty pouch must not fabricate a clip",
    );
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 5 });

    // Empty the gun so the reload has somewhere to put rounds, then stock the
    // reserve.
    const capacity = await fireDry(game, pistol.id, fireOnce);
    const clip = await spawnClip(game, STD_CLIP);
    assert.ok(
      clip.rounds >= capacity,
      `this scenario needs a clip that can fill the magazine (${clip.rounds} vs ${capacity})`,
    );
    const stocked = await reserveRounds(game, STD_CLIP);
    assert.equal(stocked, clip.rounds);

    const pouch = (await game.info()).player.body_frame?.pouch;
    assert.ok(pouch);
    assert.equal(pouch.available, true, "a compatible clip fills the pouch");
    assert.equal(pouch.clip_template, STD_CLIP);
    assert.equal(pouch.clip_rounds, clip.rounds);

    // Take it: an empty grip at the hip puts a clip in that hand.
    await reachIntoPouch(game);
    assert.equal(
      (await game.info()).player.hand_affordance.left,
      "Grabbable",
      "a stocked pouch should pre-light green",
    );
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 6 });
    const taken = offHandEntityId(await game.info());
    assert.ok(taken, "the grip should hand a clip to the free hand");
    assert.equal(
      await reserveRounds(game, STD_CLIP),
      0,
      "the clip comes out of the real reserve, not out of nowhere",
    );
    assert.equal(
      stackOf(await game.entities.detail(taken)),
      clip.rounds,
      "and it carries exactly the rounds the stack held",
    );
    assert.equal(
      (await game.info()).player.body_frame?.pouch.available,
      false,
      "the drained pouch should report itself empty",
    );

    // Carry it to the magazine: the existing insert gesture finishes the reload.
    await insertClip(game, [0, 0, 0], taken, pistol.id);
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      capacity,
      "the withdrawn clip should load the pistol",
    );
    assert.equal(
      await entityById(game, taken),
      undefined,
      "the consumed clip should be gone",
    );

    // Put one back: a clip released in the pouch returns to the reserve rather
    // than falling on the floor.
    const second = await spawnClip(game, STD_CLIP);
    // Open the hand first: the withdraw is a grip EDGE, and the hand is still
    // closed from carrying the last clip into the gun.
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 4 });
    await reachIntoPouch(game);
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 6 });
    const carried = offHandEntityId(await game.info());
    assert.ok(carried, "the restocked pouch should hand out another clip");
    assert.equal(
      await reserveRounds(game, STD_CLIP),
      0,
      "the second clip should come out of the reserve too",
    );

    await reachIntoPouch(game);
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 6 });
    assert.equal(
      offHandEntityId(await game.info()),
      null,
      "opening the hand at the pouch should give the clip up",
    );
    assert.equal(
      await reserveRounds(game, STD_CLIP),
      second.rounds,
      "and its rounds should be back in the reserve",
    );
  },
);
