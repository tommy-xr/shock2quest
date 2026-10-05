import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable ops2 mission-object ids. Runtime ids are rediscovered each launch.
const RED_ASSASSIN = 254;
const CHIP_A = 554;
const EXPERIENCE_TRAP = 1089;
const uiBodies = async (game: GameServer): Promise<PhysicsBodySummary[]> =>
  (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );

async function triggerClick(game: GameServer): Promise<void> {
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 8 });
}

test(
  "ops2 VR trigger-click runs Chip A's reward once and transfers it from the corpse",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8554),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const [assassin] = await game.entities.byTemplate(RED_ASSASSIN);
    const [chip] = await game.entities.byTemplate(CHIP_A);
    const [experienceTrap] = await game.entities.byTemplate(EXPERIENCE_TRAP);
    assert.ok(assassin, "expected ops2 Red Assassin mission object 254");
    assert.equal(chip?.name, "Chip A", "expected ops2 Chip A mission object 554");
    assert.ok(experienceTrap, "expected Chip A Experience Trap mission object 1089");
    assert.equal((await game.info()).player.stats?.cyber_modules, 0);
    assert.ok(
      (await game.entities.detail(assassin.id)).outgoing_links.some(
        (link) => link.link_type.startsWith("Contains") && link.target_id === chip.id,
      ),
      "the Red Assassin must initially contain Chip A",
    );

    // Lethal damage is test setup only. Opening the corpse and clicking its
    // rendered world panel below both use the production VR trigger ray.
    await game.entities.sendMessage(assassin.id, { type: "Damage", amount: 100 });
    await game.step({ frames: 120 });
    const corpse = await game.entities.detail(assassin.id);
    assert.equal(
      corpse.properties.find((property) => property.name === "AIBehavior")?.value,
      "Dead",
      "lethal setup damage must leave a lootable corpse",
    );

    await teleportVerified(game, {
      x: corpse.position[0] + 1.2,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.2,
    });
    const corpseAim = await game.player.aimAt(assassin, {
      hitbox: "torso",
      visibility: "required",
    });
    await aimVrHandAt(game, corpseAim.world_point);
    await triggerClick(game);

    const panels = await uiBodies(game);
    assert.equal(panels.length, 0, "corpse loot must use the cyber-interface MFD");
    const ui = await game.ui.state();
    const slot = ui.active_panel?.elements.find(e => e.entity_id === chip.id && e.kind === "button");
    assert.ok(slot);
    assert.ok(ui.panel_pose);
    const [x, y, w, h] = slot.rect;
    await aimVrHandAtCanvas(game, ui.panel_pose, [x + w / 2, y + h / 2]);
    await triggerClick(game);

    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.filter(
        (item) => item.entity_id === chip.id && item.location === "inventory",
      ).length,
      1,
      `one trigger-click must put one Chip A in the backpack: ${JSON.stringify(inventory.items)}`,
    );
    assert.ok(
      !(await game.entities.detail(assassin.id)).outgoing_links.some(
        (link) => link.link_type.startsWith("Contains") && link.target_id === chip.id,
      ),
      "the transfer must remove the corpse's Contains link to Chip A",
    );
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      10,
      "Chip A's authored Experience side effect must run exactly once",
    );
    assert.equal(
      (await game.entities.byTemplate(EXPERIENCE_TRAP)).length,
      0,
      "TrapEXPOnce must consume the authored Experience Trap",
    );

    // Repeating the same production trigger edge over the now-empty slot must
    // neither duplicate the item nor re-award the destroyed one-shot trap.
    await aimVrHandAtCanvas(game, ui.panel_pose, [x + w / 2, y + h / 2]);
    await triggerClick(game);
    assert.equal((await game.info()).player.stats?.cyber_modules, 10);
    assert.equal(
      (await game.player.inventory()).items.filter((item) => item.entity_id === chip.id).length,
      1,
    );
  },
);
