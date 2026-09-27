import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, UiPanel } from "../src/types.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { teleportVerified } from "./helpers/teleport.js";
import { hasHackTexture as hasTexture, winHack } from "./helpers/hack.js";

// Real Operations fixture from the detailed 25th-anniversary play-through:
// Security Comp 346 --SwitchLink--> Ecology 341 --SwitchLink--> Camera 350.
// Runtime ids are discovered every launch; the constants are stable authored
// mission-object/template ids. Current main already implements paid hacking;
// this regression covers suppression persisting after a successful hack.
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


test(
  "ops3 Security Computer resets alarm and applies a saved timed device hack",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops3.mis",
      port: 0,
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
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

    // Production interaction: visibility-required aim at the rendered console,
    // then the same squeeze channel used by ordinary flat play. No Frob message
    // is injected.
    await teleportVerified(game, { x: 57, y: -8.356, z: 161 });
    const aim = await game.player.aimAt(computer, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.entity_id, computer.id);
    assert.equal(aim.visibility.state, "visible");
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 4 });

    const board = await activePanel(game);
    assert.equal(board.template_id, SECURITY_COMPUTER);
    assert.ok(hasTexture(board, "hack.pcx"), "Security Computer should open the shared HRM board");
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Alert",
      "merely opening the paid HRM must leave the alarm active",
    );

    // Provision only the player prerequisites; success still travels through
    // the genuine HRM UI and authored difficulty/cost.
    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    await game.player.spawnItem(-1591); // authored Big Nanite Pile
    const nanitesBefore = await carriedNaniteTotal(game);
    assert.ok(nanitesBefore >= 45, "wallet should cover all eight bounded retries");
    const won = await winHack(game);
    assert.ok(hasTexture(won, "winh.pcx"), "genuine HRM success art should remain visible");
    assert.ok(
      (await carriedNaniteTotal(game)) < nanitesBefore,
      "real HRM attempts should spend the authored three-nanite cost",
    );

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
