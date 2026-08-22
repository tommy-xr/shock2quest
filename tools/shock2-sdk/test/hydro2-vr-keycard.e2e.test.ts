import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Authentic Hydro2 Card B chain for #583:
//   corpse 754 --Contains--> card 942 (PropKeySrc region 128)
//   card slots 1120/1122 --SwitchLink--> HydroSectorB door 1127
//
// Both cases use default VR and production hand input. The backpack case uses
// /give only to stage the authored card in the backpack; credential acquisition
// itself must come from squeezing the rendered slot, not an injected Frob.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const CORPSE = 754;
const CARD = 942;
const CARD_SLOT = 1120;
const DOOR = 1127;
const CARD_ARCHETYPE = -1495;
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;
const LOOT_PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const BACKPACK_PANEL_SIZE_PX: Vec3 = [635, 120, 0];
const VR_BACKPACK_WORLD_SCALE = 0.55;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

async function squeezeWorldPanelElement(
  game: GameServer,
  panelSizePx: Vec3,
  worldScale: number,
  element: UiElement,
): Promise<void> {
  const panel = (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );
  assert.equal(panel.length, 1, "expected exactly one production VR panel collider");
  const [x, y, width, height] = element.screen_rect;
  const u = x + width / 2;
  const v = y + height / 2;
  const panelSize: Vec3 = [
    panelSizePx[0] * GUI_PIXEL_TO_WORLD_SIZE * worldScale,
    panelSizePx[1] * GUI_PIXEL_TO_WORLD_SIZE * worldScale,
    0,
  ];
  const local: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
  const target = add(panel[0].position, quatRotate(panel[0].rotation, local));
  const aim = await aimVrHandAt(game, target, 0.35);
  const hit = await game.raycast({
    start: aim.start,
    end: aim.target,
    collision_groups: ["ui"],
    max_distance: 1,
  });
  assert.equal(hit.entity_id, panel[0].entity_id, "production hand ray must hit the panel");

  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 4 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
}

async function assertCardCollectedAndDoorOpens(
  game: GameServer,
  collectedTemplate: number,
): Promise<void> {
  assert.equal(
    (await game.entities.byTemplate(collectedTemplate)).length,
    0,
    "collecting Card B must consume the physical keycard object",
  );
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    null,
    "a collected credential must not remain physically held",
  );

  const slot = only(await game.entities.byTemplate(CARD_SLOT), "Hydro2 Card B slot 1120");
  const door = only(await game.entities.byTemplate(DOOR), "HydroSectorB door 1127");
  const before = (await game.entities.detail(door.id)).position;
  await teleportVerified(game, {
    x: slot.position[0] + 1.1,
    y: slot.position[1] + 0.4,
    z: slot.position[2] + 1.1,
  });
  const aim = await game.player.aimAt(slot, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
  await aimVrHandAt(game, aim.world_point);
  const since = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 180 });

  const after = (await game.entities.detail(door.id)).position;
  assert.ok(
    distance(after, before) > 0.5,
    `Card B credential must open door 1127: before=${before}, after=${after}`,
  );
  const played = (await game.audio.recent()).sounds
    .filter((sound) => sound.sequence > since)
    .map((sound) => sound.sample.toLowerCase());
  assert.ok(!played.includes("hackfail"), `Card B slot refused credential: ${played}`);
}

test(
  "Hydro2 VR corpse-panel squeeze collects Card B and opens its authored door",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8620),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    const corpse = only(await game.entities.byTemplate(CORPSE), "Hydro2 corpse 754");
    const card = only(await game.entities.byTemplate(CARD), "Hydro Card B 942");

    // Reviewed campaign standing point under this alcove's seven-foot ceiling.
    await teleportVerified(game, { x: 73.08, y: -0.76, z: -14.92 });
    await game.step({ frames: 20 });
    const corpseAim = await game.player.aimAt(corpse, {
      hitbox: "center",
    });
    await aimVrHandAt(game, corpseAim.world_point);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 8 });

    const active = (await game.ui.state()).active_panel;
    assert.equal(active?.entity_id, corpse.id, "corpse 754 must open its real loot panel");
    const cardElement = active.elements.find(
      (element) => element.kind === "button" && element.entity_id === card.id,
    );
    assert.ok(cardElement, "corpse 754 must expose its real Card B button");
    await squeezeWorldPanelElement(game, LOOT_PANEL_SIZE_PX, 1, cardElement);
    await assertCardCollectedAndDoorOpens(game, CARD);
  },
);

test(
  "Hydro2 VR backpack squeeze repairs an unregistered carried Card B",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8622),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    const card = await game.player.spawnItem(CARD_ARCHETYPE);
    assert.equal(
      (await game.player.inventory()).items.find((item) => item.entity_id === card.entity_id)
        ?.location,
      "inventory",
      "setup must stage the exact authored card in the backpack",
    );

    await game.input.set("head.look", [0, 0]);
    await game.input.trigger("MoveInventory");
    await game.step({ frames: 8 });
    const active = (await game.ui.state()).active_panel;
    const cardElement = active?.elements.find(
      (element) => element.kind === "button" && element.entity_id === card.entity_id,
    );
    assert.ok(cardElement, "VR backpack must expose the staged Card B button");
    await squeezeWorldPanelElement(
      game,
      BACKPACK_PANEL_SIZE_PX,
      VR_BACKPACK_WORLD_SCALE,
      cardElement,
    );
    await assertCardCollectedAndDoorOpens(game, CARD_ARCHETYPE);
  },
);
