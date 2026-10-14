import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";
import { standNear } from "./helpers/os-upgrade.js";
import { teleportVerified } from "./helpers/teleport.js";
import { canvasCenter } from "./helpers/ui.js";
import { aimVrHandAt, aimVrHandAtCanvas, aimMfdAt, drawPersonalCard } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
const machines = [
  ["replicator", "earth.mis", 262],
  ["stats trainer", "medsci1.mis", 1352],
  ["tech trainer", "medsci1.mis", 1317],
  ["weapon trainer", "medsci1.mis", 1354],
  ["psi trainer", "medsci1.mis", 1355],
  ["O/S upgrade", "medsci2.mis", 133],
] as const;

for (const [name, mission, template] of machines) {
  test(`VR frob opens ${name} in the cyber interface without a scan or purchase`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission, debugFlags: ["--vr"] });
    await game.step({ frames: 5 });
    const [machine] = await game.entities.byTemplate(template);
    assert.ok(machine);
    if (name === "replicator") {
      await crossEarthTrainingTripwire(game, 313, 319);
      const [x, , z] = machine.position;
      await teleportVerified(game, { x: x - 1.59, y: 21.404, z: z - 2.23 });
      await game.step({ frames: 30 });
    } else {
      await standNear(game, machine.id);
    }
    await game.player.setStats({ cyber_modules: 100 });
    const nanites = await game.player.spawnItem("Nanites");
    await game.entities.sendMessage(nanites.entity_id, { type: "Frob" });
    await game.step({ frames: 3 });
    const before = (await game.info()).player.stats!;
    const aim = await game.player.aimAt(machine, { hitbox: "center", visibility: "required" });
    assert.ok(aim.target_confirmed, JSON.stringify(aim));
    await aimVrHandAt(game, aim.world_point);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 30 }); // Holding the opening press must not buy anything.
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });
    const ui = await game.ui.state();
    assert.equal(ui.mode, "use");
    assert.equal(ui.active_panel?.entity_id, machine.id);
    assert.deepEqual(ui.panel_pose?.canvas, [640, 480]);
    assert.equal((await game.physics.bodies()).bodies.filter(body =>
      body.collision_groups.includes("ui")).length, 0, "no separate world panel");
    const after = (await game.info()).player;
    assert.equal(after.hand_feedback!.body_gear!.personal_card.scans, 0);
    assert.equal(after.stats!.nanites, before.nanites);
    assert.equal(after.stats!.cyber_modules, before.cyber_modules);
    assert.deepEqual(after.stats!.os_traits, before.os_traits);
    assert.ok(ui.active_panel!.elements.some(element => element.kind === "button"));
    if (name === "stats trainer") {
      const button = ui.active_panel!.elements.find(element =>
        element.kind === "button" && element.label === "Endurance");
      assert.ok(button);
      await aimVrHandAtCanvas(game, ui.panel_pose!, canvasCenter(button));
      await game.step({ frames: 3 });
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 3 });
      await aimVrHandAt(game, aim.world_point);
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 30 });
      const reopened = await game.ui.state();
      assert.equal(reopened.active_panel?.entity_id, machine.id);
      await aimVrHandAtCanvas(game, reopened.panel_pose!, canvasCenter(button), { trigger: 1 });
      await game.step({ frames: 3 });
      assert.equal((await game.info()).player.stats!.cyber_modules, before.cyber_modules,
        "reopening with a held frob must not buy the hovered upgrade");
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 3 });
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 3 });
      assert.ok((await game.info()).player.stats!.cyber_modules < before.cyber_modules,
        "release and a fresh press must buy the upgrade");
    }
  });
}

test("VR free-hand frob opens the held tricorder's replicator MFD without scanning", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", debugFlags: ["--vr"] });
  await game.devParams.set("vr_mfd_focus_scan", 0);
  await game.step({ frames: 5 });
  await crossEarthTrainingTripwire(game, 313, 319);
  const [machine] = await game.entities.byTemplate(262);
  assert.ok(machine);
  const [x, , z] = machine.position;
  await teleportVerified(game, { x: x - 1.59, y: 21.404, z: z - 2.23 });
  await game.step({ frames: 30 });
  await drawPersonalCard(game, "left");
  const aim = await game.player.aimAt(machine, { hitbox: "center", visibility: "required" });
  assert.ok(aim.target_confirmed);
  await aimVrHandAt(game, aim.world_point);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 3 });
  const ui = await game.ui.state();
  assert.equal(ui.mode, "device");
  assert.equal(ui.active_panel?.entity_id, machine.id);
  assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.scans, 0);
  await aimMfdAt(game, aim.world_point, .55, 1, 1, { hand: "left" });
  await game.step({ frames: 3 });
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 3 });
  assert.equal((await game.ui.state()).mode, "shooter",
    "stowing while holding the scan trigger must not issue another world frob");
  await aimVrHandAt(game, aim.world_point, .45, 0, 0, { hand: "left" });
  await game.input.set("left_hand.trigger", 1);
  await game.step({ frames: 30 });
  assert.equal((await game.ui.state()).active_panel?.entity_id, machine.id,
    "release and a fresh frob after stowing must open the machine");
});
