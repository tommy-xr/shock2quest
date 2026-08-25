import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, PhysicsBodySummary, Vec3 } from "../src/types.js";
import {
  carriedNaniteTotal,
} from "./helpers/earth-replicator.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Production-VR regression for #962. The test opens and hacks Earth Technical
// Training's authored replicator through its physical panel, then holds one
// trigger for two fixed frames while both hands hover the same hacked-catalog
// row. Before the fix, the idle hand re-armed the shared press state between
// right-hand messages: the held gesture emitted repeated vends instead of one.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_NANITES = 257;
const EARTH_REPLICATOR = 262;
const HE_CLIP_NAME = "*HE Clip*";
const PANEL_HEIGHT_PX = 296;
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;

type Hand = "left" | "right";

interface DebugInputHand {
  position: Vec3;
  rotation: [number, number, number, number];
}

async function uiPanel(game: GameServer): Promise<PhysicsBodySummary> {
  const panels = (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );
  assert.equal(panels.length, 1, "expected one active production VR panel");
  return panels[0];
}

function panelPoint(
  panel: PhysicsBodySummary,
  panelWidthPx: number,
  pointPx: [number, number],
): Vec3 {
  const worldSize: Vec3 = [
    panelWidthPx * GUI_PIXEL_TO_WORLD_SIZE,
    PANEL_HEIGHT_PX * GUI_PIXEL_TO_WORLD_SIZE,
    0,
  ];
  const [x, y] = pointPx;
  const local: Vec3 = [
    worldSize[0] * (0.5 - x / panelWidthPx),
    worldSize[1] * (0.5 - y / PANEL_HEIGHT_PX),
    0,
  ];
  return add(panel.position, quatRotate(panel.rotation, local));
}

async function aimHandAtPanelPoint(
  game: GameServer,
  panelWidthPx: number,
  pointPx: [number, number],
  hand: Hand,
): Promise<void> {
  const panel = await uiPanel(game);
  const target = panelPoint(panel, panelWidthPx, pointPx);
  const aim = await aimVrHandAt(game, target, 0.35);
  if (hand === "left") {
    // Reuse the production right-hand aiming calculation, then copy that
    // ordinary controller pose onto the left input channels. The later right
    // aim gives the two hands distinct points inside the same authored row.
    const inputResponse = await fetch(`${game.baseUrl}/v1/control/input`);
    assert.equal(inputResponse.ok, true, "live controller state should be inspectable");
    const input = (await inputResponse.json()) as { right_hand: DebugInputHand };
    await game.input.set("left_hand.position", input.right_hand.position);
    await game.input.set("left_hand.rotation", input.right_hand.rotation);
    await game.input.set("left_hand.trigger", 0);
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 3 });
  }
  const hit = await game.raycast({
    start: aim.start,
    end: aim.target,
    collision_groups: ["ui"],
    max_distance: 1,
  });
  assert.equal(
    hit.entity_id,
    panel.entity_id,
    `${hand} production hand ray must hit panel point ${JSON.stringify(pointPx)}; ${JSON.stringify(hit)}`,
  );
}

async function clickPanelPoint(
  game: GameServer,
  panelWidthPx: number,
  pointPx: [number, number],
): Promise<void> {
  await aimHandAtPanelPoint(game, panelWidthPx, pointPx, "right");
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
}

async function vrFrobEntity(game: GameServer, entity: EntitySummary): Promise<void> {
  const target = (await game.entities.detail(entity.id)).position;
  await teleportVerified(game, {
    x: target[0] + 0.5,
    y: target[1] + 1,
    z: target[2],
  });
  const aim = await aimVrHandAt(game, target, 0.3);
  const hit = await game.raycast({
    start: aim.start,
    end: aim.target,
    collision_groups: ["entity", "selectable", "world", "raycast"],
    max_distance: 1,
    ignore_sensors: true,
  });
  assert.equal(hit.entity_id, entity.id, "production hand ray must hit frob target");
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
}

