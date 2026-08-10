import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
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
    assert.equal(
      (await game.entities.byTemplate(SYMPATHETIC_RESONATOR)).length,
      1,
      "the ordinary replicator purchase should dispense template -1671",
    );
  },
);
