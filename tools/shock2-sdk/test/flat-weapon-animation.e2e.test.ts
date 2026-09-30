import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

function animation(detail: { properties: { name: string; value: string }[] }) {
  const property = detail.properties.find(p => p.name === "FlatWeaponAnimation");
  assert.ok(property, "the first-person mesh has an animation driver");
  return JSON.parse(property.value) as { clip: string | null; frame: number; translation: { x: number; y: number; z: number }; parameters: [number, number][] };
}

test("flat pistol cycles its slide and ejects immediately", {
  skip: process.env.SHOCK2_E2E !== "1",
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 10 });
  const gun = await cycleToWeapon(game, e => e.name === "Pistol", { settleFrames: 120 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  assert.equal((await game.entities.byTemplate(-2657)).length, 1);
  await game.step({ frames: 2 });
  const pose = animation(await game.entities.detail(gun.id));
  assert.equal(pose.clip, "shoot");
  assert.ok(pose.parameters.some(([, value]) => value < -0.1), "slide recoils behind the barrel");
  await game.step({ frames: 16 });
  assert.ok(animation(await game.entities.detail(gun.id)).parameters.every(([, value]) => Math.abs(value) < 0.001),
    "slide returns to battery");
  assert.equal((await game.entities.byTemplate(-2657)).length, 1, "slide motion adds no second casing");
});

for (const [name, reserve] of [["Pistol", -31], ["Shotgun", -43]] as const) {
  test(`flat ${name} can fire during equip without snapping to rest`, {
    skip: process.env.SHOCK2_E2E !== "1",
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 10 });
    const gun = await cycleToWeapon(game, e => e.name === name, { settleFrames: 2 });
    const before = animation(await game.entities.detail(gun.id));
    assert.equal(before.clip, "raise");
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 1 });
    const after = animation(await game.entities.detail(gun.id));
    assert.equal(after.clip, "shoot", "firing begins immediately");
    assert.ok(after.translation.y < -0.15, "draw displacement blends out instead of snapping to zero");
    await game.step({ frames: 8 });
    assert.ok(Math.abs(animation(await game.entities.detail(gun.id)).translation.y) < 0.05,
      "the short blend reaches the shoot pose");
  });

  test(`flat ${name} equip and reload animate parts without changing the fire gate`, {
    skip: process.env.SHOCK2_E2E !== "1",
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 10 });
    const gun = await cycleToWeapon(game, e => e.name === name, { settleFrames: 2 });
    assert.equal(animation(await game.entities.detail(gun.id)).clip, "raise");
    await game.step({ frames: 35 });
    assert.ok(animation(await game.entities.detail(gun.id)).parameters.some(([, value]) => Math.abs(value) > 0.1),
      "equipping moves the slide or pump");
    await game.step({ frames: 120 });
    assert.equal(animation(await game.entities.detail(gun.id)).clip, null);
    await game.player.spawnItem(reserve);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    // Shotgun reload interrupts its shoot clip before the delayed ejection.
    await game.step({ frames: name === "Shotgun" ? 2 : 120 });
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.reloading, true);
    assert.equal(animation(await game.entities.detail(gun.id)).clip, "reload");
    const ammo = (await game.entities.detail(gun.id)).properties.find(p => p.name === "Ammo")!.value;
    let movedPart = false;
    let previousFrame = -1;
    const seenCasings = new Set<number>();
    while ((await game.info()).player.reloading) {
      if (name === "Shotgun") {
        for (const casing of await game.entities.byTemplate(-2658)) seenCasings.add(casing.id);
      }
      const pose = animation(await game.entities.detail(gun.id));
      assert.equal(pose.clip, "reload");
      assert.ok(pose.frame >= previousFrame, "triggering during reload must not restart the clip");
      previousFrame = pose.frame;
      movedPart ||= pose.parameters.some(([, value]) => Math.abs(value) > 0.1);
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0);
      assert.equal((await game.entities.detail(gun.id)).properties.find(p => p.name === "Ammo")!.value, ammo,
        "reload blocks firing and transfers ammo only once");
      await game.step({ frames: 1 });
    }
    assert.ok(movedPart, "reload moves the magazine or pump");
    if (name === "Shotgun") {
      assert.equal(seenCasings.size, 1,
        "interrupting the pump with reload retains the accepted shot's casing");
    }
    await game.step({ frames: 2 });
    const rest = animation(await game.entities.detail(gun.id));
    assert.equal(rest.clip, null);
    assert.ok(rest.parameters.every(([, value]) => Math.abs(value) < 0.001));
  });
}

test("flat shotgun ejects during its pump stroke, once per successful shot", {
  skip: process.env.SHOCK2_E2E !== "1",
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 10 });
  const gun = await cycleToWeapon(game, e => e.name === "Shotgun", { settleFrames: 120 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  assert.equal((await game.entities.byTemplate(-2658)).length, 0,
    "the casing stays in the chamber until the pump moves back");
  assert.equal((await game.entities.byTemplate(-2653)).length, 1,
    "the muzzle flash still accompanies the shot immediately");
  await game.step({ frames: 43 });
  assert.equal((await game.entities.byTemplate(-2658)).length, 0);
  await game.step({ frames: 4 });
  assert.equal((await game.entities.byTemplate(-2658)).length, 1,
    "the pump ejects one shell at its authored event");
  assert.ok(animation(await game.entities.detail(gun.id)).parameters.some(([, value]) => Math.abs(value) > 0.1),
    "the first-person renderer receives a moving-part pose at ejection");
  await game.step({ frames: 10 });
  assert.equal((await game.entities.byTemplate(-2658)).length, 1,
    "crossing the event must not emit it again");
  await game.step({ frames: 150 });
  assert.ok(animation(await game.entities.detail(gun.id)).parameters.every(([, value]) => Math.abs(value) < 0.001),
    "the pump returns to rest");
});
