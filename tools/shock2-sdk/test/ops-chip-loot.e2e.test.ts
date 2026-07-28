import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8194);

// Stable ops3 mission-object ids. Runtime ids are rediscovered after every
// launch and load.
const DOCILE = 125;
const CHIP_C = 840;
const WRENCH = 1176;
const OPS3_BULKHEAD = 185;

const cyberModules = async (game: GameServer) =>
  (await game.info()).player.stats?.cyber_modules;

const property = (detail: EntityDetailResult, name: string) =>
  detail.properties.find((candidate) => candidate.name === name)?.value;

const squeeze = async (game: GameServer) => {
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 8 });
};

const swingWrench = async (game: GameServer) => {
  await game.input.set("right_hand.trigger_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger_value", 0);
  await game.step({ frames: 20 });
};

test(
  "ops3: after an authored bulkhead transition, normal combat and corpse UI transfer Chip C once",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `ops_chip_c_e2e_${Date.now()}`;

    // Start in another mission and cross a process boundary before entering
    // ops3. This matches the campaign's frontier-save restore and authored
    // level-transition lifecycle.
    {
      await using game = await GameServer.launch({
        mission: "ops2.mis",
        port: basePort,
      });
      await game.step({ frames: 5 });
      // Match frontier-009's raw ShodanRoom bits (1), which permits this
      // already-reached bulkhead without pretending the objective is complete.
      await game.quests.set("ShodanRoom", "incomplete");
      await game.save(saveName);
    }

    {
      await using game = await GameServer.launch({
        mission: "ops2.mis",
        port: basePort,
      });
      await game.load(saveName);
      const [bulkhead] = await game.entities.byTemplate(OPS3_BULKHEAD);
      assert.equal(
        bulkhead?.name,
        "Bulk_On_Button",
        "expected ops2's authored ops3 bulkhead object 185",
      );
      await teleportVerified(game, { x: 32.244, y: -8.596, z: 16.665 });
      await game.player.aimAt(bulkhead, {
        hitbox: "center",
        visibility: "required",
      });
      await squeeze(game);
      assert.equal(
        (await game.info()).mission.toLowerCase(),
        "ops3.mis",
        "normal squeeze on the authored ops2 bulkhead must transition to ops3",
      );

      const [docile] = await game.entities.byTemplate(DOCILE);
      const [chip] = await game.entities.byTemplate(CHIP_C);
      const [wrench] = await game.entities.byTemplate(WRENCH);
      assert.ok(docile, "expected ops3 Docile mission object 125");
      assert.equal(chip?.name, "Chip C", "expected ops3 Chip C mission object 840");
      assert.equal(wrench?.name, "Wrench", "expected ops3 Wrench mission object 1176");
      assert.equal(await cyberModules(game), 0, "fresh character starts with 0 modules");

      const containedBefore = (await game.entities.detail(docile.id)).outgoing_links.filter(
        (link) => link.link_type.startsWith("Contains"),
      );
      assert.ok(
        containedBefore.some((link) => link.target_id === chip.id),
        "Docile must contain the authored Chip C",
      );

      // Acquire the authored world Wrench through the production flat frob
      // edge, then use its normal first-person attack path for every hit.
      await teleportVerified(game, {
        x: wrench.position[0] + 1.225,
        y: wrench.position[1] + 0.634,
        z: wrench.position[2] + 0.917,
      });
      await game.player.aimAt(wrench, {
        hitbox: "center",
        visibility: "required",
      });
      await squeeze(game);

      const armedInventory = await game.player.inventory();
      assert.equal(
        armedInventory.items.find((item) => item.entity_id === wrench.id)?.location,
        "left_hand",
        `world-frobbing the Wrench must wield it; got ${JSON.stringify(armedInventory.items)}`,
      );
      assert.equal(
        (await game.info()).player.wielded_entity_id,
        wrench.id,
        "the authored Wrench must drive the production attack path",
      );

      let liveDocile = await game.entities.detail(docile.id);
      assert.equal(Number(property(liveDocile, "HitPoints")), 48);
      await teleportVerified(game, {
        x: liveDocile.position[0] + 0.599,
        y: liveDocile.position[1] - 0.695,
        z: liveDocile.position[2] - 0.865,
      });

      for (const expectedHitPoints of [42, 36, 30, 24, 18, 12, 6, 0]) {
        await game.player.aimAt(docile, {
          hitbox: "torso",
          visibility: "required",
        });
        await swingWrench(game);
        liveDocile = await game.entities.detail(docile.id);
        assert.equal(
          Number(property(liveDocile, "HitPoints")),
          expectedHitPoints,
          `an ordinary Wrench swing must reduce Docile to ${expectedHitPoints} HP`,
        );
      }
      assert.equal(property(liveDocile, "AIBehavior"), "Dead");
      assert.equal(
        (await game.info()).player.hit_points,
        30,
        "the docile encounter should not damage the player",
      );

      // The loot action uses the same aim/squeeze edge as a real corpse frob,
      // followed by the semantic UI click that fires both BaseButton's reward
      // and the engine's MOVE transfer.
      await game.player.aimAt(docile, {
        hitbox: "torso",
        visibility: "required",
      });
      await squeeze(game);

      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, "frobbing slain Docile should open its creature loot panel");
      const chipButton = panel.elements.find(
        (element) =>
          element.kind === "button" &&
          element.entity_id === chip.id &&
          element.label === "Chip C",
      );
      assert.ok(chipButton, "Docile's corpse panel should expose the Chip C button");

      await clickUiElement(game, chipButton);

      const inventory = await game.player.inventory();
      assert.equal(
        inventory.items.filter(
          (item) => item.entity_id === chip.id && item.location === "inventory",
        ).length,
        1,
        `Chip C must transfer to the backpack exactly once; got ${JSON.stringify(inventory.items)}`,
      );
      assert.equal(
        await cyberModules(game),
        10,
        "Chip C's BaseButton must fire its authored 10-module reward exactly once",
      );
      assert.ok(
        !(await game.entities.detail(docile.id)).outgoing_links.some(
          (link) => link.link_type.startsWith("Contains") && link.target_id === chip.id,
        ),
        "the transfer must remove Chip C from Docile's contents",
      );
      assert.ok(
        !(await game.ui.state()).active_panel?.elements.some(
          (element) => element.entity_id === chip.id,
        ),
        "the transferred Chip C must disappear from the live corpse panel",
      );

      await game.save(saveName);
    }

    // A new runtime proves both halves survive the production save format:
    // the unique physical chip remains carried and the one-shot reward does
    // not re-fire during load.
    await using reloaded = await GameServer.launch({
      mission: "ops3.mis",
      port: basePort,
    });
    await reloaded.load(saveName);
    await reloaded.step({ frames: 5 });

    const inventory = await reloaded.player.inventory();
    assert.equal(
      inventory.items.filter(
        (item) => item.name === "Chip C" && item.location === "inventory",
      ).length,
      1,
      `fresh-runtime load must retain exactly one Chip C; got ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      await cyberModules(reloaded),
      10,
      "fresh-runtime load must retain the single 10-module reward",
    );
  },
);
