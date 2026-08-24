import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// End-to-end regression for #668, reproducing the campaign's exact production
// path before covering the sibling item and flat presentation:
//
//   medsci2 Desk 471 --Contains--> Med Patch 1054
//   hand trigger -> loot panel -> hand squeeze retrieval
//   held-item trigger -> authored Frob -> timed retail healing course
//
// Retail allobjs.osm behavior is data-driven by the two script names:
// MedPatchScript restores 10 HP in 2-HP pulses; MedKitScript restores 200 HP
// in 5-HP pulses. Both wait 0.1s before the first pulse and then 1.5s between
// pulses, preserve the item at full health, and receive Pharmo-Friendly's 20%
// integer bonus.
//
// Negative-first on campaign HEAD 9698555b: both script names resolve to
// UnimplementedScript. The authentic trigger reaches Med Patch 1054, but HP
// remains unchanged and the exact held entity survives.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const MEDSCI2_DESK_WITH_PATCH = 471;
const MEDSCI2_PATCH = 1054;
const PANEL_PIXEL_TO_WORLD = 1 / 250;
const LOOT_PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const BACKPACK_PANEL_SIZE_PX: Vec3 = [635, 120, 0];
const VR_BACKPACK_SCALE = 0.55;

function hp(info: Awaited<ReturnType<GameServer["info"]>>): number {
  assert.notEqual(
    info.player.hit_points,
    null,
    "mission player should have HP",
  );
  return info.player.hit_points as number;
}

async function exactMissionEntity(
  game: GameServer,
  missionObjectId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(missionObjectId);
  assert.equal(
    matches.length,
    1,
    `expected exactly one ${label} (${missionObjectId}), got ${JSON.stringify(matches)}`,
  );
  return matches[0];
}

function canvasPointWorld(
  panel: { position: Vec3; rotation: [number, number, number, number] },
  panelPixels: Vec3,
  pointPixels: Vec3,
  worldScale: number,
): Vec3 {
  const panelSize: Vec3 = [
    panelPixels[0] * PANEL_PIXEL_TO_WORLD * worldScale,
    panelPixels[1] * PANEL_PIXEL_TO_WORLD * worldScale,
    0,
  ];
  const u = pointPixels[0] / panelPixels[0];
  const v = pointPixels[1] / panelPixels[1];
  const local: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
  return add(panel.position, quatRotate(panel.rotation, local));
}

async function onlyUiPanel(game: GameServer, context: string) {
  const panels = (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );
  assert.equal(
    panels.length,
    1,
    `${context} must expose exactly one world panel`,
  );
  return panels[0];
}

async function stripItem(
  game: GameServer,
  entityId: number,
): Promise<UiElement> {
  const ui = await game.ui.state();
  assert.equal(
    ui.mode,
    "use",
    "flat healing-item use must happen in live use mode",
  );
  const item = ui.strip?.elements.find(
    (candidate) => candidate.entity_id === entityId,
  );
  assert.ok(
    item,
    `inventory strip should expose carried healing item ${entityId}`,
  );
  return item;
}

async function useFlatInventoryItem(
  game: GameServer,
  entityId: number,
): Promise<void> {
  const item = await stripItem(game, entityId);
  await clickUiElement(game, item);
  assert.equal((await game.ui.state()).cursor?.entity_id, entityId);
  await clickUiElement(game, item);
  assert.equal((await game.ui.state()).cursor, null);
}

async function buyPharmoFriendly(game: GameServer): Promise<void> {
  const machines = (await game.entities.list({ filter: "Trait Machine" }))
    .entities;
  const machine = machines.find((entity) => entity.template_id === 133);
  assert.ok(machine, "medsci2 should contain Trait Machine mission object 133");
  await teleportVerified(game, {
    x: machine.position[0] + 1.2,
    y: machine.position[1] + 0.5,
    z: machine.position[2],
  });
  await game.entities.sendMessage(machine.id, { type: "Frob" });
  await game.step({ frames: 5 });
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "the real Trait Machine should open its panel");
  const pharmo = panel.elements.find(
    (element) =>
      element.kind === "button" && element.label === "Pharmo-Friendly",
  );
  assert.ok(pharmo, "Trait Machine must expose the Pharmo-Friendly choice");
  await clickUiElement(game, pharmo);
  await game.step({ frames: 3 });
  assert.deepEqual(
    (await game.info()).player.stats?.os_traits,
    [2],
    "the real machine must acquire Pharmo-Friendly (trait 2)",
  );
}

