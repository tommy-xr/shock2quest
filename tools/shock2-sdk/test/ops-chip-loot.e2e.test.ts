import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8194);

// Stable ops3 mission-object ids. Runtime ids are rediscovered after every
// launch and load.
const DOCILE = 125;
const CHIP_C = 840;

const cyberModules = async (game: GameServer) =>
  (await game.info()).player.stats?.cyber_modules;

test(
  "ops3: looting Docile's Chip C runs its reward and transfers it exactly once",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `ops_chip_c_e2e_${Date.now()}`;

    {
      await using game = await GameServer.launch({
        mission: "ops3.mis",
        port: basePort,
      });
      await game.step({ frames: 5 });

      const [docile] = await game.entities.byTemplate(DOCILE);
      const [chip] = await game.entities.byTemplate(CHIP_C);
      assert.ok(docile, "expected ops3 Docile mission object 125");
      assert.equal(chip?.name, "Chip C", "expected ops3 Chip C mission object 840");
      assert.equal(await cyberModules(game), 0, "fresh character starts with 0 modules");

      const containedBefore = (await game.entities.detail(docile.id)).outgoing_links.filter(
        (link) => link.link_type.startsWith("Contains"),
      );
      assert.ok(
        containedBefore.some((link) => link.target_id === chip.id),
        "Docile must contain the authored Chip C",
      );

      // Kill only stages the production corpse flow; the assertion below uses
      // the real creaturecontainer panel, semantic item button, BaseButton
      // SwitchLink, EXP trap, and engine MOVE action.
      for (let i = 0; i < 8; i++) {
        await game.entities.sendMessage(docile.id, { type: "Damage", amount: 50 });
        await game.step({ frames: 3 });
      }
      await game.step({ frames: 20 });

      const corpse = await game.entities.detail(docile.id);
      await teleportVerified(game, {
        x: corpse.position[0] + 1.0,
        y: corpse.position[1] + 0.5,
        z: corpse.position[2] + 1.0,
      });
      await game.entities.sendMessage(docile.id, { type: "Frob" });
      await game.step({ frames: 5 });

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
