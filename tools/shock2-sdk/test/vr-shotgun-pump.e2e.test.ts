import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, setHandWorldPose, attachSupportHand } from "../src/index.js";
import type { Vec3, ResolvedGrip } from "../src/types.js";
import { add, aimVrHandAt, type Quat } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

const v = (p: ResolvedGrip["offset"]): Vec3 => [p.x, p.y, p.z];
const q = (r: ResolvedGrip["rotation"]): Quat => [...v(r.v), r.s];

for (const [primary, mission, triple, physical] of [
  ["left", "debug_weapons", false, false],
  ["right", "debug_weapons", false, true],
  ["left", "debug_weapons", true, false],
  ["left", "medsci1.mis", false, false],
] as const) {
  test(`shotgun pump ${primary} ${mission}${triple ? " triple" : ""}${physical ? " physical" : ""}: hand-driven cycle, delayed casing, re-grip and ownership`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission, debugFlags: physical ? ["--vr", "--experimental", "physical_held_items"] : ["--vr"] });
    await game.input.set("head.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    await game.player.setStats({ skills: { standard_weapons: 6 } });
    let gun = await cycleToWeapon(game, e => e.template_id === -19);
    await aimVrHandAt(game, gun.position, .2, 1, 0, { hand: primary, lookAtTarget: false });
    const other = primary === "left" ? "right" : "left";
    await game.input.set(`${primary}_hand.position`, [0, 1, -.5]);
    await game.input.set(`${primary}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 20 });
    const support = async () => (await game.info()).player.hand_grips.find(g => g.entity_id === gun.id)!.support!;
    const state = async (): Promise<{ fraction: number; phase: string }> => {
      const prop = (await game.entities.detail(gun.id)).properties.find(p => p.name === "ShotgunPump");
      assert.ok(prop, "the fitted shotgun must enable its manual action");
      return JSON.parse(prop.value);
    };
    const before = await support();
    assert.ok(before.pump, "support profile must expose the authored slider rail");
    const origin = v(before.controller_position);
    const travel = v(before.pump.world_travel);
    assert.ok(Math.hypot(...travel) > .07 && Math.hypot(...travel) < .2, "stroke follows the scaled asset");
    const place = async (fraction: number) => {
      const player = (await game.info()).player;
      await setHandWorldPose(game, player, other, add(origin, travel.map(x => x * fraction) as Vec3), q(before.controller_rotation));
    };
    const fire = async () => {
      await game.input.set(`${primary}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${primary}_hand.trigger`, 0);
      await game.step({ frames: 65 });
    };
    await attachSupportHand(game, primary);
    assert.equal((await support()).attached, true);
    if (triple) {
      await game.input.trigger("CycleGunSetting");
      await game.step({ frames: 2 });
    }
    const roundsPerShot = triple ? 3 : 1;
    const initial = ammoOf(await game.entities.detail(gun.id));
    await fire();
    assert.equal(ammoOf(await game.entities.detail(gun.id)), initial - roundsPerShot);
    assert.equal((await state()).phase, "Spent");
    assert.equal((await game.entities.byTemplate(-2658)).length, 0, "firing retains the shell until the rear stroke");
    await fire();
    assert.equal(ammoOf(await game.entities.detail(gun.id)), initial - roundsPerShot, "cooldown alone cannot chamber the next shot");
    await place(.5);
    await game.step({ frames: 10 });
    assert.ok(Math.abs((await state()).fraction - .5) < .015);
    assert.equal((await support()).attached, true);
    await game.input.set(`${other}_hand.squeeze`, 0);
    await game.step({ frames: 10 });
    await place(0);
    await game.step({ frames: 10 });
    assert.ok(Math.abs((await state()).fraction - .5) < .015, "release freezes the moving part");
    if (mission === "medsci1.mis") {
      const save = `shotgun-pump-${Date.now()}`;
      await game.save(save);
      await game.load(save);
      await game.step({ frames: 5 });
      const restoredPlayer = (await game.info()).player;
      const restored = primary === "left" ? restoredPlayer.wielded_entity_id : restoredPlayer.right_hand_entity_id;
      assert.ok(restored != null);
      gun = { ...gun, id: restored };
      assert.equal((await state()).phase, "Spent", "the chamber state survives a save");
      assert.ok(Math.abs((await state()).fraction - .5) < .015, "the manual part pose survives a save");
    }
    await place(.5);
    await game.input.set(`${other}_hand.squeeze`, 1);
    await game.step({ frames: 10 });
    assert.equal((await support()).attached, true, "grip is acquired at the current pump position");
    await place(1);
    await game.step({ frames: 8 });
    assert.ok((await state()).fraction > .98);
    assert.equal((await state()).phase, "Ejected");
    assert.equal((await game.entities.byTemplate(-2658)).length, 1);
    await game.step({ frames: 8 });
    assert.equal((await game.entities.byTemplate(-2658)).length, 1, "rear dwell does not duplicate the shell");
    await fire();
    assert.equal(ammoOf(await game.entities.detail(gun.id)), initial - roundsPerShot, "open action blocks firing");
    await place(0);
    await game.step({ frames: 10 });
    assert.equal((await state()).phase, "Ready", JSON.stringify({state: await state(), support: await support()}));
    await fire();
    assert.equal(ammoOf(await game.entities.detail(gun.id)), initial - 2 * roundsPerShot);
    const player = (await game.info()).player;
    assert.equal(primary === "left" ? player.wielded_entity_id : player.right_hand_entity_id, gun.id);
    assert.equal(other === "left" ? player.wielded_entity_id : player.right_hand_entity_id, null);
    if (mission === "medsci1.mis") {
      const save = `pump-cross-mode-${Date.now()}`;
      await game.save(save);
      await using flat = await GameServer.launch({ mission });
      await flat.load(save);
      await flat.input.trigger("EquipShotgun");
      await flat.step({ frames: 120 });
      const flatGun = (await flat.info()).player.wielded_entity_id;
      assert.ok(flatGun != null);
      const flatAmmo = ammoOf(await flat.entities.detail(flatGun));
      await flat.input.set("right_hand.trigger", 1);
      await flat.step({ frames: 1 });
      await flat.input.set("right_hand.trigger", 0);
      await flat.step({ frames: 120 });
      assert.equal(ammoOf(await flat.entities.detail(flatGun)), flatAmmo - 1);
      await flat.save(`${save}-flat`);
      await game.load(`${save}-flat`);
      await game.step({ frames: 10 });
      const returned = (await game.info()).player.wielded_entity_id;
      assert.ok(returned != null);
      gun = { ...gun, id: returned };
      assert.equal((await state()).phase, "Ready", "flat's completed action clears the old VR chamber latch");
      assert.ok((await state()).fraction < .01);
    }
  });
}
