import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";
import {
  activePanel,
  hasTexture,
  playHackBoardToCriticalFailure,
  playHackBoardToWin,
} from "./helpers/hack.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable template ids. `debug_camera` wires the authored security chain
// (camera -> ecology -> alarm) plus a security console; `debug_turret` has a
// laser turret and a live hostile for a hacked one to shoot at.
const CAMERA = -367;
const CONSOLE = -1250;
const TURRET = -168;
const HOSTILE = -397;
const BIG_NANITE_PILE = -1591;

/// The console's authored `P$HackTime`, in seconds - how long a win hides the
/// player from the cameras.
const BLIND_SECONDS = 120;
/// The board a won hack shows.
const WON_TEXTURE = "winh.pcx";

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

async function blindSeconds(game: GameServer): Promise<number> {
  return (await game.ui.state()).security_cameras_blind_seconds ?? 0;
}

/**
 * Stand next to a hackable object and frob it. Scripts only run near the
 * player, so a frob injected from across the level reaches nothing.
 */
async function frobNearby(game: GameServer, entityId: number): Promise<void> {
  const [x, y, z] = (await game.entities.detail(entityId)).position;
  await game.player.teleport({ x: x - 1.2, y: y + 0.5, z: z - 1.2 });
  await game.step({ frames: 5 });
  await game.entities.sendMessage(entityId, { type: "Frob" });
  await game.step({ frames: 5 });
}

/** Dismiss whatever panel is open, by clicking the bare view beside it. */
async function closePanel(game: GameServer): Promise<void> {
  await game.input.set("pointer.position", [0.9, 0.9]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 5 });
}

/** Max Hack and a full wallet: skill leaves the board with no mines to lose on. */
async function provisionExpertHacker(game: GameServer): Promise<void> {
  await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
  await game.player.spawnItem(BIG_NANITE_PILE);
}

test(
  "hacking the security console stands the alarm down and blinds the cameras until the window lapses",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_camera" });
    await game.step({ frames: 10 });

    const camera = await only(game, CAMERA, "security camera");
    const console_ = await only(game, CONSOLE, "security computer");
    const spawn = await game.player.position();

    // The camera identifies the player on its own and raises the alarm.
    let cameraDetail = await game.entities.detail(camera.id);
    for (
      let attempt = 0;
      attempt < 20 && property(cameraDetail, "AIAlertness") !== "High";
      attempt += 1
    ) {
      await game.step({ frames: 60 });
      cameraDetail = await game.entities.detail(camera.id);
    }
    await game.step({ frames: 10 });
    assert.ok(await alarm(game), "the camera should raise the station alarm");
    assert.equal(await blindSeconds(game), 0, "nothing has blinded the cameras yet");

    // KEY: frobbing the console opens the HRM board rather than standing
    // security down outright - retail gates the console behind a hack.
    await provisionExpertHacker(game);
    await frobNearby(game, console_.id);
    const board = await activePanel(game);
    assert.equal(board.entity_id, console_.id, "the board should be bound to the console");
    assert.ok(hasTexture(board, "hack.pcx"), "frobbing the console should open the HRM board");

    await playHackBoardToWin(game, (panel) => hasTexture(panel, WON_TEXTURE));
    await game.step({ frames: 10 });

    assert.equal(await alarm(game), null, "a won hack should stand security down");
    const blind = await blindSeconds(game);
    assert.ok(
      blind > BLIND_SECONDS - 10 && blind <= BLIND_SECONDS,
      `the win should blind the cameras for the authored ${BLIND_SECONDS}s, got ${blind}`,
    );

    // Back in the camera's cone: it keeps scanning but cannot pick the player
    // out, so no new alarm.
    await game.player.teleport(spawn);
    await game.step({ frames: 10 * 60 });
    assert.equal(await alarm(game), null, "a blinded camera must not raise the alarm");
    assert.notEqual(
      property(await game.entities.detail(camera.id), "AIAlertness"),
      "High",
      "a blinded camera must not identify the player",
    );

    // The window lapses and the camera sees again.
    for (
      let attempt = 0;
      attempt < 30 && (await blindSeconds(game)) > 0;
      attempt += 1
    ) {
      await game.step({ frames: 5 * 60 });
    }
    assert.equal(await blindSeconds(game), 0, "the window should run out");
    for (
      let attempt = 0;
      attempt < 20 && (await alarm(game)) === null;
      attempt += 1
    ) {
      await game.step({ frames: 60 });
    }
    assert.ok(
      await alarm(game),
      "once the window lapses the camera should identify the player again",
    );
  },
);

