import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, UiPanel } from "../src/types.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { teleportVerified } from "./helpers/teleport.js";
import { hasHackTexture as hasTexture, winHack } from "./helpers/hack.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { aimMfdAt, drawPersonalCard } from "./helpers/vr-hand.js";

// Real Operations fixture from the detailed 25th-anniversary play-through:
// Security Comp 346 --SwitchLink--> Ecology 341 --SwitchLink--> Camera 350.
// Runtime ids are discovered every launch; the constants are stable authored
// mission-object/template ids. Opening resets the alarm without a payment or
// suppression window; a paid hack adds suppression that persists through saves.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const SECURITY_COMPUTER = 346;
const ECOLOGY = 341;
const CAMERA = 350;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "production frob should keep the Security Computer MFD open");
  return panel;
}


for (const vr of [false, true]) test(
  `ops3 Security Computer clears alarms on opening (${vr ? "VR scan" : "flat frob and saved timed hack"})`,
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops3.mis",
      port: 0,
      debugFlags: vr ? ["--vr"] : [],
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    if (vr) await game.devParams.set("vr_mfd_focus_scan", 0);
    await game.step({ frames: 10 });

    const [computer] = await game.entities.byTemplate(SECURITY_COMPUTER);
    const [ecology] = await game.entities.byTemplate(ECOLOGY);
    const [camera] = await game.entities.byTemplate(CAMERA);
    assert.ok(computer && ecology && camera, "ops3 authored security circuit must exist");
    const computerDetail = await game.entities.detail(computer.id);
    assert.ok(
      computerDetail.outgoing_links.some(
        (link) => link.link_type === "SwitchLink" && link.target_id === ecology.id,
      ),
      "Security Comp 346 must exercise its authored ecology SwitchLink",
    );
    // First stand in camera 350's open scan area so ordinary perception raises
    // its real ecology alarm. The console itself occludes the camera's ray to
    // the finding's closer frob point, so alarm setup and production frob use
    // two nearby, independently verified clear positions.
    await teleportVerified(game, { x: 54, y: -8.356, z: 159.75 });
    for (let poll = 0; poll < 20; poll += 1) {
      const cameraDetail = await game.entities.detail(camera.id);
      const ecologyDetail = await game.entities.detail(ecology.id);
      if (
        property(cameraDetail, "AIAlertness") === "High" &&
        property(ecologyDetail, "EcologyState") === "Alert"
      ) {
        break;
      }
      await game.step({ frames: 120 });
    }
    assert.equal(property(await game.entities.detail(camera.id), "AIAlertness"), "High");
    assert.equal(property(await game.entities.detail(ecology.id), "EcologyState"), "Alert");

    // Both presentation paths reach the ordinary SecurityComputer frob:
    // flat aim/squeeze, or the held tricorder's production scan in VR.
    const openComputer = async () => {
      await teleportVerified(game, { x: 57, y: -8.356, z: 161 });
      const aim = await game.player.aimAt(computer, {
        hitbox: "center", visibility: "required",
      });
      assert.equal(aim.entity_id, computer.id);
      assert.equal(aim.visibility.state, "visible");
      if (vr) {
        await drawPersonalCard(game, "left");
        await aimMfdAt(game, aim.world_point, .55, 1, 0, { hand: "left" });
        await game.input.set("left_hand.trigger", 1);
        await game.step({ frames: 12 });
        await game.input.set("left_hand.trigger", 0);
      } else {
        await game.input.set("right_hand.squeeze_value", 1);
        await game.step({ frames: 2 });
        await game.input.set("right_hand.squeeze_value", 0);
      }
      await game.step({ frames: 4 });
    };
    const walletBeforeOpen = await carriedNaniteTotal(game);
    await openComputer();

    const board = await activePanel(game);
    assert.equal(board.template_id, SECURITY_COMPUTER);
    assert.ok(hasTexture(board, "alarmfd.pcx"), "Security Computer should open its retail station panel first");
    assert.match(board.elements.map((element) => element.text ?? "").join(" "), /Security system active/);
    assert.ok(!board.elements.some((element) => element.label === "start-hack"),
      "the station must not expose paid HRM controls until Hack is selected");
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
      "opening security must clear the alarm before a paid hack",
    );

    assert.equal((await game.ui.state()).security_alarm ?? null, null);
    assert.equal(await carriedNaniteTotal(game), walletBeforeOpen, "alarm reset is free");

    const openHack = async () => {
      const ui = await game.ui.state();
      const hack = ui.active_panel?.elements.find((element) => element.label === "hack-security");
      assert.ok(hack, "station must offer the retail Hack plug");
      const balance = await carriedNaniteTotal(game);
      if (vr) await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(hack));
      else await clickUiElement(game, hack);
      const hrm = await activePanel(game);
      assert.ok(hasTexture(hrm, "hack.pcx"), "Hack opens the existing HRM board");
      assert.ok(hrm.elements.some((element) => element.label === "start-hack"));
      assert.equal(await carriedNaniteTotal(game), balance, "opening Hack must not charge before START");
    };
    await openHack();

    // Opening is a reset, not a timed hack: an unhacked camera can immediately
    // identify the player again once they return to its clear scan cone.
    await teleportVerified(game, { x: 54, y: -8.356, z: 159.75 });
    for (let poll = 0; poll < 20; poll += 1) {
      if (property(await game.entities.detail(ecology.id), "EcologyState") === "Alert") break;
      await game.step({ frames: 120 });
    }
    assert.equal(property(await game.entities.detail(camera.id), "AIAlertness"), "High");
    assert.equal(property(await game.entities.detail(ecology.id), "EcologyState"), "Alert",
      "opening the board must not grant timed security suppression");
    if (vr) return; // The unchanged paid timer/save path is exercised below in flat mode.
    await openComputer();

    // Provision only the player prerequisites; success still travels through
    // the genuine HRM UI and authored difficulty/cost.
    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    await game.player.spawnItem(-1591); // authored Big Nanite Pile
    const nanitesBefore = await carriedNaniteTotal(game);
    assert.ok(nanitesBefore >= 45, "wallet should cover all eight bounded retries");
    await openHack();
    const won = await winHack(game);
    assert.ok(hasTexture(won, "winh.pcx"), "genuine HRM success art should remain visible");
    assert.ok(
      (await carriedNaniteTotal(game)) < nanitesBefore,
      "real HRM attempts should spend the authored three-nanite cost",
    );

    await openComputer();
    const disabledStation = await activePanel(game);
    assert.ok(hasTexture(disabledStation, "alarmfd.pcx"), "reopening returns to station status");
    assert.match(disabledStation.elements.map((element) => element.text ?? "").join(" "),
      /Cameras deactivated/, "station status must reflect the active timed hack");

    // Cyber 6 scales ops3's 55,000ms HackTime to 330 seconds. Save immediately
    // after success; after reload the camera must still be blind at 300s, then
    // reacquire once the remaining window expires.
    await teleportVerified(game, { x: 54, y: -8.356, z: 159.75 });
    const saveName = `ops3_security_hack_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    for (let interval = 0; interval < 30; interval += 1) {
      await game.step({ frames: 10 * 60 });
      const [restoredCamera] = await game.entities.byTemplate(CAMERA);
      assert.ok(restoredCamera, "the saved security circuit must remain in the mission");
      assert.equal(
        property(await game.entities.detail(restoredCamera.id), "AIAlertness"),
        "Lowest",
        "saved active security hack should keep camera 350 blind before 330 seconds",
      );
    }
    for (let interval = 0; interval < 4; interval += 1) {
      await game.step({ frames: 10 * 60 });
    }
    const reloadedCamera = (await game.entities.byTemplate(CAMERA))[0]!;
    const reloadedEcology = (await game.entities.byTemplate(ECOLOGY))[0]!;
    for (let poll = 0; poll < 20; poll += 1) {
      if (property(await game.entities.detail(reloadedCamera.id), "AIAlertness") === "High") break;
      await game.step({ frames: 120 });
    }
    assert.equal(
      property(await game.entities.detail(reloadedCamera.id), "AIAlertness"),
      "High",
      "camera 350 should detect the still-present player after the timed hack expires",
    );
    assert.equal(
      property(await game.entities.detail(reloadedEcology.id), "EcologyState"),
      "Alert",
      "restored camera detection should raise the authored ecology again",
    );
  },
);
