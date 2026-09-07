import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";
import { dot, sub } from "./helpers/vr-hand.js";
import type { Vec3 } from "../src/types.js";

// Where a model sits in a VR hand is authored in `assets/vr_grips.json`, with
// anything it leaves out measured off the model's own box. `/v1/vr/grip` reads
// the resolved answer and writes into the live registry, which is the loop an
// agent (or the owner) tunes a grip in: nudge, screenshot, save.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/**
 * The palm frame, copied from `shock2vr::hand_seat`: the palm's centre, the way
 * it faces, and how far its skin is from the joints the frame runs through
 * (`PALM_DEPTH`, 12 mm in world units).
 *
 * The palm plane is oblique to every hand axis - the palm faces roughly -X, not
 * -Y - so "the seat put it against the palm" is a projection onto the normal,
 * not a sign test on one coordinate. Transcribed rather than served over HTTP,
 * so `the_palm_frame_matches_the_glove_rig` is what actually guards the numbers;
 * this checks the *seat* built on them.
 */
const PALM_CENTRE: Vec3 = [0.005287, -0.000243, -0.058655];
const PALM_NORMAL: Vec3 = [-0.97836, 0.15474, -0.13728];
const PALM_DEPTH = 0.012 / 0.762;

/** Offsets round-trip through `f32`, so compare them with a tolerance. */
function assertVecClose(actual: number[] | undefined, expected: number[], what: string) {
  assert.ok(actual, `${what} should be present`);
  for (let i = 0; i < expected.length; i += 1) {
    assert.ok(
      Math.abs(actual[i] - expected[i]) < 1e-5,
      `${what}[${i}] is ${actual[i]}, expected ${expected[i]}`,
    );
  }
}

test(
  "VR: a grip comes from the profile file, and the tuner moves it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_grips" });

    // `debug_grips` holds one test item at a time, starting with the mug.
    // Stepping is what lets the scene measure its geometry.
    await game.step({ frames: 10 });

    // The pistol's whole seat is authored, so it comes straight out of the file.
    const pistol = await game.vrGrips.get("atek_h");
    assert.equal(pistol.source, "profile", "a gun's seat is authored");
    assertVecClose(pistol.profile?.offset, [0.0, 0.073, 0.02], "the pistol's grip");

    // The mug's seat is not: the file only says how big to hold it, and where
    // it sits is measured off its own box.
    const mug = await game.vrGrips.get("mug");
    assert.equal(mug.source, "heuristic", "an unauthored seat is measured");
    assert.ok(Math.abs(mug.scale - 0.65) < 1e-5, `held at ${mug.scale}, expected 0.65`);
    assert.equal(mug.family, "cylindrical", "a mug-sized box is gripped");
    // A measured seat puts the item's near face on the palm skin, so its origin
    // clears the palm plane by at least that skin depth - and by rather more,
    // since half the mug's own depth is added on top. The seat this replaced
    // dropped it along hand -Y, which is *across* the knuckles: that lands the
    // mug 0.014 on the wrong side of the palm and fails here.
    const outOfPalm = dot(sub(mug.offset as Vec3, PALM_CENTRE), PALM_NORMAL);
    assert.ok(
      outOfPalm > PALM_DEPTH,
      `a measured seat should clear the palm skin (${PALM_DEPTH}), got ${outOfPalm}`,
    );

    // Nudge it: the readout moves, and the change is merged rather than
    // replacing the scale that was already authored.
    await game.vrGrips.set("mug", { offset: [0.01, -0.05, -0.12] });
    const nudged = await game.vrGrips.get("mug");
    assert.equal(nudged.source, "profile", "an authored offset outranks the box");
    assertVecClose(nudged.offset, [0.01, -0.05, -0.12], "the nudged mug");
    assert.ok(Math.abs(nudged.scale - 0.65) < 1e-5, "the nudge kept the authored scale");

    // Saving goes to a scratch path, so the test never rewrites the asset.
    const dir = mkdtempSync(join(tmpdir(), "vr-grips-"));
    try {
      const path = join(dir, "vr_grips.json");
      const saved = await game.vrGrips.save(path);
      assert.equal(saved.path, path);
      const written = JSON.parse(readFileSync(path, "utf8"));
      assertVecClose(written.mug.offset, [0.01, -0.05, -0.12], "the saved mug");
      // Every migrated gun grip is still in the file the tuner writes back.
      assertVecClose(written.atek_h.offset, [0.0, 0.073, 0.02], "the saved pistol");
      assert.deepEqual(Object.keys(written), [...Object.keys(written)].sort());
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }

    // Clearing takes the mug back to a measured seat at its authored size.
    await game.vrGrips.clear("mug");
    await game.step({ frames: 10 });
    const cleared = await game.vrGrips.get("mug");
    assert.equal(cleared.source, "heuristic");
    assert.equal(cleared.scale, 1.0, "clearing drops the authored scale too");
  },
);

test(
  "VR: a hand reports the model it is holding, so a grip can be tuned by hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    assert.equal(await game.vrGrips.heldModel("right"), null, "an empty hand holds nothing");

    const pistol = await cycleToWeapon(game, (e) => e.name === "Pistol", {
      settleFrames: 90,
    });
    const pawnY = (await game.info()).player.position[1];
    const [px, py, pz] = pistol.position;
    await game.input.set("right_hand.position", [px + 0.4, py - pawnY, pz]);
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });

    assert.equal(await game.vrGrips.heldModel("right"), "atek_h");
    assert.equal((await game.vrGrips.get("atek_h")).source, "profile");
  },
);
