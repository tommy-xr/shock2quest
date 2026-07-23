import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { pickupEarthWeapons } from "./helpers/earth-weapons.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end regression test for the reload ANIMATION + fire gate. The wielded
// weapon tilts during reload (a three-phase pitch: down -> hold -> up) and
// firing is blocked while it reloads. Both are observable headlessly via
// /v1/info (reloading / reload_pitch_deg / reload_progress) - no screenshots.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

test(
  "reload tilts the viewmodel and blocks firing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8098),
    });

    await game.step({ frames: 30 });
    const { pistol, clips } = await pickupEarthWeapons(game, 3);
    const pistolId = pistol.id;
    assert.equal(ammoOf(await game.entities.detail(pistolId)), 0, "Earth pistol starts empty");

    // Seed a full magazine from two authored reserve clips, then wait for that
    // reload to finish before testing the next reload's animation.
    await game.input.trigger("Reload");
    await game.step({ frames: 140 });
    const clip = ammoOf(await game.entities.detail(pistolId));
    assert.equal(clip, 12, "two small clips filled the pistol");
    assert.ok(
      (await game.player.inventory()).items.some((item) => item.entity_id === clips[2].id),
      "third reserve clip remains for the animated partial reload",
    );

    // Not reloading at rest.
    let snap = (await game.info()).player;
    assert.equal(snap.reloading, false, "not reloading at rest");
    assert.equal(snap.reload_pitch_deg, 0, "no tilt at rest");

    // Drain a few rounds, then start a reload.
    for (let i = 0; i < 4; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistolId)), clip - 4, "fired 4 rounds");

    await game.input.trigger("Reload");
    await game.step({ frames: 6 });

    // Reload is in progress and the viewmodel has begun to tilt.
    snap = (await game.info()).player;
    assert.equal(snap.reloading, true, "reloading after trigger");
    assert.ok(
      Math.abs(snap.reload_pitch_deg) > 1,
      `viewmodel should be tilting (got ${snap.reload_pitch_deg} deg)`,
    );
    assert.ok(
      snap.reload_progress > 0 && snap.reload_progress < 1,
      `reload should be partway (got ${snap.reload_progress})`,
    );

    // Fire gate: firing mid-reload must NOT consume ammo.
    const ammoMidReload = ammoOf(await game.entities.detail(pistolId));
    await fireOnce(game);
    await fireOnce(game);
    assert.equal((await game.info()).player.reloading, true, "still reloading");
    assert.equal(
      ammoOf(await game.entities.detail(pistolId)),
      ammoMidReload,
      "firing is blocked during reload (no ammo consumed)",
    );

    // The tilt reaches a clear peak partway through (the pistol dips ~67 deg).
    await game.step({ frames: 30 });
    assert.ok(
      Math.abs((await game.info()).player.reload_pitch_deg) > 40,
      "viewmodel reaches a clear peak tilt mid-reload",
    );

    // Step well past the total reload; it completes and the tilt returns to 0.
    await game.step({ frames: 120 });
    snap = (await game.info()).player;
    assert.equal(snap.reloading, false, "reload completes");
    assert.equal(snap.reload_pitch_deg, 0, "viewmodel returns to rest (no tilt)");

    // And the clip is full again (reload refilled it).
    assert.equal(ammoOf(await game.entities.detail(pistolId)), clip, "clip refilled to capacity");
  },
);
