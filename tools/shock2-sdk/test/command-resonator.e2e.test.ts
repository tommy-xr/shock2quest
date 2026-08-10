import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import type { UiElement, UiPanel } from "../src/types.js";
import { clickUiElement } from "./helpers/ui.js";

// Focused Command objective regression for #844. Runtime ids are discovered
// from stable authored ids on every launch/load. The full authored relays are:
//   tripwire 129 -> QB filter 1245 (ShuttleBBoom) -> replicator 394
//   Shuttle B 2392 -> QB filter 1252 (InRoom)     -> replicator 394
// This test injects their final TurnOn directly so it isolates the catalog
// script, then uses the real MFD/HRM/purchase flow.
//
// Negative-first evidence (2026-08-08, origin/main c07099af): after TurnOn,
// save/load, and a genuine successful hack, the panel still offers EMP Grenade
// in slot 1; `buy:big bomb` is absent because PutBombInReplicator is a no-op.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const COMMAND_REPLICATOR = 394;
const BIG_NANITE_PILE = -1591;
const SYMPATHETIC_RESONATOR = -1671;

function button(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(
    found,
    `panel should expose ${label}; got ${JSON.stringify(
      panel.elements.map((element) => element.label ?? element.texture),
    )}`,
  );
  return found;
}

function hasTexture(panel: UiPanel, texture: string): boolean {
  return panel.elements.some(
    (element) => element.texture?.toLowerCase() === texture,
  );
}

async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "replicator interaction should keep an MFD panel open");
  return panel;
}

async function openReplicator(game: GameServer, id: number): Promise<UiPanel> {
  await game.entities.sendMessage(id, { type: "Frob" });
  await game.step({ frames: 5 });
  return activePanel(game);
}

async function winHack(game: GameServer): Promise<void> {
  const routes = [
    ["node-2-0", "node-3-0", "node-4-0"],
    ["node-2-1", "node-2-2", "node-2-3"],
    ["node-0-1", "node-0-2", "node-0-3"],
    ["node-4-0", "node-4-1", "node-4-2"],
  ];
  for (let attempt = 0; attempt < 12; attempt += 1) {
    let panel = await activePanel(game);
    if (hasTexture(panel, "winh.pcx")) return;
    assert.ok(
      !hasTexture(panel, "loseh.pcx"),
      "max Hack and Cyber-Affinity should leave no critical-failure mines",
    );

    const inPlay = panel.elements.some(
      (element) => element.label === "reset-hack",
    );
    const burnedOut = hasTexture(panel, "failh.pcx");
    if (!inPlay || burnedOut) {
      const deal = panel.elements.find(
        (element) =>
          element.label === "start-hack" || element.label === "reset-hack",
      );
      assert.ok(deal, "an unwon board should offer START/RESET");
      await clickUiElement(game, deal);
    }

    for (const label of routes[attempt % routes.length]) {
      panel = await activePanel(game);
      if (hasTexture(panel, "winh.pcx")) return;
      if (hasTexture(panel, "failh.pcx")) break;
      await clickUiElement(game, button(panel, label));
    }
  }
  assert.fail(
    `the max-skill HRM route should win; rng=${game
      .logs()
      .filter((line) => line.includes("HRM rng"))
      .slice(-8)
      .join(" | ")}`,
  );
}