test(
  "medsci2 VR Desk 471 Med Patch heals through backpack, held trigger, and save/load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const initial = await game.info();
    assert.equal(hp(initial), 30);
    assert.equal(initial.player.max_hit_points, 30);
    const playerId = initial.player.entity_id;
    assert.notEqual(playerId, null, "medsci2 should have a live player");
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 10 });
    await game.step({ frames: 2 });
    assert.equal(hp(await game.info()), 20);

    const desk = await exactMissionEntity(
      game,
      MEDSCI2_DESK_WITH_PATCH,
      "medsci2 Desk with Med Patch",
    );
    const deskDetail = await game.entities.detail(desk.id);
    const contains = deskDetail.outgoing_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    assert.equal(
      contains.length,
      1,
      "Desk 471 should contain exactly its Med Patch",
    );
    assert.equal(contains[0].target_name, "Med Patch");
    assert.equal(
      typeof contains[0].contains_ordinal,
      "number",
      `Desk link must expose its exact slot: ${JSON.stringify(contains[0])}`,
    );
    const patchId = contains[0].target_id;
    const exactPatch = await exactMissionEntity(
      game,
      MEDSCI2_PATCH,
      "Desk 471 Med Patch",
    );
    assert.equal(
      exactPatch.id,
      patchId,
      "mission object 1054 must be Desk 471's loot",
    );

    await teleportVerified(game, {
      x: desk.position[0] + 1.2,
      y: desk.position[1] + 0.5,
      z: desk.position[2] + 1.2,
    });
    await game.step({ frames: 5 });
    const deskAim = await game.player.aimAt(desk, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      deskAim.target_confirmed,
      true,
      "Desk 471 must be physically reachable",
    );
    await aimVrHandAt(game, deskAim.world_point);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    const lootPanel = await onlyUiPanel(game, "Desk 471 frob");
    // Desk 471 is yawed in the mission. Stand on the panel's authored front
    // normal instead of remaining on top of the desk, so the controller ray
    // crosses its face (rather than grazing the thin collider edge).
    const panelFront = quatRotate(lootPanel.rotation, [0, 0, -1]);
    const eyeHeight = (await game.info()).player.camera_offset[1];
    await teleportVerified(game, {
      x: lootPanel.position[0] + panelFront[0] * 1.25,
      y: lootPanel.position[1] - eyeHeight,
      z: lootPanel.position[2] + panelFront[2] * 1.25,
    });
    await game.step({ frames: 3 });
    const lootUi = (await game.ui.state()).active_panel;
    assert.ok(lootUi, "Desk 471's VR panel must be introspectable");
    const lootElement = lootUi.elements.find(
      (element) => element.entity_id === patchId,
    );
    assert.ok(
      lootElement,
      `the real panel must render exact Med Patch ${patchId}: ${JSON.stringify(lootUi)}`,
    );
    const [lootX, lootY, lootW, lootH] = lootElement.rect;
    const lootSlot = canvasPointWorld(
      lootPanel,
      LOOT_PANEL_SIZE_PX,
      [lootX + lootW / 2, lootY + lootH / 2, 0],
      1,
    );
    const lootAim = await aimVrHandAt(game, lootSlot, 0.35);
    const lootHit = await game.raycast({
      start: lootAim.start,
      end: lootAim.target,
      collision_groups: ["ui"],
      max_distance: 1,
    });
    assert.equal(
      lootHit.entity_id,
      lootPanel.entity_id,
      "the hand ray must hit the patch slot",
    );
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 10 });
    assert.equal(
      (await game.player.inventory()).items.find(
        (item) => item.entity_id === patchId,
      )?.location,
      "right_hand",
      "squeezing the real loot icon must retrieve the exact patch from Desk 471",
    );
    assert.equal(
      (await game.entities.detail(desk.id)).outgoing_links.some(
        (link) =>
          link.target_id === patchId && link.link_type.startsWith("Contains"),
      ),
      false,
      "physical loot must sever Desk 471's exact Contains link",
    );

    // Close the Desk panel without releasing the physical patch: walk away,
    // and the bound panel's distance auto-close puts it away (the original's
    // per-overlay `distance`). The old backpack round-trip here - drop,
    // world-trigger pickup into the backpack, Quest-X retrieval - is gone
    // with the world-quad backpack; panel-side retrieval returns with the
    // cyber interface's pointer slice, and the stored-pickup behavior itself
    // is covered by the flat presentation test below.
    await teleportVerified(game, {
      x: initial.player.position[0],
      y: initial.player.position[1],
      z: initial.player.position[2],
    });
    await game.step({ frames: 10 });
    assert.equal(
      (await game.physics.bodies()).bodies.filter((body) =>
        body.collision_groups.includes("ui"),
      ).length,
      0,
      "walking away must close the transient desk panel before the use",
    );

    // Use the patch straight from the holding hand: held-item trigger is the
    // authored Frob.
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    const messages = (await game.messages.recent()).messages.filter(
      (message) => message.to.entity_id === patchId,
    );
    assert.ok(messages.some((message) => message.payload === "Frob"));
    assert.ok(!messages.some((message) => message.payload === "TriggerPull"));
    assert.equal(
      hp(await game.info()),
      20,
      "retail healing must not be instantaneous",
    );
    assert.equal(
      (await game.entities.byTemplate(MEDSCI2_PATCH)).length,
      0,
      "a successful below-max use must consume the exact mission entity",
    );
    assert.ok(
      !(await game.player.inventory()).items.some(
        (item) => item.entity_id === patchId,
      ),
      "consumption must clear the exact patch from every carried location",
    );

    await game.step({ frames: 7 });
    assert.equal(
      hp(await game.info()),
      22,
      "the first pulse restores exactly 2 HP after 0.1s",
    );
    await game.screenshot("issue668-vr-medpatch-first-pulse.png");

    const saveName = `issue668_vr_medpatch_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 80 });
    assert.equal(
      hp(await game.info()),
      22,
      "load must preserve the next-pulse delay",
    );
    await game.step({ frames: 15 });
    assert.equal(
      hp(await game.info()),
      24,
      "the saved course must resume at 1.5s cadence",
    );
    await game.step({ frames: 300 });
    assert.equal(
      hp(await game.info()),
      30,
      "one Med Patch must spend exactly its 10-HP budget",
    );
    assert.equal((await game.entities.byTemplate(MEDSCI2_PATCH)).length, 0);
    await game.screenshot("issue668-vr-medpatch-complete.png");
  },
);

test(
  "flat Medical Kit heals to full while full-health Med Patch is preserved",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
    });
    await game.step({ frames: 5 });
    const initial = await game.info();
    const playerId = initial.player.entity_id;
    assert.notEqual(playerId, null);
    const maxHp = initial.player.max_hit_points;
    const psi = initial.player.psi_points;
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 29 });
    await game.step({ frames: 2 });
    assert.equal(hp(await game.info()), 1);

    const kit = await game.player.spawnItem("Medical Kit");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    await useFlatInventoryItem(game, kit.entity_id);
    assert.equal(
      hp(await game.info()),
      1,
      "Medical Kit use must not heal instantly",
    );
    await game.step({ frames: 5 });
    assert.equal(
      hp(await game.info()),
      6,
      "the delayed first pulse must restore exactly 5 HP",
    );
    assert.ok(
      !(await game.player.inventory()).items.some(
        (item) => item.entity_id === kit.entity_id,
      ),
      "a successful kit use must consume its source entity",
    );
    await game.step({ frames: 80 });
    assert.equal(
      hp(await game.info()),
      6,
      "the next pulse must not arrive before 1.5s",
    );
    await game.step({ frames: 15 });
    assert.equal(
      hp(await game.info()),
      11,
      "Medical Kit must continue in exact 5-HP pulses",
    );
    await game.step({ frames: 405 });
    const healed = await game.info();
    assert.equal(hp(healed), maxHp);
    assert.equal(healed.player.max_hit_points, maxHp);
    assert.equal(healed.player.psi_points, psi);

    const unused = await game.player.spawnItem("Med Patch");
    await useFlatInventoryItem(game, unused.entity_id);
    await game.step({ frames: 120 });
    assert.equal(hp(await game.info()), maxHp);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === unused.entity_id,
      ),
      "full-health inventory use must preserve the exact patch",
    );
    const saveName = `issue668_full_health_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.name === "Med Patch",
      ),
      "full-health refusal must survive save/load",
    );
  },
);

test(
  "Pharmo-Friendly gives Med Patch the retail 12-HP integer budget",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
    });
    await game.step({ frames: 5 });
    await buyPharmoFriendly(game);

    await game.transitionLevel("earth.mis");
    await game.step({ frames: 5 });
    assert.deepEqual(
      (await game.info()).player.stats?.os_traits,
      [2],
      "Pharmo-Friendly must survive the ordinary cross-mission character path",
    );

    const playerId = (await game.info()).player.entity_id;
    assert.notEqual(playerId, null);
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 20 });
    await game.step({ frames: 2 });
    assert.equal(hp(await game.info()), 10);
    const patch = await game.player.spawnItem("Med Patch");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    if ((await game.ui.state()).mode === "shooter") {
      // The first toggle dismisses the still-open Trait Machine MFD; the
      // second enters the ordinary inventory use mode.
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 5 });
    }
    await useFlatInventoryItem(game, patch.entity_id);
    await game.step({ frames: 500 });
    assert.equal(
      hp(await game.info()),
      22,
      "Pharmo-Friendly must increase a patch's 10-HP budget to exactly 12",
    );
    assert.ok(
      !(await game.player.inventory()).items.some(
        (item) => item.entity_id === patch.entity_id,
      ),
    );
  },
);
