import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Regression test for #502: crouch must shrink the player collider, not just
// the camera. The MedSci critical path proves it end-to-end - behind the
// keypad-45100 door (template 1739) the corridor is collapsed into a five-foot
// passage (floor y=-1.6, ceiling y=0.4): the standing 6.0 ft capsule cannot
// enter, a crouched one can, and standing up under the ceiling is refused.
//
// Waypoints were mapped against the live level (raycast probes): the passage
// runs west along z~=-16.7..-17.1 at floor y=-1.6; a "Broken Railing" prop
// intrudes from the south, so the crawl line hugs the north side.
test(
  "crouch shrinks the collider: MedSci five-foot passage is crouch-only",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    // This test has to QuickSave (it exercises the crouch save/load edge),
    // which writes save1.sav into the repo root. Put that file back exactly as
    // it was found, so the suite stays order-independent - quickload-missing
    // requires a worktree with no session quicksave and glob order runs it
    // after this file - and so a developer's own quicksave survives a test run
    // rather than being silently replaced by a MedSci passage run.
    const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
    const quicksave = join(repoRoot, "save1.sav");
    const saved = existsSync(quicksave) ? readFileSync(quicksave) : null;
    t.after(() => {
      if (saved === null) rmSync(quicksave, { force: true });
      else writeFileSync(quicksave, saved);
    });

    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    await game.step({ frames: 10 });

    // Bounded, collision-validated walk toward a target: repeat single moves
    // until blocked or arrived (each move is clamped, so several are needed).
    const walk = async (target: { x: number; y: number; z: number }) => {
      let last;
      for (let i = 0; i < 25; i++) {
        last = await game.player.moveTo(target);
        await game.step({ frames: 3 });
        if (last.blocked || last.distance_moved < 0.01) break;
      }
      return last!;
    };

    // Open the cryo-exit door (stable template id; runtime ids change per
    // launch). TurnOn is what the keypad sends over its SwitchLink - the
    // keypad MFD flow itself is covered by keypad.e2e.test.ts.
    const [door] = await game.entities.byTemplate(1739);
    assert.ok(door, "medsci1 should contain the keypad door (template 1739)");
    await game.entities.sendMessage(door.id, { type: "TurnOn" });
    await game.step({ frames: 120 });

    // Walk the authored route: through the doorway to the collapsed-corridor
    // threshold in front of the low passage.
    await teleportVerified(game, { x: -25.5, y: 0.7, z: -11.7 });
    await game.step({ frames: 10 });
    for (const wp of [
      { x: -24.8, y: 0.7, z: -13.1 },
      { x: -24.0, y: 0.7, z: -14.6 },
      { x: -23.2, y: 0.7, z: -16.1 },
      { x: -22.4, y: 0.7, z: -17.6 },
    ]) {
      await walk(wp);
    }

    // STANDING: the world ceiling begins at x=-26.4 and must reject the
    // six-foot capsule. (Before the footprint fix, the 4.8-foot body crossed.)
    await walk({ x: -27.0, y: -0.9, z: -16.9 });
    const standing = await game.player.position();
    assert.ok(
      standing.x > -26.0,
      `standing player must not pass the five-foot ceiling face (reached x=${standing.x.toFixed(2)})`,
    );

    // CROUCH: the capsule shrinks feet-planted (center y drops), and the
    // passage becomes traversable - crawl west past the ceiling face and the
    // authored tripwire (x=-29.6) to its far half.
    const beforeCrouch = await game.player.position();
    await game.input.set("crouch", 1);
    await game.step({ frames: 10 });
    const crouched = await game.player.position();
    assert.ok(
      beforeCrouch.y - crouched.y > 0.2,
      `crouching must lower the collider center (y ${beforeCrouch.y.toFixed(2)} -> ${crouched.y.toFixed(2)})`,
    );

    for (const wp of [
      { x: -24.05, y: -0.9, z: -16.9 },
      { x: -25.5, y: -0.9, z: -16.7 },
      { x: -28.0, y: -0.9, z: -16.7 },
      { x: -31.0, y: -0.9, z: -16.9 },
    ]) {
      await walk(wp);
    }
    const deep = await game.player.position();
    assert.ok(
      deep.x < -30.0,
      `crouched player must crawl through the passage (reached x=${deep.x.toFixed(2)})`,
    );

    // SAVE/LOAD while crouched: the save must normalize the lowered center
    // and restore the crouched capsule, not reload a standing capsule
    // embedded in the passage (regression guard for the crouch-save edge).
    const preSave = await game.player.position();
    await game.input.trigger("QuickSave");
    await game.step({ frames: 30 });
    await game.input.trigger("QuickLoad");
    await game.step({ frames: 30 });
    const loaded = await game.player.position();
    assert.ok(
      Math.abs(loaded.y - preSave.y) < 0.2,
      `crouched save must reload in the crouched pose (y ${preSave.y.toFixed(2)} -> ${loaded.y.toFixed(2)})`,
    );

    // STAND-UP REFUSAL: crawl back to x=-27, still below the five-foot
    // ceiling, and release crouch - the collider must stay crouched. The
    // upward world ray makes the fixture self-checking instead of relying on
    // a prop collider or an assumed waypoint.
    for (const wp of [
      { x: -28.0, y: -0.9, z: -16.7 },
      { x: -27.0, y: -0.9, z: -16.7 },
    ]) {
      await walk(wp);
    }
    const underCeiling = await game.player.position();
    const lowCeiling = await game.raycast({
      start: [underCeiling.x, -1.55, underCeiling.z],
      end: [underCeiling.x, 4.0, underCeiling.z],
      collision_groups: ["world"],
    });
    assert.ok(
      lowCeiling.hit_point && lowCeiling.hit_point[1] < 0.5,
      `setup must remain under the authored low ceiling: ${JSON.stringify(lowCeiling)}`,
    );
    await game.input.set("crouch", 0);
    await game.step({ frames: 20 });
    const afterRelease = await game.player.position();
    assert.ok(
      Math.abs(afterRelease.y - underCeiling.y) < 0.2,
      `stand-up under the five-foot ceiling must be refused (y ${underCeiling.y.toFixed(2)} -> ${afterRelease.y.toFixed(2)}; standing would be +0.64)`,
    );

    // Continue east while crouched. The low ceiling ends at x=-26.4; the
    // reachable point at (-23.2, -16.1) has twelve feet of world headroom
    // (floor y=-1.6, ceiling y=3.2). Prove that clearance with the same upward
    // ray before releasing crouch, then require the actual stand.
    await game.input.set("crouch", 1);
    await game.step({ frames: 5 });
    await walk({ x: -25.5, y: -0.9, z: -16.7 });
    await walk({ x: -24.05, y: -0.9, z: -16.9 });
    await walk({ x: -22.4, y: -0.9, z: -17.3 });
    await walk({ x: -22.4, y: 0.7, z: -17.6 });
    await walk({ x: -23.2, y: 0.7, z: -16.1 });
    const preStand = await game.player.position();
    const highCeiling = await game.raycast({
      start: [preStand.x, -1.55, preStand.z],
      end: [preStand.x, 4.0, preStand.z],
      collision_groups: ["world"],
    });
    assert.ok(
      highCeiling.hit_point && highCeiling.hit_point[1] > 0.9,
      `exit must have more than six feet of headroom: ${JSON.stringify(highCeiling)}`,
    );
    await game.input.set("crouch", 0);
    await game.step({ frames: 20 });
    const stood = await game.player.position();
    assert.ok(
      stood.y - preStand.y > 0.25,
      `player must stand once clear of the passage ` +
        `(${JSON.stringify(preStand)} -> ${JSON.stringify(stood)}; standing center is +0.64)`,
    );
  },
);
