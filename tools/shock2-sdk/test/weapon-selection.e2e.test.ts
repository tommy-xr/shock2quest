import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression for the original System Shock 2 direct-weapon
// bindings. Debug provisioning establishes the backpack contents, but every
// selection below goes through the production InputAction -> wield path: it
// must select the matching carried entity, never spawn a replacement, and
// holster the displaced weapon back into the inventory.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const WEAPON_BINDINGS = [
  { key: "1", action: "EquipWrench", templateId: -928 },
  { key: "2", action: "EquipPistol", templateId: -17 },
  { key: "3", action: "EquipShotgun", templateId: -19 },
  { key: "4", action: "EquipAssaultRifle", templateId: -18 },
  { key: "5", action: "EquipLaserPistol", templateId: -22 },
  { key: "6", action: "EquipEmpRifle", templateId: -23 },
  { key: "7", action: "EquipElectroShock", templateId: -24 },
  { key: "8", action: "EquipGrenadeLauncher", templateId: -21 },
  { key: "9", action: "EquipStasisFieldGenerator", templateId: -25 },
  { key: "0", action: "EquipFusionCannon", templateId: -26 },
  { key: "-", action: "EquipCrystalShard", templateId: -28 },
  { key: "=", action: "EquipViralProliferator", templateId: -29 },
  { key: "\\", action: "EquipWormLauncher", templateId: -27 },
  { key: "`", action: "EquipPsiAmp", templateId: -247 },
] as const;

test(
  "original weapon keys wield matching carried items without spawning or dropping",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
    });
    await game.step({ frames: 5 });

    await game.input.trigger("EquipPistol");
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.wielded_entity_id,
      null,
      "selecting an unowned weapon must be a no-op",
    );
    assert.equal(
      (await game.player.inventory()).count,
      0,
      "selecting an unowned weapon must not spawn it",
    );

    const equipped = new Map<string, number>();
    for (const binding of WEAPON_BINDINGS) {
      const item = await game.player.spawnItem(binding.templateId);
      equipped.set(binding.action, item.entity_id);
    }

    const originalInventory = await game.player.inventory();
    const originalIds = originalInventory.items
      .map((item) => item.entity_id)
      .sort((a, b) => a - b);
    assert.equal(originalInventory.count, WEAPON_BINDINGS.length);

    const actions = await game.input.actions();
    for (const binding of WEAPON_BINDINGS) {
      assert.ok(
        actions.includes(binding.action),
        `${binding.key} action ${binding.action} should be available`,
      );

      await game.input.trigger(binding.action);
      await game.step({ frames: 2 });

      const expectedEntity = equipped.get(binding.action);
      assert.equal(
        (await game.info()).player.wielded_entity_id,
        expectedEntity,
        `${binding.key} should wield the carried ${binding.action} entity`,
      );

      const carried = await game.player.inventory();
      assert.equal(
        carried.count,
        WEAPON_BINDINGS.length,
        `${binding.key} must not spawn, drop, or lose a weapon`,
      );
      assert.deepEqual(
        carried.items.map((item) => item.entity_id).sort((a, b) => a - b),
        originalIds,
        `${binding.key} must preserve the exact carried entity set`,
      );
      assert.equal(
        carried.items.find((item) => item.entity_id === expectedEntity)?.location,
        "left_hand",
        `${binding.key} should move only its matching carried entity into the flat wield slot`,
      );
    }
  },
);
