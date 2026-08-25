import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanel } from "../src/types.js";
import { carriedNaniteTotal } from "./helpers/earth-replicator.js";
import { clickUiElement } from "./helpers/ui.js";

// The fixture is the verified boss-entry save produced by the end-to-end
// play-through that found #710. It is intentionally supplied out of tree:
// retail saves contain copyrighted mission state and must not be committed.
// The assertion itself uses production aim + squeeze from the saved position;
// there is no debug teleport or injected Frob message.
const bossSave = process.env.SHOCK2_SHODAN_BOSS_SAVE;
const e2eEnabled = process.env.SHOCK2_E2E === "1" && Boolean(bossSave);

const CPUS = [268, 264, 262] as const;
const HACKED_SHODAN_COMPUTER = 1269;
const SHODAN_HEAD = 298;
const SHIELDS = [270, 272, 274, 275, 277, 278, 279, 280] as const;

function button(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(found, `panel should expose button ${label}`);
  return found;
}

async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "computer interaction should keep an MFD panel open");
  return panel;
}

async function closePanel(game: GameServer): Promise<void> {
  const panel = (await game.ui.state()).active_panel;
  const close = panel?.elements.find((element) => element.label === "close");
  if (close) {
    await clickUiElement(game, close);
  }
}

async function backAwayFromComputer(game: GameServer): Promise<void> {
  await closePanel(game);
  // The authored hacked Corpse replacement occupies the CPU's full cabinet.
  // Back away through ordinary locomotion before taking the next bounded
  // collision-valid move, so the player does not begin that shape cast in
  // contact with the freshly instantiated cabinet.
  await game.input.set("right_hand.thumbstick", [0, -1]);
  await game.step({ frames: 15 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 2 });
}

async function approach(game: GameServer, templateId: number): Promise<void> {
  const start = await game.player.position();
  const staging =
    templateId === CPUS[1]
      ? [
          { x: 25.56, y: start.y, z: 62.4 },
          // Stay west of the corner's contact band. The original 2.4-foot
          // player footprint cannot reach the old point at (25.15, 60.05),
          // but this nearby pose is collision-valid for both footprints and
          // preserves the same outer route around the shield chamber.
          { x: 24.9, y: start.y, z: 60.25 },
          { x: 28.8, y: start.y, z: 59.84 },
          { x: 32, y: start.y, z: 59.84 },
          { x: 35.2, y: start.y, z: 59.84 },
          { x: 38.51, y: start.y, z: 59.84 },
          { x: 41.6, y: start.y, z: 61.82 },
          { x: 43.53, y: start.y, z: 64.36 },
          { x: 41, y: start.y, z: 65.3 },
        ]
      : templateId === CPUS[2]
        ? [
            { x: 43.53, y: start.y, z: 64.36 },
            { x: 43.95, y: start.y, z: 65.15 },
            { x: 44.15, y: start.y, z: 65.5 },
            { x: 44, y: start.y, z: 68.8 },
            { x: 44, y: start.y, z: 72 },
            { x: 44, y: start.y, z: 75.2 },
            { x: 42, y: start.y, z: 80 },
          ]
        : [];
  for (const waypoint of staging) {
    for (let hop = 0; hop < 6; hop += 1) {
      const position = await game.player.position();
      if (Math.hypot(waypoint.x - position.x, waypoint.z - position.z) < 0.2) {
        break;
      }
      const moved = await game.player.moveTo(waypoint);
      if (moved.blocked) {
        const remaining = Math.hypot(
          waypoint.x - moved.new_position[0],
          waypoint.z - moved.new_position[2],
        );
        assert.ok(
          remaining < 0.3,
          `the authored outer route to computer ${templateId} should stay collision-clear; ` +
            `waypoint=${JSON.stringify(waypoint)} result=${JSON.stringify(moved)}`,
        );
        break;
      }
    }
  }
  for (let hop = 0; hop < 6; hop += 1) {
    const [entity] = await game.entities.byTemplate(templateId);
    assert.ok(entity, `computer ${templateId} should still exist while approaching it`);
    const detail = await game.entities.detail(entity.id);
    const position = await game.player.position();
    const distance = Math.hypot(
      detail.position[0] - position.x,
      detail.position[2] - position.z,
    );
    if (distance < 2.5) {
      return;
    }
    const moved = await game.player.moveTo({
      x: detail.position[0],
      y: position.y,
      z: detail.position[2],
    });
    assert.ok(
      moved.moved || moved.blocked,
      `bounded move toward ${templateId} should either advance or meet its surface`,
    );
    if (moved.blocked) {
      return;
    }
  }
}

