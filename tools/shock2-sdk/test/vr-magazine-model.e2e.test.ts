import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { drawPouchAmmo } from "./helpers/ammo-pouch.js";
import { ammoOf } from "./helpers/weapon.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

for (const [model, template] of [["atek_h", -17], ["ar15_h", -18]] as const) {
  for (const hand of ["left", "right"] as const) {
    test(`${model} ${hand}: pouch magazine keeps ammo identity, fitted scale and save appearance`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 30 });
      const primary = hand === "left" ? "right" : "left";
      const spawned = await game.player.spawnItem(template);
      const gun = { id: spawned.entity_id };
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      const ui = await game.ui.state();
      const cell = ui.strip?.elements.find(e => e.entity_id === gun.id);
      assert.ok(cell && ui.panel_pose);
      await aimVrHandAtCanvas(game, ui.panel_pose, [cell.rect[0] + cell.rect[2] / 2, cell.rect[1] + cell.rect[3] / 2], { hand: primary, squeeze: 1 });
      await game.step({ frames: 5 });
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
      await game.input.set(`${primary}_hand.position`, [primary === "left" ? -.3 : .3, 1, -.5]);
      await game.input.set(`${primary}_hand.rotation`, [0, 0, 0, 1]);
      await game.step({ frames: 10 });
      await game.input.trigger("EjectClip");
      await game.step({ frames: 5 });
      await game.player.spawnItem(-31);
      await game.step({ frames: 5 });
      const { offer, player: held, entityId: id } = await drawPouchAmmo(game, hand);
      const detail = await game.entities.detail(id);
      const appearance = detail.properties.find(p => p.name === "MagazineModel");
      assert.ok(appearance, "pouch ammo must use the extracted magazine");
      const magazine = JSON.parse(appearance.value);
      assert.equal(magazine.source, model);
      assert.equal(Number(detail.properties.find(p => p.name === "StackCount")?.value), offer.rounds);
      const grip = held.hand_grips.find(g => g.entity_id === id);
      const gunGrip = held.hand_grips.find(g => g.entity_id === gun.id);
      assert.ok(grip?.grip && grip.item_bounds && gunGrip?.grip);
      assert.equal(grip.grip.item_scale, gunGrip.grip.item_scale, "magazine must retain its weapon's physical size");
      const [min, max] = grip.item_bounds;
      assert.ok(Math.max(...max.map((v, i) => v - min[i]!)) < .3, "bounds must exclude the source gun and arms");
      assert.ok((await game.scene.objects({ entityId: id })).objects.length > 0, "extracted mesh must render");
      // Move away from body slots before saving, to avoid a restore release
      // becoming a pouch return or a magazine insert.
      await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.6 : .6, 1.1, -.5]);
      await game.step({ frames: 5 });
      const save = `magazine-${model}-${hand}-${Date.now()}`;
      await game.save(save);
      await game.load(save);
      await game.step({ frames: 5 });
      const restored = (await game.info()).player;
      const restoredId = hand === "left" ? restored.wielded_entity_id : restored.right_hand_entity_id;
      assert.ok(restoredId != null);
      const saved = await game.entities.detail(restoredId);
      const restoredGun = primary === "left" ? restored.wielded_entity_id : restored.right_hand_entity_id;
      assert.ok(restoredGun != null);
      const seating = (await game.entities.detail(restoredGun)).properties.find(p => p.name === "MagazineSeated");
      assert.ok(seating);
      assert.deepEqual(JSON.parse(seating.value), { present: false, rendered: false }, "removed magazine stays absent after save/load");
      assert.deepEqual(JSON.parse(saved.properties.find(p => p.name === "MagazineModel")!.value), magazine);
      assert.equal(Number(saved.properties.find(p => p.name === "StackCount")?.value), offer.rounds);
      assert.ok((await game.scene.objects({ entityId: restoredId })).objects.length > 0);
      await game.input.set(`${hand}_hand.squeeze`, 1);
      await game.step({ frames: 2 });
      await game.input.set(`${hand}_hand.squeeze`, 0);
      await game.step({ frames: 2 });
      const loose = (await game.scene.objects({ entityId: restoredId })).objects;
      assert.ok(loose.length > 0);
      for (const object of loose) {
        assert.ok(object.scale.every(s => Math.abs(s - magazine.item_scale) < 1e-5),
          "dropping must retain the magazine's physical size");
      }
      await game.save(save);
      await game.load(save);
      await game.step({ frames: 2 });
      let looseId: number | undefined;
      for (const entity of (await game.entities.list()).entities.filter(e => e.template_id === saved.template_id)) {
        const detail = await game.entities.detail(entity.id);
        if (detail.properties.some(p => p.name === "MagazineModel")) { looseId = entity.id; break; }
      }
      assert.ok(looseId != null, "loose magazine appearance survives save/load");
      for (const object of (await game.scene.objects({ entityId: looseId })).objects) {
        assert.ok(object.scale.every(s => Math.abs(s - magazine.item_scale) < 1e-5));
      }
      if (model === "atek_h" && hand === "right") {
        await using flat = await GameServer.launch({ mission: "medsci1.mis" });
        await flat.load(save);
        await flat.input.trigger("EquipPistol");
        await flat.step({ frames: 10 });
        await flat.input.trigger("Reload");
        await flat.step({ frames: 300 });
        const flatGun = (await flat.info()).player.wielded_entity_id;
        assert.ok(flatGun != null);
        assert.ok(ammoOf(await flat.entities.detail(flatGun)) > 0);
        await flat.save(`${save}-flat`);
        await game.load(`${save}-flat`);
        await game.step({ frames: 8 });
        const returned = (await game.info()).player.wielded_entity_id;
        assert.ok(returned != null);
        const restoredSeating = (await game.entities.detail(returned)).properties.find(p => p.name === "MagazineSeated");
        assert.ok(restoredSeating);
        assert.deepEqual(JSON.parse(restoredSeating.value), { present: true, rendered: true },
          "a successful flat reload restores the magazine when returning to VR");
      }
    });
  }
}
