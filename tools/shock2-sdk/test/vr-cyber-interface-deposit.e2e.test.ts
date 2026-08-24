import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// Releasing a held item while the VR cyber interface is open used to drop it
// on the floor wherever the hand was pointing - including straight through the
// inventory strip, which is a head-anchored UI quad with no collider, so
// `VirtualHand`'s release raycast never sees it. A release over the strip is
// now the deposit the player meant.
//
// Negative-first: on the parent the first scenario fails at "the clip must be
// in the backpack" - the clip is a loose world prop on the floor instead, with
// a physics body and no Contains link.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8619);

/** Stable earth.mis mission object: the Weapons Training standard clip, a
 * loose grabbable world prop (see flat-world-pickup.e2e.test.ts). */
const CLIP_OBJ = 249;

/** The middle of the top-docked inventory strip - anchored at (2, 0) and
 * 636x121 on the shared 640x480 canvas, so this is comfortably inside it. */
const STRIP_CANVAS: [number, number] = [320, 60];

async function launchWithHeldClip(port: number): Promise<{
  game: GameServer;
  clip: EntitySummary;
}> {
  const game = await GameServer.launch({
    mission: "earth.mis",
    port,
    debugFlags: ["--vr"],
  });
  await game.step({ frames: 30 });

  const clip = (await game.entities.list()).entities.find(
    (entity) => entity.template_id === CLIP_OBJ,
  );
  assert.ok(clip, `expected earth mission object ${CLIP_OBJ} (Standard Clip)`);
  assert.ok(
    (await game.physics.bodies({ entityId: clip.id })).bodies.length > 0,
    "the clip must start as a physical world prop",
  );

  // Stand an arm's length off and squeeze-grab it with the production VR hand
  // ray. The offset matters: the hand is placed a stand-off *back* along the
  // eye->item ray, so standing on top of the item would put it behind the eye.
  const [x, y, z] = clip.position;
  await game.player.teleport({ x: x + 1.2, y: y - 0.8, z: z + 1.2 });
  await game.step({ frames: 30 });
  const aim = await game.player.aimAt(clip.id, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
  await aimVrHandAt(game, aim.world_point, 0.35, 1);
  await game.step({ frames: 5 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    clip.id,
    "the world squeeze must put the clip in the hand",
  );

  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const ui = await game.ui.state();
  assert.equal(ui.mode, "use", "the cyber interface must be open");
  assert.ok(ui.panel_pose, "the open interface must report its panel pose");

  return { game, clip };
}

test(
  "releasing a held item over the VR inventory strip deposits it in the backpack",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    const { game, clip } = await launchWithHeldClip(basePort);
    await using _game = game;

    const pose = (await game.ui.state()).panel_pose!;
    // Aim the holding hand at the strip, squeeze still held so the item stays
    // in the hand across the re-aim.
    await aimVrHandAtCanvas(game, pose, STRIP_CANVAS, { squeeze: 1 });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      clip.id,
      "aiming at the panel must not by itself drop the item",
    );
    const pointer = (await game.ui.state()).pointer;
    assert.deepEqual(
      [pointer?.hand ?? null, pointer?.canvas !== undefined],
      ["right", true],
      `the holding hand must own the canvas pointer: ${JSON.stringify(pointer)}`,
    );

    // Open the hand over the strip.
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 10 });

    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      null,
      "the release must empty the hand",
    );
    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.entity_id === clip.id)?.location,
      "inventory",
      `the clip must be in the backpack, got ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: clip.id })).bodies.length,
      0,
      "a deposited item must not also be a loose world prop",
    );
    const strip = (await game.ui.state()).strip;
    assert.ok(
      strip?.elements.some((element) => element.entity_id === clip.id),
      `the strip must show the deposited clip: ${JSON.stringify(strip?.elements)}`,
    );
  },
);

test(
  "releasing a held item away from the VR inventory strip still drops it into the world",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    const { game, clip } = await launchWithHeldClip(basePort + 1);
    await using _game = game;

    const pose = (await game.ui.state()).panel_pose!;
    // Same spot, same held squeeze - but the controller is turned off the
    // panel, so this release is the ordinary world drop.
    await aimVrHandAtCanvas(game, pose, STRIP_CANVAS, {
      squeeze: 1,
      facing: "away",
    });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).pointer?.hand ?? null,
      null,
      "the hand must be off the panel for this to be the world-drop case",
    );

    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 10 });

    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.equal(
      (await game.player.inventory()).items.find(
        (item) => item.entity_id === clip.id,
      )?.location,
      undefined,
      "an off-strip release must not deposit the item",
    );
    assert.ok(
      (await game.physics.bodies({ entityId: clip.id })).bodies.length > 0,
      "an off-strip release must leave a loose world prop",
    );
  },
);
