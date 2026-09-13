import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement, UiPanelPose, UiState, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import {
  LOOT_PANEL_SIZE_PX,
  drawPersonalCard,
  aimVrHandAt,
  aimVrHandAtCanvas,
  squeezeWorldPanelElement,
} from "./helpers/vr-hand.js";

// Authentic Hydro2 Card B chain for #583:
//   corpse 754 --Contains--> card 942 (PropKeySrc region 128)
//   card slots 1120/1122 --SwitchLink--> HydroSectorB door 1127
//
// VR physical grips hold found cards until release. The downloaded credential
// opens its authored gate only after presenting the permanent personal card.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const CORPSE = 754;
const CARD = 942;
const CARD_SLOT = 1120;
const DOOR = 1127;
const CARD_ARCHETYPE = -1495;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/** The strip element bound to `entityId`, or undefined once it has left the grid. */
function stripSlotFor(ui: UiState, entityId: number): UiElement | undefined {
  return ui.strip?.elements.find((element) => element.entity_id === entityId);
}

function requirePanel(ui: UiState): UiPanelPose {
  assert.ok(ui.panel_pose, "the VR cyber interface must report its panel pose");
  return ui.panel_pose;
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

  assert.ok(distance((await game.entities.detail(door.id)).position, before) < 0.05,
    "owning a credential must not let empty-hand Frob bypass scanning");
  await drawPersonalCard(game);
  await aimVrHandAt(game, aim.world_point, 0.20, 1);
  await game.step({ frames: 180 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 3 });

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
  "Hydro2 VR corpse-panel grip holds Card B until release, then scans its gate",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
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
    // The panel helper completes the release too; state is collected on return.
    await game.input.set("right_hand.position", [0.3, 0.9, -0.5]);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
    await assertCardCollectedAndDoorOpens(game, CARD);
  },
);

test(
  "Hydro2 VR strip grip holds Card B and release downloads its credential",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    // Stage the authored card in the backpack to exercise the strip's grip
    // path independently of the corpse-panel path above.
    const card = await game.player.spawnItem(CARD_ARCHETYPE);
    assert.equal(
      (await game.player.inventory()).items.find((item) => item.entity_id === card.entity_id)
        ?.location,
      "inventory",
      "setup must stage the exact authored card in the backpack",
    );

    await game.input.set("head.look", [0, 0]);
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    let ui = await game.ui.state();
    assert.equal(ui.mode, "use", "ToggleUseMode must open the cyber interface in VR");
    const panel = requirePanel(ui);
    const cardSlot = stripSlotFor(ui, card.entity_id);
    assert.ok(cardSlot, "the staged Card B must occupy a strip slot");
    const [x, y, width, height] = cardSlot.rect;
    const slotCenter: [number, number] = [x + width / 2, y + height / 2];

    await aimVrHandAtCanvas(game, panel, slotCenter, { squeeze: 0 });
    await game.step({ frames: 3 });
    await aimVrHandAtCanvas(game, panel, slotCenter, { squeeze: 1 });
    await game.step({ frames: 8 });

    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      card.entity_id,
      "a keycard squeeze must hold the credential until release",
    );
    ui = await game.ui.state();
    assert.equal(
      ui.strip?.elements.find(element => element.entity_id === card.entity_id && element.kind === "button"),
      undefined,
      "the held card must leave backpack cells (the hand readout may remain)",
    );

    // Leave the cyber interface - it swallows the world trigger while open,
    // so the reader interaction below needs the ordinary shooter-mode hand.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    assert.equal((await game.ui.state()).mode, "shooter");

    await game.input.set("right_hand.position", [0.3, 0.9, -0.5]);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
    await assertCardCollectedAndDoorOpens(game, CARD_ARCHETYPE);
  },
);
