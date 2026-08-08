import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, UiElement, UiPanel } from "../src/types.js";
import { carriedNaniteTotal } from "./helpers/earth-replicator.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

// Real Operations fixture from the detailed 25th-anniversary play-through:
// Security Comp 346 --SwitchLink--> Ecology 341 --SwitchLink--> Camera 350.
// Runtime ids are discovered every launch; the constants are stable authored
// mission-object/template ids. On pre-fix main, the production aim + squeeze
// reaches object 346 at 1.39 units, but SecurityComputer is unimplemented and
// `/v1/ui.active_panel` remains null.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const SECURITY_COMPUTER = 346;
const ECOLOGY = 341;
const CAMERA = 350;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

function button(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(found, `security HRM should expose ${label}`);
  return found;
}

function hasTexture(panel: UiPanel, texture: string): boolean {
  return panel.elements.some(
    (element) => element.texture?.toLowerCase() === texture,
  );
}

async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "production frob should keep the Security Computer MFD open");
  return panel;
}

async function playSecurityHrmToWin(game: GameServer): Promise<UiPanel> {
  const routes = [
    ["node-2-0", "node-3-0", "node-4-0"],
    ["node-2-1", "node-2-2", "node-2-3"],
    ["node-0-1", "node-0-2", "node-0-3"],
    ["node-4-0", "node-4-1", "node-4-2"],
    ["node-0-3", "node-1-3", "node-2-3"],
  ];
  for (let attempt = 0; attempt < 15; attempt += 1) {
    let panel = await activePanel(game);
    if (hasTexture(panel, "winh.pcx")) return panel;
    assert.ok(
      !hasTexture(panel, "loseh.pcx"),
      "max Hack/Cyber should avoid terminal critical failure in this bounded run",
    );
    const inPlay = panel.elements.some((element) => element.label === "reset-hack");
    if (!inPlay || hasTexture(panel, "failh.pcx")) {
      const deal = panel.elements.find(
        (element) =>
          element.label === "start-hack" || element.label === "reset-hack",
      );
      assert.ok(deal, "an unwon board should offer START/RESET");
      await clickUiElement(game, deal);
      assert.ok(
        !hasTexture(await activePanel(game), "payh.pcx"),
        "the provisioned authored nanite stack should cover every retry",
      );
    }
    for (const label of routes[attempt % routes.length]!) {
      panel = await activePanel(game);
      if (hasTexture(panel, "winh.pcx")) return panel;
      if (hasTexture(panel, "failh.pcx") || hasTexture(panel, "loseh.pcx")) break;
      await clickUiElement(game, button(panel, label));
    }
  }
  assert.fail(
    `real Security Computer HRM did not win; rng=${game.logs()
      .filter((line) => line.includes("HRM rng"))
      .slice(-8)
      .join(" | ")}`,
  );
}

test(
  "ops3 Security Computer resets alarm and applies a saved timed device hack",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops3.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8743),
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
      "Normal",
      "normal frob should reset the active global alarm before opening the overlay",
    );

    // Provision only the player prerequisites; success still travels through
    // the genuine HRM UI and authored difficulty/cost.
    await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });
    await game.player.spawnItem(-1591); // authored Big Nanite Pile
    const nanitesBefore = await carriedNaniteTotal(game);
    assert.ok(nanitesBefore >= 45, "wallet should cover all 15 bounded retries");
    const won = await playSecurityHrmToWin(game);
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
    }
    assert.equal(
      property(await game.entities.detail((await game.entities.byTemplate(CAMERA))[0]!.id), "AIAlertness"),
      "Lowest",
      "saved active security hack should keep camera 350 blind before 330 seconds",
    );
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
