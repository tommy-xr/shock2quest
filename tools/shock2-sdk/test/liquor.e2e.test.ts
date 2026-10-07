import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";

// Real gamesys Liquor, Vodka and Champagne, all inheriting the Liquor script.
const bottles = [-967, -964, -965];
const enabled = process.env.SHOCK2_E2E === "1";

for (const presentation of ["flat", "left", "right"] as const) {
  test(`Liquor heals, drains PSI and consumes once through ${presentation} inventory use`, {
    skip: !enabled, timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
      debugFlags: presentation === "flat" ? [] : ["--vr"],
    });
    await game.step({ frames: 3 });
    const initial = (await game.info()).player;
    assert.ok(initial.entity_id !== null && initial.hit_points! > 5);
    if (presentation === "flat") {
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 2 });
    }
    if (presentation !== "flat") await game.input.set(`${presentation}_hand.squeeze`, 1);
    for (const [index, template] of bottles.entries()) {
      // First bottle is used at full health; the others restore exactly one.
      if (index === 1) {
        await game.entities.sendMessage(initial.entity_id!, { type: "Damage", amount: 5 });
        await game.step({ frames: 1 });
      }
      const { entity_id: id } = await game.player.spawnItem(template,
        presentation === "flat" ? undefined : { hand: presentation });
      const before = (await game.info()).player;
      const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      if (presentation === "flat") {
        const slot = (await game.ui.state()).strip?.elements.find(e => e.entity_id === id);
        assert.ok(slot, "carried bottle is selectable");
        await clickUiElement(game, slot);
        await clickUiElement(game, slot);
      } else {
        await game.input.set(`${presentation}_hand.trigger`, 1);
        await game.step({ frames: 5 });
      }
      const after = (await game.info()).player;
      assert.equal(after.hit_points, Math.min(before.hit_points! + 1, before.max_hit_points!));
      assert.equal(after.psi_points, Math.max(0, before.psi_points! - 4));
      assert.ok(!(await game.player.inventory()).items.some(item => item.entity_id === id));
      assert.equal((await game.audio.recent()).sounds.filter(sound => sound.sequence > sequence &&
        sound.tags.some(([k, v]) => k === "event" && v === "activate") &&
        sound.tags.some(([k, v]) => k === "foodtype" && v === "drink")).length, 1);
      await game.step({ frames: 12 });
      assert.equal((await game.info()).player.psi_points, after.psi_points,
        "a held trigger cannot consume the destroyed bottle again");
      if (presentation !== "flat") {
        await game.input.set(`${presentation}_hand.trigger`, 0);
        await game.step({ frames: 1 });
      }
    }
    const beforeSave = (await game.info()).player;
    const save = `liquor-${presentation}`;
    assert.equal((await game.save(save)).success, true);
    assert.equal((await game.load(save)).success, true);
    const loaded = (await game.info()).player;
    assert.equal(loaded.hit_points, beforeSave.hit_points);
    assert.equal(loaded.psi_points, beforeSave.psi_points);
  });
}
