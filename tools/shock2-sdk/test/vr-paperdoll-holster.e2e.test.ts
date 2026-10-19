import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";

// Paperdoll wells in /v1/ui and their storage slot (storage is right, left).
for (const [label, slot] of [["Right holster", 0], ["Left holster", 1]] as const) {
  for (const hand of ["right", "left"] as const) {
    test(`VR: ${hand} hand drags a pistol from the backpack onto the ${label} well`, { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
      await game.step({ frames: 5 });
      const existing = new Set((await game.entities.list()).entities.map(e => e.id));
      await game.player.spawnItem(-17);
      const item = (await game.entities.list()).entities.find(e => e.template_id === -17 && !existing.has(e.id))!;
      assert.ok(item);
      const ammo = ammoOf(await game.entities.detail(item.id));
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      const ui = await game.ui.state();
      const center = (rect: number[]): [number, number] => [rect[0] + rect[2] / 2, rect[1] + rect[3] / 2];
      const cell = ui.strip?.elements.find(e => e.entity_id === item.id);
      const well = ui.strip?.elements.find(e => e.label === label);
      assert.ok(cell && well);

      // Squeeze the cell, carry it across the canvas onto the well, release.
      await aimVrHandAtCanvas(game, ui.panel_pose!, center(cell.rect), { hand, squeeze: 1 });
      await game.step({ frames: 5 });
      const owner = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
      assert.equal((await game.info()).player[owner], item.id);
      await aimVrHandAtCanvas(game, ui.panel_pose!, center(well.rect), { hand, squeeze: 1 });
      await game.step({ frames: 5 });
      assert.equal((await game.info()).player[owner], item.id, "hovering is not releasing");
      await game.input.set(`${hand}_hand.squeeze`, 0);
      await game.step({ frames: 8 });

      const player = (await game.info()).player;
      assert.equal(player[owner], null);
      assert.equal(player.hand_feedback?.holsters?.items[slot], item.id);
      assert.equal((await game.player.inventory()).items.find(i => i.entity_id === item.id), undefined);
      assert.equal((await game.physics.bodies({ entityId: item.id })).bodies.length, 0);
      assert.equal(ammoOf(await game.entities.detail(item.id)), ammo);
      const shown = (await game.ui.state()).strip?.elements.find(e => e.label === label);
      assert.equal(shown?.entity_id, item.id, "the paperdoll shows the newly holstered pistol");
    });
  }
}

test("VR: an occupied paperdoll well refuses a second item and keeps it in hand", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  const existing = new Set((await game.entities.list()).entities.map(e => e.id));
  await game.player.spawnItem(-17);
  await game.player.spawnItem(-928);
  const entities = (await game.entities.list()).entities.filter(e => !existing.has(e.id));
  const pistol = entities.find(e => e.template_id === -17)!;
  const wrench = entities.find(e => e.template_id === -928)!;
  assert.ok(pistol && wrench);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const center = (rect: number[]): [number, number] => [rect[0] + rect[2] / 2, rect[1] + rect[3] / 2];
  const drag = async (entity: number) => {
    const ui = await game.ui.state();
    const cell = ui.strip!.elements.find(e => e.entity_id === entity)!;
    const well = ui.strip!.elements.find(e => e.label === "Right holster")!;
    await aimVrHandAtCanvas(game, ui.panel_pose!, center(cell.rect), { squeeze: 1 });
    await game.step({ frames: 5 });
    await aimVrHandAtCanvas(game, ui.panel_pose!, center(well.rect), { squeeze: 1 });
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
  };
  await drag(pistol.id);
  assert.equal((await game.info()).player.hand_feedback?.holsters?.items[0], pistol.id);
  await drag(wrench.id);
  const player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, wrench.id, "a refused item stays in the hand");
  assert.equal(player.hand_feedback?.holsters?.items[0], pistol.id);
});
