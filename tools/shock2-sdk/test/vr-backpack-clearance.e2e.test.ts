import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Production-VR regression for #1095. The stance and +90-degree head yaw are
// the independently reproduced Hydro chemical-storage case: fixed 2 m
// placement puts the backpack behind the wall at x=61.2. Setup provisions one
// real item into slot zero, but opening and retrieval use the same Quest-X,
// world-panel collider, controller ray, and squeeze path as the headset.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const HYDRO_WALL_X = 61.2;
const HYDRO_STANCE = [59.377445, -1.956119, -41.344913] as const;
const BACKPACK_CANVAS_PX: Vec3 = [635, 120, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;
const VR_BACKPACK_SCALE = 0.55;

function uiPanel(bodies: PhysicsBodySummary[]): PhysicsBodySummary {
  const panels = bodies.filter((body) => body.collision_groups.includes("ui"));
  assert.equal(panels.length, 1, "MoveInventory must expose one physical VR panel");
  return panels[0];
}

function horizontalDistance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

test(
  "VR backpack clears the Hydro wall, stays 2 m in open space, and remains clickable",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8595),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    const toxin = await game.player.spawnItem(-1341);
    await teleportVerified(game, {
      x: HYDRO_STANCE[0],
      y: HYDRO_STANCE[1],
      z: HYDRO_STANCE[2],
    });

    // Control: looking into the open aisle keeps the authored 2 m distance.
    let player = (await game.info()).player;
    await game.input.lookAtWorldPoint(
      add(player.position, [0, player.camera_offset[1], 10]),
      { eyeHeight: player.camera_offset[1] },
    );
    await game.step({ frames: 2 });
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 5 });
    player = (await game.info()).player;
    let panel = uiPanel((await game.physics.bodies()).bodies);
    assert.ok(
      Math.abs(horizontalDistance(panel.position, player.position) - 2.0) < 0.01,
      `open-space backpack distance must remain 2 m: player=${player.position}, panel=${panel.position}`,
    );
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 3 });

    // Reopen toward +X at the exact failing wall. The complete panel footprint
    // must stop on the viewer side, not just its center ray.
    player = (await game.info()).player;
    await game.input.lookAtWorldPoint(
      add(player.position, [10, player.camera_offset[1], 0]),
      { eyeHeight: player.camera_offset[1] },
    );
    await game.step({ frames: 2 });
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 5 });
    player = (await game.info()).player;
    panel = uiPanel((await game.physics.bodies()).bodies);
    const clampedDistance = horizontalDistance(panel.position, player.position);
    assert.ok(
      clampedDistance < 1.9,
      `nearby geometry must clamp the fixed 2 m placement, got ${clampedDistance}`,
    );
    assert.ok(
      panel.position[0] <= HYDRO_WALL_X - 0.09,
      `backpack center must retain viewer-side wall clearance: ${panel.position[0]}`,
    );

    const ui = (await game.ui.state()).active_panel;
    assert.ok(ui, "the clamped backpack remains the active production panel");
    const item = ui.elements.find((element) => element.entity_id === toxin.entity_id);
    assert.ok(item, `the provisioned toxin must render in the backpack: ${JSON.stringify(ui)}`);
    const [x, y, width, height] = item.rect;
    const panelSize: Vec3 = [
      BACKPACK_CANVAS_PX[0] * GUI_PIXEL_TO_WORLD_SIZE * VR_BACKPACK_SCALE,
      BACKPACK_CANVAS_PX[1] * GUI_PIXEL_TO_WORLD_SIZE * VR_BACKPACK_SCALE,
      0,
    ];
    const localPoint: Vec3 = [
      panelSize[0] * (0.5 - (x + width / 2) / BACKPACK_CANVAS_PX[0]),
      panelSize[1] * (0.5 - (y + height / 2) / BACKPACK_CANVAS_PX[1]),
      0,
    ];
    const itemWorld = add(panel.position, quatRotate(panel.rotation, localPoint));
    const aim = await aimVrHandAt(game, itemWorld, 0.35);
    const firstHit = await game.raycast({
      start: aim.start,
      end: aim.target,
      collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
      max_distance: 1,
    });
    assert.equal(
      firstHit.entity_id,
      panel.entity_id,
      `controller ray must reach the backpack before the wall: ${JSON.stringify(firstHit)}`,
    );

    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.player.inventory()).items.find(
        (candidate) => candidate.entity_id === toxin.entity_id,
      )?.location,
      "right_hand",
      "production squeeze must retrieve the exact visible item",
    );
  },
);
