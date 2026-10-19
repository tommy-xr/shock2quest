import assert from "node:assert/strict";
import { test } from "node:test";

import { launchWithSettings } from "./helpers/settings.js";
import { reachHolster } from "./helpers/vr-hand.js";
import { pullTrigger } from "./helpers/weapon.js";
import { worldSeconds } from "./helpers/world-clock.js";

// The bullet-time showcase: Speed Boost hypo in the left hand, laser-sighted
// pistol in the right, a second in the left holster. The hypo slows the world
// to 0.25 and frees the left hand to draw for dual wielding.
test("debug_bullet_time: use the hypo, then draw the holstered pistol", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async (t) => {
  await using game = await launchWithSettings(t, { vr: { speed_boosts: "SlowWorld" } }, {
    mission: "debug_bullet_time", debugFlags: ["--vr"],
  });
  // A VR hand keeps its item only while its grip is squeezed.
  await game.input.set("left_hand.squeeze", 1);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 10 });
  let { player } = await game.info();
  const [hypo] = await game.entities.byTemplate(-58);
  const pistols = (await game.entities.byTemplate(-17)).map(e => e.id);
  assert.equal(player.wielded_entity_id, hypo.id, "hypo in the left hand");
  assert.ok(pistols.includes(player.right_hand_entity_id!), "pistol in the right hand");
  const holstered = player.hand_feedback?.holsters?.items[1];
  assert.ok(holstered && pistols.includes(holstered) && holstered !== player.right_hand_entity_id,
    "second pistol in the left holster");
  assert.equal((await game.entities.byTemplate(-175)).length, 1, "one shotgun hybrid");
  for (const id of pistols) {
    const upgrades = (await game.entities.detail(id)).properties.find(p => p.name === "WeaponUpgrades");
    assert.equal(JSON.parse(upgrades!.value).laser_enabled, true, `pistol ${id} has a lit laser`);
  }

  await pullTrigger(game, "left");
  assert.equal((await game.info()).player.wielded_entity_id, null, "the hypo is used up");
  assert.ok(Math.abs((await worldSeconds(game, 60)) - 0.25) < 1e-3, "world at 0.25");

  // Draw: release the grip, reach the left holster, squeeze afresh.
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 2 });
  await reachHolster(game, "left", 1);
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 30 });
  player = (await game.info()).player;
  assert.equal(player.wielded_entity_id, holstered, "drawn into the left hand");
  assert.ok(pistols.includes(player.right_hand_entity_id!), "still holding the right pistol");
});