test(
  "a critically failed console breaks and refuses to open again",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_camera" });
    await game.step({ frames: 10 });

    const console_ = await only(game, CONSOLE, "security computer");
    // Deliberately unskilled - the mines stay on the board. Nanites still
    // cover the repeated attempts.
    await game.player.setStats({ cyber_affinity: 1, skills: { hack: 0 } });
    await game.player.spawnItem(BIG_NANITE_PILE);
    await game.player.spawnItem(BIG_NANITE_PILE);

    await frobNearby(game, console_.id);
    await playHackBoardToCriticalFailure(game);
    await game.step({ frames: 10 });

    assert.equal(
      property(await game.entities.detail(console_.id), "ObjectState"),
      "Broken",
      "losing on a mine should break the console",
    );

    // A broken console refuses normal use: with the ruined board dismissed,
    // frobbing it opens nothing at all.
    await closePanel(game);
    await game.entities.sendMessage(console_.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "a broken console must not offer its board again",
    );
  },
);

test(
  "a hacked turret joins the player's team and opens fire on the level's hostiles",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_turret" });
    await game.step({ frames: 30 });

    const turret = await only(game, TURRET, "laser turret");
    const hostile = await only(game, HOSTILE, "hostile creature");

    const hostileHp = async () =>
      Number(property(await game.entities.detail(hostile.id), "HitPoints") ?? "0");

    // Before the hack the turret is on its authored hostile team and shoots
    // the player, not the creature beside them.
    assert.notEqual(
      property(await game.entities.detail(turret.id), "AITeam"),
      "Good",
      "an authored turret is hostile to the player",
    );
    const playerHpBefore = (await game.info()).player.hit_points ?? 0;
    const hostileHpBefore = await hostileHp();
    await game.step({ frames: 8 * 60 });
    assert.ok(
      ((await game.info()).player.hit_points ?? 0) < playerHpBefore,
      "a hostile turret should be shooting the player",
    );
    assert.equal(
      await hostileHp(),
      hostileHpBefore,
      "a hostile turret should not shoot its own side",
    );

    // Hack it.
    await provisionExpertHacker(game);
    await frobNearby(game, turret.id);
    const board = await activePanel(game);
    assert.ok(hasTexture(board, "hack.pcx"), "frobbing a hostile turret should open the board");
    await playHackBoardToWin(game, (panel) => hasTexture(panel, WON_TEXTURE));
    await game.step({ frames: 10 });

    assert.equal(
      property(await game.entities.detail(turret.id), "AITeam"),
      "Good",
      "a won hack should move the turret onto the player's team",
    );

    // ...and it now fires on what is still hostile.
    const hostileHpAfterHack = await hostileHp();
    await game.step({ frames: 12 * 60 });
    assert.ok(
      (await hostileHp()) < hostileHpAfterHack,
      "a hacked turret should shoot the level's hostiles",
    );

    // A turret already on the player's team has nothing left to sell.
    await closePanel(game);
    await game.entities.sendMessage(turret.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const panel = (await game.ui.state()).active_panel;
    assert.ok(
      panel === null || panel.entity_id !== turret.id,
      "a hacked turret must not offer its board again",
    );
  },
);