test(
  "command1 objective adds and dispenses the Sympathetic Resonator",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9440),
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    await game.step({ frames: 5 });

    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    for (let i = 0; i < 3; i += 1) {
      await game.player.spawnItem(BIG_NANITE_PILE);
    }

    const [replicator] = await game.entities.byTemplate(COMMAND_REPLICATOR);
    assert.ok(replicator, "command1 should instantiate authored replicator 394");
    await game.entities.sendMessage(replicator.id, { type: "TurnOn" });
    await game.step({ frames: 5 });

    // The objective mutates a registered Dark property, so crossing a real
    // save/load boundary before hacking must retain the new catalog.
    const saveName = `command_resonator_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });

    const [loadedReplicator] = await game.entities.byTemplate(COMMAND_REPLICATOR);
    assert.ok(loadedReplicator, "save/load should restore authored replicator 394");

    const [x, y, z] = (await game.entities.detail(loadedReplicator.id)).position;
    await game.player.teleport({ x: x + 2, y, z: z + 2 });
    await game.step({ frames: 5 });

    const beforeOpen = await game.info();
    assert.equal(
      beforeOpen.player.life_state,
      "alive",
      `fresh Command test player must remain alive: ${JSON.stringify(beforeOpen)}`,
    );

    const normal = await openReplicator(game, loadedReplicator.id);
    assert.ok(
      normal.elements.some((element) => element.label === "hack-replicator"),
      "the objective should leave a normal replicator available for hacking",
    );
    await clickUiElement(game, button(normal, "hack-replicator"));
    await clickUiElement(game, button(await activePanel(game), "start-hack"));
    await winHack(game);
    await clickUiElement(game, button(await activePanel(game), "close"));

    const hacked = await openReplicator(game, loadedReplicator.id);
    const resonatorButton = button(hacked, "buy:big bomb");
    assert.ok(
      hacked.elements.some(
        (element) => element.kind === "text" && element.text === "100",
      ),
      "the augmented hacked slot should retain retail's 100-nanite cost",
    );

    assert.equal(
      (await game.entities.byTemplate(SYMPATHETIC_RESONATOR)).length,
      0,
      "the unique objective item should not exist before purchase",
    );
    await clickUiElement(game, resonatorButton);
    await game.step({ frames: 10 });
    const resonators = await game.entities.byTemplate(SYMPATHETIC_RESONATOR);
    assert.equal(
      resonators.length,
      1,
      "the ordinary replicator purchase should dispense template -1671",
    );

    const resonator = resonators[0];
    const worldDetail = await game.entities.detail(resonator.id);
    const replicatorContainsResonator = (
      await game.entities.detail(loadedReplicator.id)
    ).outgoing_links.some(
      (link) =>
        link.link_type.startsWith("Contains") &&
        link.target_id === resonator.id,
    );
    assert.equal(
      replicatorContainsResonator,
      false,
      "the replicator must not author a runtime Contains link to its own output",
    );
    assert.equal(
      worldDetail.incoming_links.filter((link) =>
        link.link_type.startsWith("Contains"),
      ).length,
      0,
      "the replicator hopper must not convert its dispensed item into hidden containment",
    );
    assert.notEqual(
      worldDetail.properties
        .find((property) => property.name === "HasRefs")
        ?.value.toLowerCase(),
      "false",
      "the world item must retain references instead of being hidden as container contents",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: resonator.id })).bodies.length,
      1,
      "the dispensed resonator must retain its authored world collider",
    );
    assert.equal(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === resonator.id,
      ),
      false,
      "the hopper item must remain in the world until the player picks it up",
    );

    await clickUiElement(game, button(await activePanel(game), "close"));
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "closing the replicator MFD should return control to world interaction",
    );

    const [bombX, bombY, bombZ] = worldDetail.position;
    await game.player.teleport({
      x: bombX + 0.2,
      y: bombY - PLAYER_EYE_HEIGHT_WORLD,
      z: bombZ,
    });
    const aim = await game.player.aimAt(resonator, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      aim.target_confirmed,
      true,
      `the hopper item should expose a selectable surface: ${JSON.stringify(aim)}`,
    );

    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 5 });

    const carried = (await game.player.inventory()).items.find(
      (item) => item.entity_id === resonator.id,
    );
    assert.equal(
      carried?.location,
      "inventory",
      `an ordinary visible world squeeze should pick up the resonator: ${JSON.stringify(carried)}`,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: resonator.id })).bodies.length,
      0,
      "the picked-up resonator should no longer retain a world body",
    );

    const pickupSave = `command_resonator_picked_up_${Date.now()}`;
    assert.equal((await game.save(pickupSave)).success, true);
    assert.equal((await game.load(pickupSave)).success, true);
    await game.step({ frames: 5 });

    const loadedResonators = await game.entities.byTemplate(SYMPATHETIC_RESONATOR);
    assert.equal(
      loadedResonators.length,
      1,
      "save/load must restore the unique purchased resonator",
    );
    const loadedInventory = await game.player.inventory();
    assert.equal(
      loadedInventory.items.find(
        (item) => item.entity_id === loadedResonators[0].id,
      )?.location,
      "inventory",
      `save/load must preserve the picked-up resonator: ${JSON.stringify(loadedInventory.items)}`,
    );
  },
);
