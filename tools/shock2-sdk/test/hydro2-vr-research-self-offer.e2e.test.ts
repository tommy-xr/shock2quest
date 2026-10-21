import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";
import type {
  EntityDetailResult,
  EntitySummary,
  PhysicsBodySummary,
} from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import {
  add,
  aimVrHandAt,
  aimVrHandAtCanvas,
  normalize,
  scale,
  sub,
} from "./helpers/vr-hand.js";

// Regression for #1092 and #2379. This follows the 25th Anniversary Hydro2 VR
// production path:
//
//   Desk 713 --Contains--> Toxin-A 547
//   trigger desk -> cyber-interface MFD -> squeeze the rendered Toxin slot
//   held-item trigger -> research, with no panel on the hand -> release
//
// #1092: releasing the vial over its own Research world panel created a
// self-Contains link and removed its world refs and physics. #2379 removed
// that panel; the released vial must still be an ordinary world vial. Runtime
// ids are discovered on every launch; no debug Frob, Give, or quest mutation
// participates in the interaction under test.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SURVEY_LAB_DESK = 713;
const TOXIN_A = 547;

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
  "Hydro2 VR held Toxin-A researches without a panel and stays a re-grabbable world vial",
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

    assert.equal((await uiBodies(game)).length, 0, "loot must use the MFD");
    const deskGui = (await game.ui.state()).active_panel;
    assert.ok(deskGui, "Desk 713 must own the active container panel");
    assert.equal(deskGui.entity_id, desk.id);
    const toxinSlot = deskGui.elements.find(
      (element) => element.kind === "button" && element.entity_id === toxin.id,
    );
    assert.ok(toxinSlot, "Desk panel must render the authored Toxin-A slot");

    const pose = (await game.ui.state()).panel_pose;
    assert.ok(pose);
    const [x, y, w, h] = toxinSlot.rect;
    await aimVrHandAtCanvas(game, pose, [x + w / 2, y + h / 2]);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      toxin.id,
      "squeezing the real Desk slot must hold authored Toxin-A 547",
    );
    assert.equal(contains(await game.entities.detail(desk.id), toxin.id), false);

    await game.input.trigger("LeftHandLowerButton");
    await game.step({ frames: 2 });

    // Carry the real vial clear of its source desk before opening Research.
    // Dropping while still aimed through the desk can legitimately offer it
    // back to that container instead of exercising the self-offer path.
    const pickupPosition = (await game.info()).player.position;
    await game.input.set("left_hand.thumbstick", [0, -1]);
    await game.step({ frames: 20 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.step({ frames: 3 });
    const carried = (await game.info()).player;
    assert.equal(carried.right_hand_entity_id, toxin.id);
    assert.ok(
      Math.hypot(
        ...carried.position.map((value, axis) => value - pickupPosition[axis]),
      ) > 0.5,
      "ordinary locomotion must carry the vial away from the source desk",
    );

    // A held-item trigger uses ResearchableScript's real inventory action. With
    // no tricorder or cyber interface up it researches without opening any
    // panel - nothing exists on the hand to release the vial over.
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).active_panel, null, "held Toxin-A opens no panel");
    assert.equal((await uiBodies(game)).length, 0, "held Toxin-A opens no world panel");

    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    await assertPhysicalWorldToxin(game, toxin.id);

    // The native drop can leave the vial falling or rotating. Let its real
    // body settle before selecting a surface point that must remain under
    // the hand ray while the aim helper advances simulation frames.
    let settled = false;
    let lastBody: PhysicsBodySummary | undefined;
    for (let attempt = 0; attempt < 50; attempt++) {
      await game.step({ frames: 6 });
      const [body] = (await game.physics.bodies({ entityId: toxin.id })).bodies;
      lastBody = body;
      assert.ok(
        body?.is_enabled,
        `the dropped vial must keep its physical body while settling: ${JSON.stringify({ attempt, body })}`,
      );
      if (
        Math.hypot(...body.velocity) < 0.02 &&
        Math.hypot(...body.angular_velocity) < 0.05
      ) {
        settled = true;
        break;
      }
    }
    assert.ok(
      settled,
      `the dropped vial should settle before native re-grab: ${JSON.stringify(lastBody)}`,
    );

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
    const regrabAim = await aimVrHandAt(game, droppedAim.world_point, 0.35);
    const regrabHit = await game.raycast({
      start: regrabAim.start,
      // The actual hand ray continues beyond the selected surface.
      end: add(
        regrabAim.start,
        scale(normalize(sub(regrabAim.target, regrabAim.start)), 1),
      ),
      collision_groups: ["world", "entity", "selectable", "raycast", "ui"],
      max_distance: 1,
    });
    assert.equal(
      regrabHit.entity_id,
      toxin.id,
      `the production ray must reach the settled vial before re-grabbing: ${JSON.stringify(regrabHit)}`,
    );
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      toxin.id,
      "the surviving world vial must be re-grabbable through production VR input",
    );
  },
);
