import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// Where a model sits in a VR hand is authored in `assets/vr_grips.json`, with
// anything it leaves out measured off the model's own box. `/v1/vr/grip` reads
// the resolved answer and writes into the live registry, which is the loop an
// agent (or the owner) tunes a grip in: nudge, screenshot, save.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

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
    assert.ok(
      mug.offset[1] < 0,
      `a measured seat hangs below the palm, got ${mug.offset[1]}`,
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
