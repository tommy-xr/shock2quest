import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Default-VR regression for #964. This uses the real Earth Basic Juice
// (mission object/template 355) and only production interactions:
//
//   hand trigger -> InternalFrobMove -> backpack Contains(slot 0)
//   MoveInventory -> internal_inventory world panel
//   hand squeeze over slot 0 -> ContainerGui::GrabEntity -> right hand
//
// The save/load occurs while the Juice is still stored, so the second half
// proves that its authored containment slot and the VR retrieval path survive
// entity re-instantiation. Runtime ids are intentionally rediscovered after
// load; within each side of the load, the exact id must remain unchanged from
// panel slot to hand.
//
// Negative-first: on main, MoveInventory only moves the synthetic inventory's
// blue placeholder cube. It never opens internal_inventory, so no UI body is
// present and the panel assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const BASIC_JUICE = 355;
const BACKPACK_PANEL_SIZE_PX: Vec3 = [635, 120, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;
const VR_BACKPACK_WORLD_SCALE = 0.55;
const BACKPACK_SLOT_ZERO_CENTER_PX: Vec3 = [4 + 35 / 2, 17 + 34 / 2, 0];

async function exactBasicJuice(game: GameServer): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(BASIC_JUICE);
  assert.equal(
    matches.length,
    1,
    `expected the exact Earth Basic Juice (355), got ${JSON.stringify(matches)}`,
  );
  return matches[0];
}

async function backpackContainsLink(game: GameServer, itemId: number) {
  const backpackId = (await game.info()).player.inventory_entity_id;
  assert.ok(backpackId, "the production snapshot must expose the live backpack id");
  const link = (await game.entities.detail(backpackId)).outgoing_links.find(
    (candidate) =>
      candidate.target_id === itemId && candidate.link_type.startsWith("Contains"),
  );
  assert.ok(link, `backpack ${backpackId} must contain exact item ${itemId}`);
  return { backpackId, link };
}

test(
  "VR backpack exposes stored Earth Juice and retrieves its exact slot into a hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8564),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const juiceBeforeSave = await exactBasicJuice(game);
    await teleportVerified(game, {
      x: juiceBeforeSave.position[0] + 1.0,
      y: juiceBeforeSave.position[1] + 0.4,
      z: juiceBeforeSave.position[2] + 1.0,
    });
    const juiceAim = await game.player.aimAt(juiceBeforeSave, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(juiceAim.target_confirmed, true, "the Basic Juice must be physically reachable");
    await aimVrHandAt(game, juiceAim.world_point, 0.35);

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    const storedBeforeSave = (await game.player.inventory()).items.find(
      (item) => item.entity_id === juiceBeforeSave.id,
    );
    assert.equal(
      storedBeforeSave?.location,
      "inventory",
      "the exact physically-frobbed Juice must enter the backpack",
    );
    const containsBeforeSave = await backpackContainsLink(game, juiceBeforeSave.id);
    assert.equal(containsBeforeSave.link.contains_ordinal, 0, "Juice must occupy exact slot 0");
    assert.equal((await game.info()).player.right_hand_entity_id, null);

    const saveName = `issue964_vr_backpack_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });

    const juiceAfterLoad = await exactBasicJuice(game);
    const storedAfterLoad = (await game.player.inventory()).items.find(
      (item) => item.entity_id === juiceAfterLoad.id,
    );
    assert.equal(
      storedAfterLoad?.location,
      "inventory",
      "save/load must preserve the Juice in its backpack slot",
    );
    const containsAfterLoad = await backpackContainsLink(game, juiceAfterLoad.id);
    assert.equal(
      containsAfterLoad.link.contains_ordinal,
      0,
      "save/load must preserve exact backpack slot 0",
    );
    assert.equal((await game.info()).player.right_hand_entity_id, null);

    // Open from a level gaze instead of inheriting the downward pickup look.
    // MoveInventory deliberately follows the head so a real player can place
    // the panel clear of nearby geometry by looking where they want it.
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 2 });
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 5 });

    const uiBodies = (await game.physics.bodies()).bodies.filter((body) =>
      body.collision_groups.includes("ui"),
    );
    assert.equal(
      uiBodies.length,
      1,
      "MoveInventory in VR must expose one physical internal_inventory panel",
    );
    const panel = uiBodies[0];
    assert.equal(panel.body_type, "kinematic");

    // Invert ProxyGuiScript's world -> normalized-canvas mapping for the exact
    // center of slot 0. Juice is the only stored item, and its saved Contains
    // ordinal is zero, so a squeeze here must retrieve that same runtime id.
    const panelSize: Vec3 = [
      BACKPACK_PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE * VR_BACKPACK_WORLD_SCALE,
      BACKPACK_PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE * VR_BACKPACK_WORLD_SCALE,
      0,
    ];
    const u = BACKPACK_SLOT_ZERO_CENTER_PX[0] / BACKPACK_PANEL_SIZE_PX[0];
    const v = BACKPACK_SLOT_ZERO_CENTER_PX[1] / BACKPACK_PANEL_SIZE_PX[1];
    const localSlot: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
    const slotWorld = add(panel.position, quatRotate(panel.rotation, localSlot));

    await game.input.set("left_hand.rotation", [0, 1, 0, 0]);
    const panelAim = await aimVrHandAt(game, slotWorld, 0.35);
    const panelHit = await game.raycast({
      start: panelAim.start,
      end: panelAim.target,
      collision_groups: ["ui"],
      max_distance: 1,
    });
    assert.equal(panelHit.entity_id, panel.entity_id, "the hand ray must hit backpack slot 0");

    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 5 });

    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      juiceAfterLoad.id,
      "squeezing slot 0 must retrieve the exact rendered Juice into the right hand",
    );
    const retrieved = (await game.player.inventory()).items.find(
      (item) => item.entity_id === juiceAfterLoad.id,
    );
    assert.equal(retrieved?.location, "right_hand");
    assert.equal(
      (await game.entities.detail(containsAfterLoad.backpackId)).outgoing_links.some(
        (link) => link.target_id === juiceAfterLoad.id && link.link_type.startsWith("Contains"),
      ),
      false,
      "retrieval must sever the exact slot-0 Contains link",
    );

    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 3 });
    assert.equal(
      (await game.physics.bodies()).bodies.filter((body) =>
        body.collision_groups.includes("ui"),
      ).length,
      0,
      "the same production action must close the transient backpack panel",
    );
  },
);
