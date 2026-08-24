import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Production-VR regression for #1100. Hydro3 mission Desk #2 (2116) has an
// authored desk_sd OBB and Contains the deck-3/log-16 Audio Log (200) in its
// third row. Before the fix the panel plane rendered inside the desk mesh:
// that row's y=1.671..1.807 span is below the desk top at y=1.809, so the
// desk's own geometry buried the panel and it z-fought/occluded rather than
// showing (clickable via #1115's host-bypass hit-testing, but not visible).
//
// #1115 already fixed hit-testing (a panel bypasses its own host collider),
// so this no longer asserts on a raw nearest-hit raycast against world
// geometry - that guards physics, not what the player sees. Instead it
// asserts the player-observable outcomes: the buried row is still clickable
// end-to-end (aim + trigger collects it), and the panel's rendered quads
// report the render-over-everything overlay layer rather than the ordinary
// world layer that would let the desk occlude them (see `GuiManager` in
// `shock2vr/src/gui/gui_manager.rs`).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const HYDRO3_DESK_2 = 2116;
const HYDRO3_AUDIO_LOG = 200;
const PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;

test(
  "Hydro3 Desk #2 draws log 200 over its collider and lets the production VR hand collect it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro3.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8600),
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

    // Visual claim: the open panel's quads must be on the depth-bypass
    // overlay layer (`scene_ui`), the same "draw over the world" layer the
    // per-eye HUD/viewmodel use - not the ordinary `world` layer a wide host
    // like Desk #2 can depth-test in front of and bury. Every panel element
    // (background, row art, text, cursor) becomes at least one opaque quad,
    // so opening the panel must add at least that many scene_ui objects, and
    // none of them may be the see-through-the-world default (depth_write
    // false would mean the panel could show the desk drawn through it).
    const sceneUiAfterOpen = (await game.scene.objects()).objects.filter(
      (object) => object.render_layer === "scene_ui",
    );
    assert.ok(
      sceneUiAfterOpen.length >= panelState.elements.length,
      `opening Desk #2's panel must add at least ${panelState.elements.length} scene_ui objects, got ${sceneUiAfterOpen.length}`,
    );
    assert.ok(
      sceneUiAfterOpen.every((object) => object.depth_write),
      "every panel quad must write depth so it still self-occludes correctly, just not against the world",
    );

    const panelBodies = (await game.physics.bodies()).bodies.filter((body) =>
      body.collision_groups.includes("ui"),
    );
    assert.equal(panelBodies.length, 1, "exactly one world panel should be open");
    const panel = panelBodies[0];
    const [x, y, width, height] = logButton.rect;
    const u = (x + width / 2) / PANEL_SIZE_PX[0];
    const v = (y + height / 2) / PANEL_SIZE_PX[1];
    const panelSize: Vec3 = [
      PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
      PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
      0,
    ];
    const localLog: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
    const logWorld = add(panel.position, quatRotate(panel.rotation, localLog));
    await aimVrHandAt(game, logWorld, 0.35);

    // The buried row is still clickable end-to-end through the production
    // hand path (#1115's host-bypass hit-testing) - the point of this
    // regression is that it is now also visible, asserted above.
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
