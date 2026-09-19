import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon, pullTrigger, waitForShotReady } from "./helpers/weapon.js";

/** The flat crosshair fire ray the wielded gun actually shoots along - which
 * recoil bends away from the camera forward. */
async function fireRay(
  game: GameServer,
  id: number,
): Promise<[number, number, number]> {
  const detail = await game.entities.detail(id);
  const property = detail.properties.find((p) => p.name === "FlatAim");
  assert.ok(property, "the flat viewmodel should expose its fire ray");
  return (JSON.parse(property.value) as { forward: [number, number, number] })
    .forward;
}

test(
  "flat shots ride the recoil: rapid fire walks the aim up, a settled gun is on the crosshair",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 60 });
    // Agility zeroes the authored angular kick (CalcKickAngle) and the bench
    // character is maxed out, so there would be nothing to ride.
    await game.devParams.set("gun_agility_override", 1);
    const gun = await cycleToWeapon(game, (e) => e.template_id === -17, {
      settleFrames: 60,
    });
    assert.equal((await game.info()).player.wielded_entity_id, gun.id);
    await game.step({ frames: 60 });

    const rest = await fireRay(game, gun.id);
    const look = (await game.info()).player.camera_rotation;

    // Three shots fired faster than the spring recovers: the aim should end up
    // above where it started, and the camera should not have moved at all.
    for (let shot = 0; shot < 3; shot += 1) {
      await waitForShotReady(game);
      await pullTrigger(game);
      await game.step({ frames: 2 });
    }
    const walked = await fireRay(game, gun.id);
    assert.ok(
      walked[1] > rest[1] + 0.01,
      `sustained fire should walk the aim up: ${walked} vs ${rest}`,
    );
    assert.deepEqual(
      (await game.info()).player.camera_rotation,
      look,
      "the shot rides the gun, not the camera - the view must not move",
    );

    // Let it settle: a rested gun fires exactly along the crosshair again.
    await game.step({ frames: 300 });
    const settled = await fireRay(game, gun.id);
    assert.ok(
      settled.every((v, i) => Math.abs(v - rest[i]) < 1e-3),
      `a settled gun should fire on the crosshair: ${settled} vs ${rest}`,
    );

    // The knob turns the whole mechanic off without touching the viewmodel.
    await game.devParams.set("flat_recoil_aim", 0);
    await waitForShotReady(game);
    await pullTrigger(game);
    await game.step({ frames: 2 });
    const cosmetic = await fireRay(game, gun.id);
    assert.ok(
      cosmetic.every((v, i) => Math.abs(v - rest[i]) < 1e-3),
      `flat_recoil_aim=0 should keep the shot on the crosshair: ${cosmetic}`,
    );
  },
);