async function frobComputer(game: GameServer, templateId: number): Promise<void> {
  await approach(game, templateId);
  const [computer] = await game.entities.byTemplate(templateId);
  assert.ok(computer, `authored computer ${templateId} should exist`);
  const aim = await game.player
    .aimAt(computer, {
      hitbox: "center",
      visibility: "required",
    })
    .catch(async (error: unknown) => {
      throw new Error(
        `could not see computer ${templateId} after bounded approach; ` +
          `player=${JSON.stringify(await game.player.position())}; ` +
          `detail=${JSON.stringify(await game.entities.detail(computer.id))}; ` +
          `aim=${JSON.stringify(
            error && typeof error === "object" && "result" in error
              ? error.result
              : String(error),
          )}`,
      );
    });
  assert.equal(aim.entity_id, computer.id);
  assert.equal(aim.classification, "surface");
  assert.equal(aim.visibility.state, "visible");

  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 2 });

  const panel = await activePanel(game);
  assert.equal(panel.template_id, templateId);
  assert.ok(
    panel.elements.some(
      (element) => element.texture?.toLowerCase() === "hack.pcx",
    ),
    "the Computer panel should render the shared authored HRM board",
  );
  assert.ok(
    panel.elements.some(
      (element) =>
        element.kind === "text" &&
        element.text?.includes("Hack all three Interlocks"),
    ),
    `the board should resolve Shield_Interlock through HACKTEXT.STR; text=${JSON.stringify(
      panel.elements
        .filter((element) => element.kind === "text")
        .map((element) => element.text),
    )}`,
  );
}

async function playComputerHrm(
  game: GameServer,
  templateId: number,
): Promise<void> {
  await frobComputer(game, templateId);
  const route = [
    "node-2-0",
    "node-3-0",
    "node-4-0",
    "node-2-1",
    "node-2-2",
    "node-2-3",
    "node-0-1",
    "node-1-1",
    "node-4-1",
    "node-0-2",
    "node-3-2",
    "node-4-2",
    "node-0-3",
    "node-1-3",
  ];

  for (let attempt = 0; attempt < 5; attempt += 1) {
    const unpaid = await activePanel(game);
    const start =
      unpaid.elements.find((element) => element.label === "start-hack") ??
      unpaid.elements.find((element) => element.label === "reset-hack");
    assert.ok(start, `attempt ${attempt + 1} should expose START/RESET`);
    const nanitesBeforePayment = await carriedNaniteTotal(game);
    await clickUiElement(game, start);
    assert.equal(
      await carriedNaniteTotal(game),
      nanitesBeforePayment - 3,
      "each real Computer HRM attempt should debit its authored 3-nanite cost",
    );

    for (const label of route) {
      if ((await game.entities.byTemplate(templateId)).length === 0) {
        return;
      }
      const panel = await activePanel(game);
      if (
        panel.elements.some(
          (element) => element.texture?.toLowerCase() === "failh.pcx",
        )
      ) {
        break;
      }
      await clickUiElement(game, button(panel, label));
      await game.step({ frames: 2 });
    }
  }

  const finalPanel = (await game.ui.state()).active_panel;
  assert.fail(
    `five real HRM attempts did not replace computer ${templateId}; ` +
      `textures=${JSON.stringify(finalPanel?.elements.map((element) => element.texture))}; ` +
      `inventory=${JSON.stringify((await game.player.inventory()).items)}; rng=${game
        .logs()
        .filter((line) => line.includes("HRM rng"))
        .join(" | ")}`,
  );
}

