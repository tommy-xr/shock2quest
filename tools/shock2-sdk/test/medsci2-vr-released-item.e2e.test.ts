import assert from "node:assert/strict";
import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer, findRepoRoot } from "../src/index.js";
import type { EntitySummary, UiElement, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// MedSci2 production-VR guard for #974: the whole carry cycle for an ordinary
// contained prop, player-observable and using no direct Frob/Grab/Give message:
//
//   Desk 503 loot-panel squeeze -> Gameboy 72 into the hand
//   first world release -> world re-grab
//   carry -> second world release -> selectable body -> save/load
//
// (The retrieval used to round-trip through the Quest-X backpack world quad;
// that panel was removed with the cyber-interface use mode, whose panel-side
// item interaction lands in a later slice - the loot panel's squeeze grab is
// the same production ContainerGui path.)
//
// Every release must leave the exact live entity world-referenced
// (`HasRefs(true)`), free of residual `Contains` links, and physically
// selectable through the production ray.
//
// The item is deliberately an ordinary physical prop (the desk's Gameboy), not
// the R&D card sharing the same desk: key sources are registered on the keyring
// and consumed rather than carried, so they never reach this hand/world path.
//
// This scenario also passes on the parent - on this base the grab path already
// restores refs for every case reachable from a real gesture - so it is a
// standing guard on the invariant rather than a failing-before reproduction.
// `restore_live_entity_world_refs`'s unit tests in `mission_core.rs` pin the
// released-item behavior (including the consumed-entity case) directly.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const DESK = 503;
const GAMEBOY = 72;
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

function savePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((root): root is string => Boolean(root));
  return roots
    .map((root) => join(root, "saves", `${saveName}.sav`))
    .find(existsSync);
}

function panelElementWorldPoint(
  panel: { position: Vec3; rotation: [number, number, number, number] },
  panelSizePx: Vec3,
  worldScale: number,
  element: UiElement,
): Vec3 {
  const centerX = element.rect[0] + element.rect[2] / 2;
  const centerY = element.rect[1] + element.rect[3] / 2;
  const width = panelSizePx[0] * GUI_PIXEL_TO_WORLD_SIZE * worldScale;
  const height = panelSizePx[1] * GUI_PIXEL_TO_WORLD_SIZE * worldScale;
  const local: Vec3 = [
    width * (0.5 - centerX / panelSizePx[0]),
    height * (0.5 - centerY / panelSizePx[1]),
    0,
  ];
  return add(panel.position, quatRotate(panel.rotation, local));
}

async function hasRefs(game: GameServer, entityId: number): Promise<string | undefined> {
  return (await game.entities.detail(entityId)).properties
    .find((property) => property.name === "HasRefs")
    ?.value.toLowerCase();
}

async function assertNoContains(game: GameServer, entityId: number): Promise<void> {
  const detail = await game.entities.detail(entityId);
  assert.equal(
    detail.incoming_links.some((link) => link.link_type.startsWith("Contains")),
    false,
    "released item must have no incoming Contains link",
  );
  assert.equal(
    detail.outgoing_links.some((link) => link.link_type.startsWith("Contains")),
    false,
    "released item must have no outgoing Contains link",
  );
}

