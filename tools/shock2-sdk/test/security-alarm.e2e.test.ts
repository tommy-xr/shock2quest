import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { only, property } from "./helpers/entities.js";
import { activePanel, hasTexture, playHackBoardToWin } from "./helpers/hack.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// `debug_camera` wires the authored security chain a real level has: a
// security camera switch-linked to a security ecology (and back), plus a
// security computer. Stable template ids.
const CAMERA = -367;
const ECOLOGY = -975;
const CONSOLE = -1250;
/// The ecology's authored alert recovery, which is the alarm's duration.
const ALARM_SECONDS = 120;
const BIG_NANITE_PILE = -1591;



async function alarm(game: GameServer) {
  return (await game.ui.state()).security_alarm ?? null;
}

test(
  "a camera that identifies the player raises the station alarm until security stands down",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_camera" });
    await game.step({ frames: 10 });

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

    // Hacking the security computer stands the whole station down well before
    // the deadline: the alarm clears and the ecology is reset out of its alert
    // tier, which in turn clears the camera that raised it. (Frobbing the
    // console only opens its board - the win is what stands security down.)
    const spawn = await game.player.position();
    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    await game.player.spawnItem(BIG_NANITE_PILE);
    const [x, y, z] = (await game.entities.detail(console_.id)).position;
    await game.player.teleport({ x: x - 1.2, y: y + 0.5, z: z - 1.2 });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(console_.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.ok(
      hasTexture(await activePanel(game), "hack.pcx"),
      "the console should offer its hack board",
    );
    await playHackBoardToWin(game, (panel) => hasTexture(panel, "winh.pcx"));
    await game.step({ frames: 10 });
    assert.equal(await alarm(game), null, "the console should stand security down");
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

    // The camera re-arms: seeing the player again raises a fresh alarm - once
    // the hack's camera-blindness window has run out.
    await game.player.teleport(spawn);
    for (
      let attempt = 0;
      attempt < 30 && ((await game.ui.state()).security_cameras_blind_seconds ?? 0) > 0;
      attempt += 1
    ) {
      await game.step({ frames: 5 * 60 });
    }
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
