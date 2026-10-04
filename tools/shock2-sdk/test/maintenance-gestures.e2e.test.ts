import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { clickUiElement } from "./helpers/ui.js";
import { cycleToWeapon } from "./helpers/weapon.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };
const TOOL = -2949;

function gunCondition(detail: { properties: { name: string; value: string }[] }): number {
  const prop = detail.properties.find(p => p.name === "Condition");
  assert.ok(prop, "gun exposes condition");
  return Number(prop.value);
}

test("dragging a maintenance tool onto an inventory gun maintains that gun", options, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 30 });
  await game.input.trigger("DebugCycleWeapon");
  await game.step({ frames: 10 });
  const gun = (await game.info()).player.wielded_entity_id!;
  await game.entities.sendMessage(gun, { type: "SetGunCondition", condition: 20 });
  const tool = (await game.entities.byTemplate(TOOL))[0];
  await game.player.give(tool.id);
  await game.player.give(gun);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const elements = (await game.ui.state()).strip!.elements;
  const source = elements.find(e => e.kind === "button" && e.entity_id === tool.id)!;
  const target = elements.find(e => e.kind === "button" && e.entity_id === gun)!;
  assert.ok(source); assert.ok(target);
  await clickUiElement(game, source);
  const [x, y, w, h] = target.screen_rect;
  await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
  await game.step({ frames: 3 });
  assert.match((await game.ui.state()).name_strip ?? "", /Maintain 2\.0 -> 8\.0.*consumes 1 tool/);
  assert.equal(gunCondition(await game.entities.detail(gun)), 20, "hover previews without applying maintenance");
  await clickUiElement(game, target);
  await game.step({ frames: 5 });
  assert.equal(gunCondition(await game.entities.detail(gun)), 80);
  assert.equal((await game.entities.byTemplate(TOOL)).length, 0, "successful maintenance consumes the tool");
  assert.equal((await game.ui.state()).cursor, null, "consumed tool clears the cursor");
});

for (const held of [false, true]) {
  for (const gesture of ["trigger", "release"] as const) {
    test(`VR maintenance ${gesture} near a ${held ? "held" : "world"} gun`, options, async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
      await game.step({ frames: 30 });
      const gun = await cycleToWeapon(game, e => e.template_id === -17);
      if (!held) await game.step({ frames: 120 }); // Let the spawned world gun land before aiming.
      const gunHand = await aimVrHandAt(game, (await game.entities.detail(gun.id)).position as Vec3, 0.3);
      if (held) {
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
      }
      await game.entities.sendMessage(gun.id, { type: "SetGunCondition", condition: 20 });
      const tool = (await game.entities.byTemplate(TOOL))[0];
      await aimVrHandAt(game, tool.position as Vec3, 0.3, 0, 0, { hand: "left" });
      await game.input.set("left_hand.squeeze", 1);
      await game.step({ frames: 8 });
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === tool.id && i.location === "left_hand"));
      if (held) {
        await game.input.set("left_hand.position", [gunHand.local[0] + 0.1, gunHand.local[1], gunHand.local[2]]);
      } else {
        await aimVrHandAt(game, (await game.entities.detail(gun.id)).position as Vec3, 0.1, 1, 0, { hand: "left" });
      }
      await game.step({ frames: 3 });
      assert.ok((await game.ui.state()).messages?.some(m => m.includes("Maintain 2.0 -> 8.0") && m.includes("consumes 1 tool")), "nearby tool previews its effect");
      assert.equal(gunCondition(await game.entities.detail(gun.id)), 20, "preview does not perform maintenance");
      const near = (await game.input.state()).left_hand.position;
      await game.input.set("left_hand.position", [near[0] + 2, near[1], near[2]]);
      await game.step({ frames: 3 });
      assert.ok(!(await game.ui.state()).messages?.some(m => m.includes("consumes 1 tool")), "preview disappears away from the gun");
      await game.input.set("left_hand.position", near);
      await game.step({ frames: 3 });
      await game.input.set(gesture === "trigger" ? "left_hand.trigger" : "left_hand.squeeze", gesture === "trigger" ? 1 : 0);
      await game.step({ frames: 8 });
      assert.equal(gunCondition(await game.entities.detail(gun.id)), 80);
      assert.equal((await game.entities.byTemplate(TOOL)).length, 0, "successful maintenance consumes the tool");
      assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === tool.id), "consumed tool clears the hand");
    });
  }
}
