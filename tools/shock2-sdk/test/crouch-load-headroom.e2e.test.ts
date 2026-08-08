import assert from "node:assert/strict";
import { existsSync, rmSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Locate the on-disk .sav for a save name (mirrors shock2vr::save_file_path =
// <data_root>/saves/<name>.sav; data_root is DARK_ASSET_PATH or a ./Data search
// relative to the repo root), so the test can clean up after itself.
function findSavePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((d): d is string => Boolean(d));
  for (const root of roots) {
    const p = join(root, "saves", `${saveName}.sav`);
    if (existsSync(p)) return p;
  }
  return undefined;
}

// Regression test for #773: loading a save taken while crouched under a low
// ceiling must not stand the player up into it.
//
// The load path re-applies the saved crouch, then runs a frame whose crouch
// input is released. Standing up is gated on a headroom shape query, and Rapier
// only rebuilds the broad-phase BVH inside its pipeline step - so on the first
// frame of a freshly built world the query matched nothing, the check read
// "clear", and the six-foot capsule expanded into the ceiling. Once embedded in
// level geometry the character controller resolves no movement at all, in any
// direction, for the rest of the session.
//
// The fixture is hydro2's "Low Head Room" ledge above the Sector C window row
// (floor y=2.8, brush ceiling y~=4.35 - 1.55 wu, less than the 2.4 wu standing
// body), which is where the campaign play-through hit this.
test(
  "a save taken crouched under a low ceiling reloads crouched and mobile",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    const saveName = `crouch_load_headroom_e2e_${Date.now()}`;
    t.after(() => {
      const path = findSavePath(saveName);
      if (path) rmSync(path, { force: true });
    });

    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8237),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 10 });

    // Stand on the open (six-foot-clearance) end of the ledge, then crawl into
    // the low-headroom pocket with ordinary locomotion input.
    await teleportVerified(game, { x: 83.67, y: 4.04, z: 19.7 });
    await game.step({ frames: 10 });
    await game.input.set("crouch", 1);
    await game.step({ frames: 10 });
    await game.input.set("head.look", [90, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 40 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const preSave = await game.player.position();
    const ceiling = await game.raycast({
      start: [preSave.x, preSave.y, preSave.z],
      end: [preSave.x, preSave.y + 6.0, preSave.z],
      collision_groups: ["world"],
    });
    // Feet are half a crouched body (0.56 wu) below the collider center; the
    // standing body is 2.4 wu tall, so anything under 2.4 wu of clearance is
    // crouch-only.
    assert.ok(
      ceiling.hit_point && ceiling.hit_point[1] - (preSave.y - 0.56) < 2.4,
      `setup must be under a crouch-only ceiling: ${JSON.stringify(ceiling)} over feet y=${(preSave.y - 0.56).toFixed(2)}`,
    );

    await game.save(saveName);

    // Release crouch BEFORE loading: this is the cold-load case (nothing is
    // holding the crouch key when the restored world runs its first frame).
    await game.input.set("crouch", 0);
    await game.load(saveName);
    await game.step({ frames: 30 });

    const loaded = await game.player.position();
    assert.ok(
      Math.abs(loaded.y - preSave.y) < 0.2,
      `loading must keep the crouched pose the ceiling requires ` +
        `(y ${preSave.y.toFixed(3)} -> ${loaded.y.toFixed(3)}; standing would be +0.64)`,
    );

    // ...and the player must still be able to crawl back out under production
    // locomotion. An embedded capsule reports exactly zero displacement here.
    await game.input.set("head.look", [270, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const walked = await game.player.position();
    assert.ok(
      loaded.z - walked.z > 2.0,
      `a loaded crouched player must still move ` +
        `(${JSON.stringify(loaded)} -> ${JSON.stringify(walked)})`,
    );
  },
);
