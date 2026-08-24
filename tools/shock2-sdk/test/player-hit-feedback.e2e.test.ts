import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Taking damage used to be silent and invisible: hit points dropped and
// nothing else happened. `shock2vr/src/hit_feedback.rs` adds a hurt grunt and
// a brief red rim tint, and this pins the three things that make it feedback
// rather than a decoration:
//
//   1. the tint appears when the player is hit, in BOTH presentations,
//   2. it goes away on its own,
//   3. the grunt that plays is a real, resolvable sample - not a schema name
//      that resolves to a file no install ships (the `smallouch` trap, which
//      was exactly this bug in the first cut and which no scene-object
//      assertion would have caught).
//
// Negative-first: without the feature, (1) and (3) both fail - `/v1/scene` has
// no `hit_feedback` object at all and the damage plays no sound whatsoever.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const HIT_FEEDBACK_SOURCE = "hit_feedback";

/** The tint layer this frame, or undefined when nothing is showing. */
async function tint(game: GameServer) {
  const objects = await game.scene.fromSource(HIT_FEEDBACK_SOURCE);
  assert.ok(objects.length <= 1, `expected at most one tint layer, got ${objects.length}`);
  return objects[0];
}

/**
 * The player's runtime entity id, discovered every run - runtime entity ids are
 * not stable across launches.
 */
async function playerEntityId(game: GameServer): Promise<number> {
  const { player } = await game.info();
  assert.ok(player.entity_id !== null, "the mission has no player entity");
  return player.entity_id;
}

/** Damage the player through the production message path. */
async function hitPlayer(game: GameServer, amount: number): Promise<void> {
  await game.entities.sendMessage(await playerEntityId(game), { type: "Damage", amount });
  // Messages are delivered on the next step; the second frame is the one the
  // tint is measured on, so the decay assertion below has somewhere to go.
  await game.step({ frames: 2 });
}

for (const presentation of ["flat", "vr"] as const) {
  test(
    `a hit tints the view and then clears, in ${presentation}`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        debugFlags: presentation === "vr" ? ["--vr"] : [],
      });
      await game.step({ frames: 60 });

      // Nothing hurts an untouched player.
      assert.equal(await tint(game), undefined, "the view is tinted before any damage");

      await hitPlayer(game, 10);

      const shown = await tint(game);
      assert.ok(shown, "taking 10 damage drew no hit tint");
      // A layer that draws opaque would hide the world; one that draws fully
      // clear is a no-op that still passes "the object exists".
      assert.ok(
        shown.transparency !== null && shown.transparency > 0.05 && shown.transparency < 0.95,
        `the tint must be translucent, got ${shown.transparency}`,
      );
      // The two device traps from PR #1020's dim, which this layer copies: a
      // culled quad covers only part of the view on the Quest, and a
      // depth-tested one only tints what is far away.
      assert.equal(shown.backface_culling, null, "the tint must not opt into culling");
      assert.equal(shown.clear_depth, true, "the tint must begin a depth-cleared layer");
      assert.equal(
        shown.render_layer,
        "scene_overlay",
        "the tint must stay over the world and behind scene UI",
      );
      assert.equal(shown.depth_write, false, "the tint must not occlude what draws after it");

      // A bigger hit reads as bigger.
      await game.step({ frames: 1 });
      const faded = await tint(game);
      assert.ok(faded, "the tint vanished after a single frame");
      assert.ok(
        faded.transparency! > shown.transparency!,
        "the tint must fade, not hold",
      );

      // ...and it clears on its own, with nothing asked to remove it. The
      // longest a full-strength hit lasts is well under two seconds.
      await game.step({ duration: "2s" });
      assert.equal(await tint(game), undefined, "the tint outlasted its decay");
    },
  );
}

test(
  "a hit plays a player hurt grunt that actually resolves to a shipped sample",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 60 });

    const before = await game.audio.recent();
    const lastSequence = before.sounds.at(-1)?.sequence ?? 0;

    await hitPlayer(game, 10);
    await game.step({ frames: 5 });

    const after = await game.audio.recent();
    const played = after.sounds.filter((sound) => sound.sequence > lastSequence);
    assert.ok(
      played.length > 0,
      "damaging the player played nothing - the hurt schema resolved to no shipped sample",
    );
    // `dam_gen_med` -> one of dmgenme1..3. Asserting the *sample* rather than
    // the schema is the whole point: the dead `medouch` schema resolved fine
    // and then loaded no clip.
    assert.ok(
      played.some((sound) => /^dmgen/.test(sound.sample)),
      `expected a dmgen* player-damage grunt, got ${played.map((s) => s.sample).join(", ")}`,
    );
    // The player's own voice is at the ears, not at a point in the world.
    const grunt = played.find((sound) => /^dmgen/.test(sound.sample))!;
    assert.deepEqual(grunt.position, [0, 0, 0], "the hurt grunt must not be spatialized");
  },
);

test(
  "healing the player is not treated as a hit",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 60 });

    await hitPlayer(game, 10);
    await game.step({ duration: "2s" });
    assert.equal(await tint(game), undefined);

    const before = await game.audio.recent();
    const lastSequence = before.sounds.at(-1)?.sequence ?? 0;

    // Negative damage is how the healing path reaches the same applier.
    await game.entities.sendMessage(await playerEntityId(game), {
      type: "Damage",
      amount: -5,
    });
    await game.step({ frames: 5 });

    assert.equal(await tint(game), undefined, "being healed tinted the view");
    const played = (await game.audio.recent()).sounds.filter(
      (sound) => sound.sequence > lastSequence && /^dmgen/.test(sound.sample),
    );
    assert.equal(played.length, 0, "being healed played a hurt grunt");
  },
);
