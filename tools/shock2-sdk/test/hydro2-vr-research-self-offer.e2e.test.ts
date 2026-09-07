import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";
import type {
  EntityDetailResult,
  EntitySummary,
  PhysicsBodySummary,
  Vec3,
} from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Regression for #1092. This follows the exact 25th Anniversary Hydro2 VR
// production path that exposed the bug:
//
//   Desk 713 --Contains--> Toxin-A 547
//   trigger desk -> ContainerGui proxy -> squeeze the rendered Toxin slot
//   held-item trigger -> ResearchGui proxy -> release over that proxy
//
// The proxy forwards the offer to its parent Toxin-A. Before the fix the
// shared transfer fallback accepted `parent == dropped`, creating a
// self-Contains link and removing the vial's world refs and physics. Runtime
// ids are discovered on every launch; no debug Frob, Give, or quest mutation
// participates in the interaction under test.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SURVEY_LAB_DESK = 713;
const TOXIN_A = 547;
const PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

function contains(detail: EntityDetailResult, itemId: number): boolean {
  return detail.outgoing_links.some(
    (link) =>
      link.target_id === itemId && link.link_type.startsWith("Contains"),
  );
}

const uiBodies = async (game: GameServer): Promise<PhysicsBodySummary[]> =>
  (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );

async function openPanelThroughVrTrigger(
  game: GameServer,
  target: EntitySummary,
): Promise<void> {
  const aim = await game.player.aimAt(target.id, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
  await aimVrHandAt(game, aim.world_point);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 5 });
}

async function assertPhysicalWorldToxin(
  game: GameServer,
  toxinId: number,
): Promise<void> {
  const detail = await game.entities.detail(toxinId);
  assert.equal(
    property(detail, "HasRefs")?.toLowerCase(),
    "true",
    "released Toxin-A must remain world-referenced",
  );
  assert.equal(
    contains(detail, toxinId),
    false,
    "released Toxin-A must not contain itself",
  );
  assert.equal(
    detail.incoming_links.some(
      (link) =>
        link.target_id === toxinId && link.link_type.startsWith("Contains"),
    ),
    false,
    "released Toxin-A must not be contained by itself",
  );
  const bodies = (await game.physics.bodies({ entityId: toxinId })).bodies;
  assert.equal(bodies.length, 1, "released Toxin-A must have one authored body");
  assert.ok(
    bodies[0].is_enabled && bodies[0].collision_groups.includes("entity"),
    `released Toxin-A must have an enabled entity body: ${JSON.stringify(bodies[0])}`,
  );
  assert.ok(
    (await game.scene.objects({ entityId: toxinId })).objects.length > 0,
    "released Toxin-A must remain rendered",
  );
}

