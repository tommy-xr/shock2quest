import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end coverage for the Recreation deck's authored PictureSwap flow
// (#587). Code Pic 1's model tweq is:
//
//   pic05 -> static -> pic03 -> static -> code10 -> static -> ...
//
// The original PictureSwap script advances two frames per activation: it shows
// the intervening static model for one second, then advances to the next
// picture. This test uses the production flat interaction ray and squeeze
// input, not the debug message-injection endpoint.
//
// Negative-first: with `pictureswap` mapped to NoopScript, the first model
// assertion after the normal frob reads `pic05` instead of `static`. Before
// versioned script state, loading the mid-static save below reran initialize
// and reset the remaining delay to a full second, so the 31 restored frames
// still read `static` instead of `pic03`.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const CODE_PIC_1 = "Code Pic 1";

function modelOf(detail: {
  properties: { name: string; value: string }[];
}): string {
  const model = detail.properties.find((property) => property.name === "Model");
  assert.ok(model, "Code Pic should expose its live Model property");
  return model.value;
}

async function frobThroughReticle(
  game: GameServer,
  entity: { id: number; position: [number, number, number] },
): Promise<void> {
  // This side of rec1's picture has an unobstructed selectable surface. The
  // teleport only stages the player inside retail frob reach; aim and frob
  // use production paths.
  await game.player.teleport({
    x: entity.position[0] + 2.5,
    y: entity.position[1] - 1,
    z: entity.position[2],
  });
  await game.step({ frames: 3 });
  const aim = await game.player.aimAt(entity, { visibility: "required" });
  assert.equal(aim.classification, "surface", JSON.stringify(aim));
  assert.equal(aim.interaction_target_id, entity.id, JSON.stringify(aim));
  assert.equal(aim.target_confirmed, true, JSON.stringify(aim));

  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 2 });
}

test(
  "rec1.mis: a normal frob reveals Code Pic 1's code10 model through static",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
    });
    await game.step({ frames: 5 });
    const supportedSavePosition = await game.player.position();

    const pictures = (
      await game.entities.list({ filter: CODE_PIC_1, limit: 20 })
    ).entities;
    const picture = pictures.find((entity) => entity.name === CODE_PIC_1);
    assert.ok(picture, `expected rec1's authored ${CODE_PIC_1}`);
    assert.equal(modelOf(await game.entities.detail(picture.id)), "pic05");

    await frobThroughReticle(game, picture);
    assert.equal(
      modelOf(await game.entities.detail(picture.id)),
      "static",
      "the accepted frob should immediately show the transition model",
    );
    await game.step({ frames: 30 });
    assert.equal(
      modelOf(await game.entities.detail(picture.id)),
      "static",
      "PictureSwap should retain static for the authored one-second delay",
    );
    const saveName = `picture_swap_mid_static_${Date.now()}`;
    // The reticle helper stages beside the authored picture in non-playable
    // geometry. Restore the supported spawn without stepping so the exact
    // one-second PictureSwap timer remains the behavior under test.
    await game.player.teleport(supportedSavePosition);
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    const restoredPictures = (
      await game.entities.list({ filter: CODE_PIC_1, limit: 20 })
    ).entities;
    const restoredPicture = restoredPictures.find(
      (entity) => entity.name === CODE_PIC_1,
    );
    assert.ok(restoredPicture, `expected restored ${CODE_PIC_1}`);
    assert.equal(
      modelOf(await game.entities.detail(restoredPicture.id)),
      "static",
    );
    await game.step({ frames: 20 });
    assert.equal(
      modelOf(await game.entities.detail(restoredPicture.id)),
      "static",
      "the restored exact timer should not finish too early",
    );
    await game.step({ frames: 11 });
    assert.equal(
      modelOf(await game.entities.detail(restoredPicture.id)),
      "pic03",
      "the restored timer must not restart from a full second",
    );

    await frobThroughReticle(game, restoredPicture);
    assert.equal(
      modelOf(await game.entities.detail(restoredPicture.id)),
      "static",
    );
    await game.step({ frames: 61 });
    assert.equal(
      modelOf(await game.entities.detail(restoredPicture.id)),
      "code10",
      "the second normal frob should visibly reveal this frame's code segment",
    );
  },
);
