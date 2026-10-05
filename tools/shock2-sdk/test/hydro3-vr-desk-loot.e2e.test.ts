import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// A wide desk must not occlude its loot. The cyber-interface MFD owns the
// complete container canvas and its input, including lower rows.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const HYDRO3_DESK_2 = 2116;
const HYDRO3_AUDIO_LOG = 200;

test(
  "Hydro3 Desk #2 draws log 200 over its collider and lets the production VR hand collect it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro3.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const [desk] = await game.entities.byTemplate(HYDRO3_DESK_2);
    assert.ok(desk, "hydro3 must contain Desk #2 mission object 2116");
    const contains = (await game.entities.detail(desk.id)).outgoing_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    const logLink = contains.find((link) => link.target_name === "Audio Log");
    assert.ok(logLink, "Desk #2 must contain its Audio Log");
    assert.equal(
      (await game.entities.detail(logLink.target_id)).template_id,
      HYDRO3_AUDIO_LOG,
      "the contained disc must be mission object 200",
    );

    await teleportVerified(game, { x: 29.2, y: 1.644, z: -3.2 });

    // Before the panel opens, no world-panel quads are on the render-over
    // layer (only the ordinary world scene is up).
    const sceneUiBeforeOpen = (await game.scene.objects()).objects.filter(
      (object) => object.render_layer === "scene_ui",
    );
    assert.equal(
      sceneUiBeforeOpen.length,
      0,
      "no scene_ui-layer geometry should exist before any panel is open",
    );

    const deskAim = await game.player.aimAt(desk, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(deskAim.target_confirmed, true, "Desk #2 must be hand-reachable");
    await aimVrHandAt(game, deskAim.world_point);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    const panelState = (await game.ui.state()).active_panel;
    assert.equal(panelState?.template_id, HYDRO3_DESK_2, "the real desk frob must open its panel");
    const logButton = panelState.elements.find(
      (element) => element.kind === "button" && element.entity_id === logLink.target_id,
    );
    assert.ok(logButton, "the third loot row must render Audio Log 200");

    const ui = await game.ui.state();
    assert.ok(ui.panel_pose);
    assert.equal(ui.mode, "use");
    const panelBodies = (await game.physics.bodies()).bodies.filter(body => body.collision_groups.includes("ui"));
    assert.equal(panelBodies.length, 0, "loot must not create a third world panel");
    const [x, y, w, h] = logButton.rect;
    await aimVrHandAtCanvas(game, ui.panel_pose, [x + w / 2, y + h / 2]);

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    assert.ok(
      (await game.info()).player.collected_logs.some(
        (log) => log.deck === 3 && log.log === 16,
      ),
      "clicking the visible row must collect deck-3/log-16",
    );
    assert.ok(
      !(await game.ui.state()).active_panel?.elements.some(
        (element) => element.entity_id === logLink.target_id,
      ),
      "the collected log must leave the desk panel",
    );
  },
);
