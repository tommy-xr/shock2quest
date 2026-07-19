import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Regression test for #502: crouch must shrink the player collider, not just
// the camera. The MedSci critical path proves it end-to-end - behind the
// keypad-45100 door (template 1739) the corridor is collapsed and the
// authored route is a low air duct whose entrance grate leaves ~3.1 ft of
// clearance: the standing 4.8 ft capsule cannot enter, a crouched one can,
// and standing up under the grate must be refused for lack of headroom.
//
// Waypoints were mapped against the live level (raycast probes): the duct
// runs west along z~=-16.7..-17.1 at floor y=-1.6; a "Broken Railing" prop
// intrudes from the south, so the crawl line hugs the north side.
test(
  "crouch shrinks the collider: MedSci air shaft is crouch-only",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8116),
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
    // threshold in front of the duct.
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

    // STANDING: the duct entrance grate must reject the 4.8 ft capsule. The
    // player can drop into the duct mouth but not pass the grate at x=-24.0.
    await walk({ x: -27.0, y: -0.9, z: -16.9 });
    const standing = await game.player.position();
    assert.ok(
      standing.x > -24.0,
      `standing player must not pass the duct grate (reached x=${standing.x.toFixed(2)})`,
    );

    // CROUCH: the capsule shrinks feet-planted (center y drops), and the
    // duct becomes traversable - crawl west past the grate and the duct's
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
      `crouched player must crawl through the duct (reached x=${deep.x.toFixed(2)})`,
    );

    // SAVE/LOAD while crouched: the save must normalize the lowered center
    // and restore the crouched capsule, not reload a standing capsule
    // embedded in the duct (regression guard for the crouch-save edge).
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

    // STAND-UP REFUSAL: crawl back under the entrance grate (~3.1 ft of
    // headroom) and release crouch - the collider must stay crouched.
    for (const wp of [
      { x: -28.0, y: -0.9, z: -16.7 },
      { x: -25.5, y: -0.9, z: -16.7 },
      { x: -24.05, y: -0.9, z: -16.9 },
    ]) {
      await walk(wp);
    }
    const underGrate = await game.player.position();
    await game.input.set("crouch", 0);
    await game.step({ frames: 20 });
    const afterRelease = await game.player.position();
    assert.ok(
      Math.abs(afterRelease.y - underGrate.y) < 0.2,
      `stand-up under the grate must be refused (y ${underGrate.y.toFixed(2)} -> ${afterRelease.y.toFixed(2)}; standing would be +0.40)`,
    );

    // Crawl back out of the duct mouth. Sample the crouched height AFTER
    // exiting (the climb out of the duct raises y on its own), then release
    // crouch - only the actual stand should produce the remaining rise.
    await game.input.set("crouch", 1);
    await game.step({ frames: 5 });
    await walk({ x: -22.4, y: -0.9, z: -17.3 });
    await walk({ x: -22.4, y: 0.5, z: -18.5 });
    const preStand = await game.player.position();
    await game.input.set("crouch", 0);
    await game.step({ frames: 20 });
    const stood = await game.player.position();
    assert.ok(
      stood.y - preStand.y > 0.25,
      `player must stand once clear of the duct (y ${preStand.y.toFixed(2)} -> ${stood.y.toFixed(2)}; standing center is +0.40)`,
    );
  },
);
