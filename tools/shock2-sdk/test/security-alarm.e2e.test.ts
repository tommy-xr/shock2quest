import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// `debug_camera` wires the authored security chain a real level has: a
// security camera switch-linked to a security ecology (and back), plus a
// security computer. Stable template ids.
const CAMERA = -367;
const ECOLOGY = -975;
const CONSOLE = -1250;
/// The ecology's authored alert recovery, which is the alarm's duration.
const ALARM_SECONDS = 120;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

async function only(
  game: GameServer,
  templateId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(templateId);
  assert.equal(matches.length, 1, `${label}: expected one, got ${JSON.stringify(matches)}`);
  return matches[0]!;
}

async function alarm(game: GameServer) {
  return (await game.ui.state()).security_alarm ?? null;
}

test(
  "a camera that identifies the player raises the station alarm until security stands down",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_camera" });
    await game.step({ frames: 10 });
    await game.player.setStats({ skills: { hack: 6 }, cyber_affinity: 6 });
    await game.player.spawnItem("20 Nanites");

    const camera = await only(game, CAMERA, "security camera");
    const ecology = await only(game, ECOLOGY, "security ecology");
    const console_ = await only(game, CONSOLE, "security computer");

    assert.equal(await alarm(game), null, "the station starts calm");
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
    );

    // No injected message: the camera escalates through its own perception of
    // the player standing in its scan cone.
    let cameraDetail = await game.entities.detail(camera.id);
    for (
      let attempt = 0;
      attempt < 20 && property(cameraDetail, "AIAlertness") !== "High";
      attempt += 1
    ) {
      await game.step({ frames: 60 });
      cameraDetail = await game.entities.detail(camera.id);
    }
    assert.equal(
      property(cameraDetail, "AIAlertness"),
      "High",
      "the camera should identify the player on its own",
    );

    // ...which raises the alarm, alerts the ecology, and starts the countdown
    // at the ecology's authored alert recovery.
    await game.step({ frames: 10 });
    const raised = await alarm(game);
    assert.ok(raised, "identifying the player should raise the station alarm");
    assert.equal(raised.count, 1);
    assert.ok(
      raised.seconds_remaining > ALARM_SECONDS - 10 &&
        raised.seconds_remaining <= ALARM_SECONDS,
      `countdown should start at the authored ${ALARM_SECONDS}s, got ${raised.seconds_remaining}`,
    );
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Alert",
      "an alarm should put the ecology in its alert tier",
    );

    // The countdown runs down in real simulation time.
    await game.step({ frames: 5 * 60 });
    const ticked = await alarm(game);
    assert.ok(ticked, "the alarm should still be up");
    assert.ok(
      ticked.seconds_remaining < raised.seconds_remaining - 4,
      `countdown should be running, ${raised.seconds_remaining} -> ${ticked.seconds_remaining}`,
    );

    const scanPosition = (await game.info()).player.position;
    await game.player.teleport({ x: console_.position[0], y: console_.position[1], z: console_.position[2] - 2 });
    // Opening the console must not bypass its paid hacking interaction.
    await game.entities.sendMessage(console_.id, { type: "Frob" });
    await game.step({ frames: 10 });
    assert.ok(await alarm(game), "merely opening security must not clear an alarm");
    const unpaid = (await game.ui.state()).active_panel;
    assert.ok(unpaid, "security computer should open the existing HRM panel");
    const start = unpaid.elements.find((e) => e.label === "start-hack");
    assert.ok(start);
    const nanitesBefore = await carriedNaniteTotal(game);
    await clickUiElement(game, start);
    assert.ok(await carriedNaniteTotal(game) < nanitesBefore, "HRM must charge nanites");
    // Exercise real node clicks. High provisioned skill removes mines; misses
    // can block a node, so explore remaining rows rather than inject success.
    for (let attempt = 0; attempt < 5 && await alarm(game); attempt++) {
      if (attempt > 0) {
        const panel = (await game.ui.state()).active_panel;
        const reset = panel?.elements.find((e) => e.label === "reset-hack");
        assert.ok(reset, "failed HRM should offer reset");
        await clickUiElement(game, reset);
        const startAgain = (await game.ui.state()).active_panel?.elements.find((e) => e.label === "start-hack");
        if (startAgain) await clickUiElement(game, startAgain);
      }
      for (let y = 0; y < 4 && await alarm(game); y++) {
        for (let x = 0; x < 5 && await alarm(game); x++) {
          const panel = (await game.ui.state()).active_panel;
          const node = panel?.elements.find((e) => e.label === `node-${x}-${y}`);
          if (node) await clickUiElement(game, node);
        }
      }
    }
    await game.step({ frames: 2 });
    assert.equal(await alarm(game), null, "a successful hack should stand security down");
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
      "the stand-down should reset the alerted ecology",
    );
    assert.notEqual(
      property(await game.entities.detail(camera.id), "AIAlertness"),
      "High",
      "the ecology's reset should clear the camera that alarmed",
    );

    const close = (await game.ui.state()).active_panel?.elements.find((e) => e.label === "close");
    if (close) await clickUiElement(game, close);
    await game.player.teleport({ x: scanPosition[0], y: scanPosition[1], z: scanPosition[2] });
    // The camera re-arms: seeing the player again raises a fresh alarm.
    for (
      let attempt = 0;
      attempt < 20 && (await alarm(game)) === null;
      attempt += 1
    ) {
      await game.step({ frames: 60 });
    }
    const rearmed = await alarm(game);
    assert.ok(rearmed, "a re-armed camera should raise the alarm again");
    assert.equal(rearmed.count, 1, "the count must not have leaked from the first alarm");
  },
);
