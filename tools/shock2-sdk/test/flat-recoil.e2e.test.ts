import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { ammoOf, cycleToWeapon, fireOnce } from "./helpers/weapon.js";

/** The live muzzle direction of the flat viewmodel, which the recoil spring
 * pitches up and lets fall back. */
async function muzzleForward(
  game: GameServer,
  id: number,
): Promise<[number, number, number]> {
  const detail = await game.entities.detail(id);
  const property = detail.properties.find((p) => p.name === "WeaponMuzzle");
  assert.ok(property, "the wielded gun should expose its muzzle frame");
  return (JSON.parse(property.value) as { forward: [number, number, number] })
    .forward;
}

test(
  "the flat pistol viewmodel kicks on a shot and settles back to its carry pose",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 60 });
    // Agility zeroes the authored angular kick (CalcKickAngle), and the bench
    // character is maxed out - drop it so the muzzle rise is observable.
    await game.devParams.set("gun_agility_override", 1);
    const gun = await cycleToWeapon(game, (e) => e.template_id === -17, {
      settleFrames: 60,
    });
    assert.equal((await game.info()).player.wielded_entity_id, gun.id);
    await game.step({ frames: 60 });

    const restForward = await muzzleForward(game, gun.id);
    const restAim = (await game.info()).player.camera_rotation;
    const ammo = ammoOf(await game.entities.detail(gun.id));

    await fireOnce(game);
    await game.step({ frames: 4 });

    assert.equal(
      ammoOf(await game.entities.detail(gun.id)),
      ammo - 1,
      "the shot must actually fire - a refused shot draws no recoil",
    );
    const kicked = await muzzleForward(game, gun.id);
    assert.ok(
      kicked[1] > restForward[1] + 0.01,
      `the muzzle should rise: ${kicked} vs ${restForward}`,
    );
    assert.deepEqual(
      (await game.info()).player.camera_rotation,
      restAim,
      "flat recoil is viewmodel-only: the camera/crosshair must not move",
    );

    await game.step({ frames: 300 });
    const settled = await muzzleForward(game, gun.id);
    assert.ok(
      settled.every((v, i) => Math.abs(v - restForward[i]) < 1e-3),
      `the gun should settle back to its carry pose: ${settled} vs ${restForward}`,
    );
  },
);