test(
  "SHODAN finale: hack all three interlocks through real UI, persist, expose head",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    assert.ok(bossSave);
    assert.equal((await game.load(bossSave)).success, true);
    await game.step({ frames: 5 });

    const shieldCountBefore = (
      await Promise.all(SHIELDS.map((template) => game.entities.byTemplate(template)))
    ).reduce((count, shields) => count + shields.length, 0);
    assert.equal(shieldCountBefore, SHIELDS.length, "all eight authored shields should start intact");
    const [headBefore] = await game.entities.byTemplate(SHODAN_HEAD);
    assert.ok(headBefore, "the authored SHODAN head should exist behind its shields");
    for (const templateId of CPUS) {
      const [computer] = await game.entities.byTemplate(templateId);
      assert.ok(computer);
      const detail = await game.entities.detail(computer.id);
      assert.ok(
        detail.outgoing_links.some((link) =>
          link.link_type.toLowerCase().includes("hacking"),
        ),
        `loaded legacy save should restore CPU ${templateId}'s authored HackingLink; ` +
          `links=${JSON.stringify(detail.outgoing_links)}`,
      );
    }

    // The campaign-found boss fixture reached this room with no nanites, while
    // retail Computer HRM charges 3 per interlock and shodan.mis places no
    // collectible piles. Verify the genuine refusal path before staging only
    // one genuine 20-Nanite stack (enough for bounded HRM retries). No success,
    // Frob, script message, replacement, or shield state is injected.
    await frobComputer(game, CPUS[0]);
    await clickUiElement(game, button(await activePanel(game), "start-hack"));
    const refused = await activePanel(game);
    assert.ok(
      refused.elements.some(
        (element) => element.texture?.toLowerCase() === "payh.pcx",
      ),
      "a zero-nanite player should receive the authored PAYH refusal",
    );
    assert.equal(
      (await game.entities.byTemplate(CPUS[0])).length,
      1,
      "PAYH must not activate or replace the interlock",
    );
    await closePanel(game);
    await game.player.spawnItem("20 Nanites");
    assert.equal(
      await carriedNaniteTotal(game),
      20,
      "fixture staging should provide one genuine, spendable 20-Nanite stack",
    );

    const hackedBefore = (
      await game.entities.byTemplate(HACKED_SHODAN_COMPUTER)
    ).length;
    await playComputerHrm(game, CPUS[0]);
    await game.step({ frames: 20 });
    await backAwayFromComputer(game);
    assert.equal(
      (await game.entities.byTemplate(300)).length,
      0,
      "CPU1 HackingLink should route TurnOn through CPU1Router to CPU1Destroy",
    );
    assert.equal(
      (await game.entities.byTemplate(HACKED_SHODAN_COMPUTER)).length,
      hackedBefore + 1,
      "CPU1 should use its authored Corpse result model",
    );

    // Saving between interlocks is essential: TriggerMulti must remember the
    // first consumed router rather than requiring its now-destroyed CPU again.
    const midwaySave = `shodan_interlock_midway_${Date.now()}`;
    assert.equal((await game.save(midwaySave)).success, true);
    assert.equal((await game.load(midwaySave)).success, true);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.entities.byTemplate(CPUS[0])).length,
      0,
      "the first hacked interlock should remain replaced after save/load",
    );

    await playComputerHrm(game, CPUS[1]);
    await game.step({ frames: 20 });
    await backAwayFromComputer(game);
    assert.equal(
      (await game.entities.byTemplate(303)).length,
      0,
      "CPU2 HackingLink should route TurnOn through CPU2Router to CPU2Destroy",
    );
    const shieldsAfterTwo = (
      await Promise.all(SHIELDS.map((template) => game.entities.byTemplate(template)))
    ).reduce((count, shields) => count + shields.length, 0);
    assert.equal(
      shieldsAfterTwo,
      SHIELDS.length,
      "two interlocks must not disable SHODAN's shields early",
    );

    await playComputerHrm(game, CPUS[2]);
    await game.step({ frames: 30 });
    await backAwayFromComputer(game);
    const shieldsAfterThree = (
      await Promise.all(SHIELDS.map((template) => game.entities.byTemplate(template)))
    ).reduce((count, shields) => count + shields.length, 0);
    assert.equal(
      shieldsAfterThree,
      0,
      `the third genuine HRM win should complete HackMultiTrigger and destroy all shields; ` +
        `logs=${game
          .logs()
          .filter(
            (line) =>
              line.includes("turn on") ||
              line.includes("trigger_multi") ||
              line.includes("destroying"),
          )
          .join(" | ")}`,
    );
    assert.equal(
      (await game.entities.byTemplate(HACKED_SHODAN_COMPUTER)).length,
      hackedBefore + CPUS.length,
      "all three computers should persist as their authored hacked models",
    );

    // Leave the CPU3 cabinet behind and use the now-open east side of the
    // shield chamber for an unobstructed production view of the head.
    const headVantageY = (await game.player.position()).y;
    for (const waypoint of [
      { x: 44, y: headVantageY, z: 76 },
      { x: 44, y: headVantageY, z: 72 },
      { x: 40.5, y: headVantageY, z: 72 },
    ]) {
      for (let hop = 0; hop < 3; hop += 1) {
        const position = await game.player.position();
        if (Math.hypot(waypoint.x - position.x, waypoint.z - position.z) < 0.3) {
          break;
        }
        const moved = await game.player.moveTo(waypoint);
        assert.ok(
          moved.moved,
          `post-shield route should advance toward ${JSON.stringify(waypoint)}`,
        );
        if (moved.blocked) {
          break;
        }
      }
    }

    const [headAfter] = await game.entities.byTemplate(SHODAN_HEAD);
    assert.ok(headAfter, "SHODAN's head should remain as the newly exposed finale target");
    const headAim = await game.player.aimAt(headAfter, {
      hitbox: "head",
      visibility: "required",
    });
    assert.ok(
      headAim.visibility.state === "visible",
      "after the interlock chain, production aim should have line of sight to SHODAN's head",
    );
  },
);
