import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Command campaign blocker regression for #940. rec1 mission object 74 is the
// main-tram ElevatorButton. This deliberately uses default `--vr`, the real
// trigger ray for both frob and GUI input, and no direct Frob or experimental
// `gui` flag. Before #941, OpenPanel had no default-VR host, so no UI collider
// existed after the first trigger pull.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const REC1_ELEVATOR_BUTTON = 74;
// Authored ops2 Level Start Marker whose PropStartLoc and PropDestLoc are 22.
const OPS2_START_LOC_22 = 560;
const PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;

// ElevatorGui draws deck 5 at row 0 and deck 1 at row 4. Operations is deck 4,
// therefore row 1. Aim at its 142x54 button center.
const OPERATIONS_CENTER_PX: Vec3 = [14 + 142 / 2, 6 + (54 + 4) + 54 / 2, 0];

const uiBodies = async (game: GameServer): Promise<PhysicsBodySummary[]> =>
  (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );

async function assertPanelIsRendered(
  game: GameServer,
  panel: PhysicsBodySummary,
): Promise<void> {
  const response = await fetch(`${game.baseUrl}/v1/scene`);
  assert.equal(response.ok, true, "scene inspection should succeed");
  const scene = (await response.json()) as {
    objects: Array<{ position: Vec3; transparency: number | null }>;
  };
  const visiblePanelDraws = scene.objects.filter(
    (object) =>
      object.transparency !== 1 &&
      Math.hypot(
        object.position[0] - panel.position[0],
        object.position[1] - panel.position[1],
        object.position[2] - panel.position[2],
      ) < 0.01,
  );
  assert.ok(
    visiblePanelDraws.length > 0,
    "the sole UI collider must have visible ElevatorGui draws at its world transform",
  );
}

async function openElevatorThroughVrHand(
  game: GameServer,
): Promise<PhysicsBodySummary> {
  const [button] = await game.entities.byTemplate(REC1_ELEVATOR_BUTTON);
  assert.ok(button, "expected rec1 main elevator button mission object 74");

  await teleportVerified(game, {
    x: button.position[0] - 1.2,
    y: button.position[1] + 0.5,
    z: button.position[2],
  });
  await game.step({ frames: 5 });

  const aim = await game.player.aimAt(button, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(
    aim.target_confirmed,
    true,
    `button 74 must be reachable by the production ray: ${JSON.stringify(aim)}`,
  );
  await aimVrHandAt(game, aim.world_point);

  assert.equal(
    (await uiBodies(game)).length,
    0,
    "no VR panel should exist before frob",
  );
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 5 });

  const panels = await uiBodies(game);
  assert.equal(
    panels.length,
    1,
    "production VR frob of button 74 must create exactly one visible/collidable panel",
  );
  assert.equal(panels[0].body_type, "kinematic");
  await assertPanelIsRendered(game, panels[0]);
  return panels[0];
}

test(
  "default VR rec1 elevator world panel survives save/load cleanup and reaches ops2 loc 22",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8541),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    // Complete power makes Operations (deck 4) a real enabled ElevatorGui
    // button; the test still opens and selects it only through production VR
    // input below.
    await game.quests.set("ElevState", "complete");
    const panelBeforeSave = await openElevatorThroughVrHand(game);

    const saveName = `vr_elevator_panel_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    assert.equal(
      (await uiBodies(game)).length,
      0,
      "loading must close the transient world-panel proxy",
    );
    assert.equal(await game.quests.get("ElevState"), "complete");

    // Loading rebuilds runtime entity IDs. Reopen by stable mission object 74,
    // then invert ProxyGuiScript's pixel-to-world mapping for Operations row.
    const panel = await openElevatorThroughVrHand(game);
    assert.notEqual(
      panel.body_id,
      panelBeforeSave.body_id,
      "the reopened panel should be a newly created transient body",
    );
    const panelSize: Vec3 = [
      PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
      PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
      0,
    ];
    const u = OPERATIONS_CENTER_PX[0] / PANEL_SIZE_PX[0];
    const v = OPERATIONS_CENTER_PX[1] / PANEL_SIZE_PX[1];
    const localOperations: Vec3 = [
      panelSize[0] * (0.5 - u),
      panelSize[1] * (0.5 - v),
      0,
    ];
    const operationsWorld = add(
      panel.position,
      quatRotate(panel.rotation, localOperations),
    );

    const panelAim = await aimVrHandAt(game, operationsWorld, 0.35);
    const panelHit = await game.raycast({
      start: panelAim.start,
      end: panelAim.target,
      collision_groups: ["ui"],
      max_distance: 1,
    });
    assert.equal(
      panelHit.entity_id,
      panel.entity_id,
      "the production hand ray must hit the Operations row",
    );

    assert.equal((await game.info()).mission, "rec1.mis");
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 20 });

    const arrived = await game.info();
    assert.equal(
      arrived.mission,
      "ops2.mis",
      "triggering the enabled Operations row must perform the normal elevator transition",
    );
    assert.equal(
      await game.quests.get("ElevState"),
      "complete",
      "the elevator transition must preserve global campaign state",
    );

    // ElevatorGui requests loc 22. The mission loader places the player at the
    // matching StartLoc; resolve that stable object and verify the landing.
    const [startLoc22] = await game.entities.byTemplate(OPS2_START_LOC_22);
    assert.ok(startLoc22, "ops2 should contain the authored StartLoc 22 marker");
    const distance = Math.hypot(
      arrived.player.position[0] - startLoc22.position[0],
      arrived.player.position[1] - startLoc22.position[1],
      arrived.player.position[2] - startLoc22.position[2],
    );
    assert.ok(
      distance < 2,
      `expected ops2 loc 22 landing, got distance ${distance}`,
    );
  },
);