test(
  "Hydro2 VR release over Toxin-A's own Research panel remains a re-grabbable world vial",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: e2ePort(),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 10 });

    const desk = only(
      await game.entities.byTemplate(SURVEY_LAB_DESK),
      "Hydro2 Survey Lab Desk 713",
    );
    const toxin = only(
      await game.entities.byTemplate(TOXIN_A),
      "Hydro2 Toxin-A 547",
    );
    assert.equal(
      contains(await game.entities.detail(desk.id), toxin.id),
      true,
      "Desk 713 must author the Toxin-A containment used by the campaign repro",
    );

    await teleportVerified(game, {
      // Authored Survey Lab floor immediately west of Desk 713. This exact
      // pose is collision-supported and leaves the desk's multi-OBB gaps open
      // to the production hand ray.
      x: 24.550903,
      y: -1.2804077,
      z: 42.3988,
    });
    await game.step({ frames: 3 });
    await openPanelThroughVrTrigger(game, desk);

    const [deskPanel] = await uiBodies(game);
    assert.ok(deskPanel, "production desk frob must open one VR loot panel");
    assert.equal((await uiBodies(game)).length, 1);
    const deskGui = (await game.ui.state()).active_panel;
    assert.ok(deskGui, "Desk 713 must own the active container panel");
    assert.equal(deskGui.entity_id, desk.id);
    const toxinSlot = deskGui.elements.find(
      (element) => element.kind === "button" && element.entity_id === toxin.id,
    );
    assert.ok(toxinSlot, "Desk panel must render the authored Toxin-A slot");

    // Invert ProxyGuiScript's world -> normalized-canvas map for the rendered
    // Toxin slot in the 188x296 container canvas.
    const panelSize: Vec3 = [
      PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
      PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
      0,
    ];
    const slotCenter: Vec3 = [
      toxinSlot.rect[0] + toxinSlot.rect[2] / 2,
      toxinSlot.rect[1] + toxinSlot.rect[3] / 2,
      0,
    ];
    const u = slotCenter[0] / PANEL_SIZE_PX[0];
    const v = slotCenter[1] / PANEL_SIZE_PX[1];
    const localSlot: Vec3 = [
      panelSize[0] * (0.5 - u),
      panelSize[1] * (0.5 - v),
      0,
    ];
    const slotWorld = add(
      deskPanel.position,
      quatRotate(deskPanel.rotation, localSlot),
    );
    const slotAim = await aimVrHandAt(game, slotWorld, 0.35);
    const slotHit = await game.raycast({
      start: slotAim.start,
      end: slotAim.target,
      collision_groups: ["ui"],
      max_distance: 1,
    });
    assert.equal(slotHit.entity_id, deskPanel.entity_id);
    const productionSlotHit = await game.raycast({
      start: slotAim.start,
      end: slotAim.target,
      collision_groups: ["world", "entity", "selectable", "ui"],
      max_distance: 1,
    });
    assert.equal(
      productionSlotHit.entity_id,
      deskPanel.entity_id,
      `the unfiltered production ray must reach the slot: ${JSON.stringify(productionSlotHit)}`,
    );
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      toxin.id,
      "squeezing the real Desk slot must hold authored Toxin-A 547",
    );
    assert.equal(contains(await game.entities.detail(desk.id), toxin.id), false);

    // A held-item trigger uses ResearchableScript's real inventory action and
    // replaces the desk panel with the Toxin-A-bound Research GUI.
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });
    const researchPanel = (await game.ui.state()).active_panel;
    assert.ok(researchPanel, "held Toxin-A trigger must open its Research panel");
    assert.equal(researchPanel.entity_id, toxin.id);
    assert.ok(
      researchPanel.elements.some(
        (element) =>
          element.kind === "image" &&
          element.texture?.toLowerCase() === "iface/research.pcx",
      ),
      "the active panel must be the real Research GUI",
    );

    const [researchProxy] = await uiBodies(game);
    assert.ok(researchProxy, "Research GUI must expose one production UI proxy");
    assert.equal((await uiBodies(game)).length, 1);
    const panelAim = await aimVrHandAt(game, researchProxy.position, 0.35, 1);
    const panelHit = await game.raycast({
      start: panelAim.start,
      end: panelAim.target,
      collision_groups: ["ui"],
      max_distance: 1,
    });
    assert.equal(
      panelHit.entity_id,
      researchProxy.entity_id,
      "held hand ray must target Toxin-A's attached Research proxy",
    );

    // This squeeze edge first performs the ordinary physical world DropItem,
    // then offers the vial through its own proxy. The offer must be rejected
    // without undoing that already-completed drop.
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    await assertPhysicalWorldToxin(game, toxin.id);

    // Player-observable recovery: select the dropped vial through the real
    // hand ray and squeeze it back into the same hand.
    const dropped = only(
      await game.entities.byTemplate(TOXIN_A),
      "released Hydro2 Toxin-A 547",
    );
    const droppedAim = await game.player.aimAt(dropped.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(droppedAim.target_confirmed, true, JSON.stringify(droppedAim));
    await aimVrHandAt(game, droppedAim.world_point, 0.35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      toxin.id,
      "the surviving world vial must be re-grabbable through production VR input",
    );
  },
);