async function assertSelectableWorldItem(
  game: GameServer,
  item: EntitySummary,
): Promise<void> {
  const bodies = (await game.physics.bodies({ entityId: item.id })).bodies;
  assert.equal(bodies.length, 1, "released item must have one authored physics body");
  assert.ok(
    bodies[0].collision_groups.includes("entity") && bodies[0].is_enabled,
    `released item must have an enabled entity body: ${JSON.stringify(bodies[0])}`,
  );
  assert.equal(await hasRefs(game, item.id), "true");
  await assertNoContains(game, item.id);

  const current = only(await game.entities.byTemplate(GAMEBOY), "released Gameboy");
  await teleportVerified(game, {
    x: current.position[0] + 1.0,
    y: current.position[1] + 0.5,
    z: current.position[2] + 1.0,
  });
  await game.step({ frames: 3 });
  const aim = await game.player.aimAt(current.id, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(
    aim.target_confirmed,
    true,
    `released item must be reachable through the production ray: ${JSON.stringify(aim)}`,
  );
}

test(
  "MedSci2 VR desk item survives a second world release",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    const saveName = `medsci2_vr_released_item_${Date.now()}`;
    t.after(() => {
      const path = savePath(saveName);
      if (path) rmSync(path, { force: true });
    });

    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 5 });

    const desk = only(await game.entities.byTemplate(DESK), "MedSci2 Desk 503");
    const item = only(await game.entities.byTemplate(GAMEBOY), "MedSci2 Gameboy 72");
    assert.equal(
      (await game.entities.detail(desk.id)).outgoing_links.some(
        (link) => link.target_id === item.id && link.link_type.startsWith("Contains"),
      ),
      true,
      "Desk 503 must initially contain the exact Gameboy",
    );
    assert.equal((await game.physics.bodies({ entityId: item.id })).bodies.length, 0);
    assert.notEqual(
      await hasRefs(game, item.id),
      "true",
      "contained item must not begin world-referenced",
    );

    await teleportVerified(game, {
      x: desk.position[0] + 1.2,
      y: desk.position[1] + 0.5,
      z: desk.position[2] + 1.2,
    });
    await game.step({ frames: 5 });
    const deskAim = await game.player.aimAt(desk.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(deskAim.target_confirmed, true, JSON.stringify(deskAim));
    await aimVrHandAt(game, deskAim.world_point, 0.35);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    const lootUi = (await game.ui.state()).active_panel;
    assert.equal(lootUi?.entity_id, desk.id, "physical desk frob must open its loot panel");
    const itemButton = lootUi.elements.find(
      (element) => element.kind === "button" && element.entity_id === item.id,
    );
    assert.ok(itemButton, "Desk 503 panel must render exact Gameboy 72");
    const lootPanel = (await game.physics.bodies()).bodies.find((body) =>
      body.collision_groups.includes("ui"),
    );
    assert.ok(lootPanel, "loot panel must have a production VR collider");
    const itemButtonWorld = panelElementWorldPoint(
      lootPanel,
      [188, 296, 0],
      1,
      itemButton,
    );
    // Squeeze the panel's item button: ContainerGui's production grab pulls
    // the exact Gameboy out of the desk and into the hand.
    await aimVrHandAt(game, itemButtonWorld, 0.35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, item.id);
    await assertNoContains(game, item.id);

    // Walk clear of the desk before dropping: its panel auto-closes on
    // distance (the original's per-overlay `distance`), and a release within
    // the desk's give range would put the item straight back into the
    // container. The spot is open floor, so the drop is selectable.
    await teleportVerified(game, {
      x: desk.position[0] + 5.0,
      y: desk.position[1] + 0.5,
      z: desk.position[2] - 5.0,
    });
    await game.step({ frames: 10 });
    assert.equal(
      (await game.physics.bodies()).bodies.some((body) =>
        body.collision_groups.includes("ui"),
      ),
      false,
      "walking away must close the desk panel while squeeze keeps the item held",
    );
    assert.equal((await game.info()).player.right_hand_entity_id, item.id);

    // First release works on the parent and supplies a real world body.
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    await assertSelectableWorldItem(game, item);

    const firstDrop = only(await game.entities.byTemplate(GAMEBOY), "first-drop Gameboy");
    const regrabAim = await game.player.aimAt(firstDrop.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(regrabAim.target_confirmed, true, JSON.stringify(regrabAim));
    await aimVrHandAt(game, regrabAim.world_point, 0.35, 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      item.id,
      "production world squeeze must re-grab the same live item",
    );

    // Carry it clear while keeping squeeze held, then re-release. Before #974
    // this exact edge leaves HasRefs(false) and no selectable physics body.
    await game.input.set("right_hand.position", [0.45, -0.15, -0.65]);
    await game.step({ frames: 10 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    const secondDrop = only(await game.entities.byTemplate(GAMEBOY), "second-drop Gameboy");
    await assertSelectableWorldItem(game, secondDrop);

    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    const loaded = only(await game.entities.byTemplate(GAMEBOY), "loaded Gameboy");
    await assertSelectableWorldItem(game, loaded);
  },
);