async function openReplicator(
  game: GameServer,
  replicator: EntitySummary,
): Promise<void> {
  const [x, _y, z] = (await game.entities.detail(replicator.id)).position;
  // The authored booth has one clear, collision-supported standing point.
  // Keep the player there while the real HRM board advances over many frames
  // so walk-away cleanup cannot correctly close a panel under a falling pawn.
  await teleportVerified(game, {
    x: x - 1.59,
    y: 21.404,
    z: z - 2.23,
  });
  const aim = await game.player.aimAt(replicator, {
    hitbox: "center",
    visibility: "required",
  });
  await aimVrHandAt(game, aim.world_point, 0.35);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 5 });
  await uiPanel(game);
}

async function hackReplicator(
  game: GameServer,
  replicator: EntitySummary,
): Promise<void> {
  // Max Navy-tech aptitude removes critical mines while the authored HRM
  // board, payment, node rolls, and success effect remain the real game path.
  await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });

  // PLUGHACK sidecar center, then START on the ordinary 188x296 HRM board.
  await clickPanelPoint(game, 261, [42, 247]);
  await clickPanelPoint(game, 188, [167, 260]);

  // Exercise every board coordinate through the production panel. Empty cells
  // and clicks after the board is won are authored no-ops. At 85% node success
  // and zero mines, this fixed replay reaches connected-three; the reopened
  // hacked catalog and its HE purchase below prove the persistent outcome.
  for (let y = 0; y < 4; y++) {
    for (let x = 0; x < 5; x++) {
      await clickPanelPoint(game, 188, [16 + x * 30 + 8, 48 + y * 36 + 8]);
    }
  }

  // Walking away closes the transient board. Reopening the authored entity
  // resets the won board to its hacked inventory, as normal gameplay does.
  await teleportVerified(game, {
    x: replicator.position[0] + 8,
    y: replicator.position[1] + 1,
    z: replicator.position[2],
  });
  await game.step({ frames: 2 });
  assert.equal(
    (await game.physics.bodies()).bodies.filter((body) =>
      body.collision_groups.includes("ui"),
    ).length,
    0,
    "walking away must close the won board",
  );
  await openReplicator(game, replicator);
}

test(
  "Earth VR world panel arbitrates one held trigger independently per hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const [nanites] = await game.entities.byTemplate(EARTH_NANITES);
    const [replicator] = await game.entities.byTemplate(EARTH_REPLICATOR);
    assert.ok(nanites, "Earth should contain authored nanite pile 257");
    assert.ok(replicator, "Earth should contain authored RepBase 262");

    await vrFrobEntity(game, nanites);
    assert.equal(await carriedNaniteTotal(game), 250);
    await openReplicator(game, replicator);
    await hackReplicator(game, replicator);
    assert.equal(
      await carriedNaniteTotal(game),
      247,
      "authored HRM start should charge three nanites",
    );

    const beforeIds = new Set(
      (await game.entities.list({ filter: HE_CLIP_NAME, limit: 100 })).entities.map(
        (entity) => entity.id,
      ),
    );

    // Small HE Clip is hacked inventory row 0 (188x60 at y=10). Aim the two
    // production hands at separate points inside that same button so both
    // emit hover samples, then hold only the right trigger for two frames.
    await aimHandAtPanelPoint(game, 188, [55, 40], "left");
    await aimHandAtPanelPoint(game, 188, [133, 40], "right");
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    assert.equal(
      await carriedNaniteTotal(game),
      177,
      "one held physical trigger must charge exactly one 70-nanite vend",
    );
    const firstVendIds = (
      await game.entities.list({ filter: HE_CLIP_NAME, limit: 100 })
    ).entities.filter((entity) => !beforeIds.has(entity.id));
    assert.equal(firstVendIds.length, 1, "two-hand hover must dispense exactly one HE clip");

    // One-hand control: move the idle hand away, release/press again, and
    // verify the ordinary authored row still produces exactly one new vend.
    await game.input.set("left_hand.position", [0, -10, 0]);
    await game.step({ frames: 2 });
    await aimHandAtPanelPoint(game, 188, [94, 40], "right");
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    assert.equal(await carriedNaniteTotal(game), 107);
    const allVendIds = (
      await game.entities.list({ filter: HE_CLIP_NAME, limit: 100 })
    ).entities.filter((entity) => !beforeIds.has(entity.id));
    assert.equal(allVendIds.length, 2, "one-hand control must add one more HE clip");
  },
);
