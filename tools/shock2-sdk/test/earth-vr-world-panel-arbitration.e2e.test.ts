import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";
import {
  carriedNaniteTotal,
} from "./helpers/earth-replicator.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt, aimVrHandAtCanvas, drawPersonalCard } from "./helpers/vr-hand.js";
import { canvasCenter, clickCanvasWithRay, requirePanelPose } from "./helpers/ui.js";
import { installEarthCyberInterface } from "./helpers/cyber-interface.js";

// Production-VR regression for #962. The test opens and hacks Earth Technical
// Training's authored replicator through the cyber interface, then holds one
// trigger for two fixed frames while both hands hover the same hacked-catalog
// row. Before the fix, the idle hand re-armed the shared press state between
// right-hand messages: the held gesture emitted repeated vends instead of one.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_NANITES = 257;
const EARTH_REPLICATOR = 262;
const HE_CLIP_NAME = "*HE Clip*";
type Hand = "left" | "right";

async function aimHandAtPanelPoint(
  game: GameServer,
  pointPx: [number, number],
  hand: Hand,
): Promise<void> {
  const ui = await game.ui.state();
  assert.ok(ui.active_panel, "the machine must remain open");
  // Machine-local pixels occupy the shared left MFD slot at (2, 124).
  const canvas: [number, number] = [pointPx[0] + 2, pointPx[1] + 124];
  await aimVrHandAtCanvas(game, requirePanelPose(ui), canvas, { hand });
  await game.step({ frames: 3 });
}

async function clickPanelPoint(
  game: GameServer,
  pointPx: [number, number],
): Promise<void> {
  await aimHandAtPanelPoint(game, pointPx, "right");
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
}

async function collectNanites(game: GameServer, entity: EntitySummary): Promise<void> {
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
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.right_hand_entity_id, entity.id,
    "the physical grip must hold the nanite pile before release collects it");
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 2 });
}

async function openReplicator(
  game: GameServer,
  replicator: EntitySummary,
): Promise<void> {
  const ui = await game.ui.state();
  if (ui.mode === "use") {
    const exit = ui.readout.find(element => element.label === "close_use_mode");
    assert.ok(exit);
    await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(exit));
  }
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
  // Replicators now require an actual personal-card scan in VR.
  await drawPersonalCard(game);
  await aimVrHandAt(game, aim.world_point, 0.02, 1);
  await game.step({ frames: 5 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
  const opened = await game.ui.state();
  assert.equal(opened.mode, "use");
  assert.equal(opened.active_panel?.entity_id, replicator.id);
  requirePanelPose(opened);
}

async function hackReplicator(
  game: GameServer,
  replicator: EntitySummary,
): Promise<void> {
  // Max Navy-tech aptitude removes critical mines while the authored HRM
  // board, payment, node rolls, and success effect remain the real game path.
  await game.player.setStats({ cyber_affinity: 6, skills: { hack: 6 } });

  // PLUGHACK button center, right of the MFD, then START on the HRM board.
  // The canvas keeps the plug's room (252px) for the whole session.
  await clickPanelPoint(game, [221, 247]);
  await clickPanelPoint(game, [167, 260]);

  // Exercise every board coordinate through the production panel. Empty cells
  // and clicks after the board is won are authored no-ops. At 85% node success
  // and zero mines, this fixed replay reaches connected-three; the reopened
  // hacked catalog and its HE purchase below prove the persistent outcome.
  for (let y = 0; y < 4; y++) {
    for (let x = 0; x < 5; x++) {
      await clickPanelPoint(game, [16 + x * 30 + 8, 48 + y * 36 + 8]);
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
    (await game.ui.state()).active_panel,
    null,
    "walking away must close the won board",
  );
  await openReplicator(game, replicator);
}

test(
  "Earth VR replicator interface arbitrates one held trigger independently per hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      debugFlags: ["--vr"],
    });
    // Exercise machine frobs without a held MFD taking ownership of the panel.
    await game.devParams.set("vr_mfd_device", 0);
    await game.step({ frames: 5 });
    await installEarthCyberInterface(game);

    const [nanites] = await game.entities.byTemplate(EARTH_NANITES);
    const [replicator] = await game.entities.byTemplate(EARTH_REPLICATOR);
    assert.ok(nanites, "Earth should contain authored nanite pile 257");
    assert.ok(replicator, "Earth should contain authored RepBase 262");

    await collectNanites(game, nanites);
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

    // Small HE Clip is hacked inventory box 0 (142x62 at 10,8). Aim the two
    // production hands at separate points inside that same button so both
    // emit hover samples, then hold only the right trigger for two frames.
    // Isolate the left ray first: the shared debug pointer prefers the right
    // hand when both hover. Leave the verified left pose in place afterward.
    await game.input.set("right_hand.position", [0, -10, 0]);
    for (const [hand, x] of [["left", 55], ["right", 133]] as const) {
      await aimHandAtPanelPoint(game, [x, 40], hand);
      const pointer = (await game.ui.state()).pointer;
      assert.equal(pointer?.hand, hand);
      assert.ok(pointer?.canvas, `${hand} ray must hit the canvas`);
      assert.ok(Math.abs(pointer.canvas[0] - (x + 2)) < 2);
      assert.ok(Math.abs(pointer.canvas[1] - 164) < 2);
    }
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
    await aimHandAtPanelPoint(game, [94, 40], "right");
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
