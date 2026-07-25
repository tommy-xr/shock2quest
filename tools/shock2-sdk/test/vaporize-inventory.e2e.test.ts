import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement } from "../src/types.js";
import { aimAtWorldPoint } from "./helpers/aim.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";
import { teleportVerified } from "./helpers/teleport.js";

// Earth training gives the player temporary supplies which must not escape the
// Basic/Advanced rooms. Each authored exit tripwire SwitchLinks to a
// Player Teleport Trap with both inherited TrapTeleportPlayer and local
// VaporizeInventory scripts:
//
//   Basic    tripwire 380 -> trap 379
//   Weapons  tripwire 374 -> trap 371
//   Tech     tripwire 375 -> trap 372
//   Psionic  tripwire 376 -> trap 373
//
// The primary scenario acquires the real Basic supplies via ordinary gameplay
// input (crosshair squeeze + container UI), then enters the real exit sensor
// through a bounded collision-valid move. No debug give, script-message
// injection, or direct level transition is used.
//
// Negative-first: on main VaporizeInventory is a NoopScript. Trap 379 performs
// its teleport, but Chem #1 and the Juice bottle remain in the backpack, so the
// empty-inventory assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const BASIC_CRATE = 307;
const BASIC_CHEM = 242;
const BASIC_JUICE = 355;
const BASIC_EXIT_TRIPWIRE = 380;
const BASIC_EXIT_DESTINATION = 379;
const TECHNICAL_ENTRY_TRIPWIRE = 314;
const TECHNICAL_ENTRY_DESTINATION = 324;
const TECHNICAL_EXIT_TRIPWIRE = 375;
const TECHNICAL_EXIT_DESTINATION = 372;

const ADVANCED_EXITS = [
  { name: "Weapons", tripwire: 374, destination: 371 },
  { name: "Technical", tripwire: 375, destination: 372 },
  { name: "Psionic", tripwire: 376, destination: 373 },
] as const;

async function exactlyOne(
  game: GameServer,
  templateId: number,
  what: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(templateId);
  assert.equal(
    matches.length,
    1,
    `expected exactly one ${what} (mission object ${templateId}), got ${JSON.stringify(matches)}`,
  );
  return matches[0];
}

async function click(game: GameServer, element: UiElement): Promise<void> {
  const [x, y, width, height] = element.screen_rect;
  await game.input.set("pointer.position", [x + width / 2, y + height / 2]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

async function squeeze(game: GameServer): Promise<void> {
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 2 });
}

/**
 * Stand near an entity, compose a world-space look with the authored player
 * pawn rotation, and squeeze. Teleport is only spatial setup; acquisition
 * itself follows the production reticle/Frob/pickup path.
 */
async function frobNormally(
  game: GameServer,
  target: EntitySummary,
  didInteract: () => Promise<boolean>,
  aimOffsetY = 0,
): Promise<void> {
  const [tx, ty, tz] = target.position;
  const aimY = ty + aimOffsetY;
  // Mission props may sit tight against one wall. Try the four cardinal
  // approach faces, always using the production crosshair + squeeze path, and
  // stop as soon as the expected interaction state appears.
  const stands = [
    { x: tx - 2.3, y: ty, z: tz },
    { x: tx + 2.3, y: ty, z: tz },
    { x: tx, y: ty, z: tz - 2.3 },
    { x: tx, y: ty, z: tz + 2.3 },
  ];
  for (const stand of stands) {
    await teleportVerified(game, stand);
    await aimAtWorldPoint(game, [tx, aimY, tz]);
    await game.step({ frames: 3 });
    await squeeze(game);
    if (await didInteract()) {
      return;
    }
  }
  assert.fail(`ordinary crosshair squeeze could not interact with ${target.name}`);
}

async function acquireBasicItemsNormally(
  game: GameServer,
): Promise<{ chemId: number }> {
  const crate = await exactlyOne(game, BASIC_CRATE, "Basic crate");
  await frobNormally(
    game,
    crate,
    async () => (await game.ui.state()).active_panel?.entity_id === crate.id,
    0.7,
  );

  const opened = await game.ui.state();
  assert.equal(
    opened.active_panel?.entity_id,
    crate.id,
    `ordinary squeeze should open the Basic crate, got ${JSON.stringify(opened.active_panel)}`,
  );
  const chem = opened.active_panel.elements.find(
    (element) => element.kind === "button" && element.entity_id !== null,
  );
  assert.ok(chem, "Basic crate should expose its Fermium item button");
  const chemDetail = await game.entities.detail(chem.entity_id!);
  assert.equal(
    chemDetail.template_id,
    BASIC_CHEM,
    "the normally-looted Basic crate item must be authored Chem #1",
  );
  await click(game, chem);

  const juice = await exactlyOne(game, BASIC_JUICE, "Basic Juice bottle");
  await frobNormally(
    game,
    juice,
    async () =>
      (await game.player.inventory()).items.some((item) => item.entity_id === juice.id),
  );

  const inventory = await game.player.inventory();
  assert.deepEqual(
    inventory.items
      .map((item) => item.name)
      .sort(),
    ["Chem #1", "Juice bottle"],
    `ordinary interactions should acquire both Basic supplies, got ${JSON.stringify(inventory)}`,
  );
  return { chemId: chem.entity_id! };
}

test(
  "earth Basic exit vaporizes supplies acquired through normal interactions",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8196),
    });
    await game.step({ frames: 5 });

    const acquired = await acquireBasicItemsNormally(game);

    // Mirror the strict playthrough edge case: lift Chem #1 onto the use-mode
    // cursor before leaving. It remains a backpack-owned item, but the host has
    // an additional pure-UI EntityId reference which vaporization must clear.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const strip = (await game.ui.state()).strip;
    const chemElement = strip?.elements.find(
      (element) => element.entity_id === acquired.chemId,
    );
    assert.ok(chemElement, "use-mode strip should expose the acquired Chem #1");
    await click(game, chemElement);
    assert.equal(
      (await game.ui.state()).cursor?.entity_id,
      acquired.chemId,
      "Chem #1 should ride the cursor before crossing the exit",
    );

    await crossEarthTrainingTripwire(game, BASIC_EXIT_TRIPWIRE, BASIC_EXIT_DESTINATION);

    const inventory = await game.player.inventory();
    assert.equal(
      inventory.count,
      0,
      `the Basic exit must vaporize every temporary carried item, got ${JSON.stringify(inventory)}`,
    );
    // The cleanup is destruction, not merely hiding/removing container links.
    // Assert by stable mission identity: runtime EntityId slots may be recycled
    // immediately after deletion, so querying an old numeric id can correctly
    // resolve to a different newly-created entity.
    assert.equal((await game.entities.byTemplate(BASIC_CHEM)).length, 0);
    assert.equal((await game.entities.byTemplate(BASIC_JUICE)).length, 0);
    const player = (await game.info()).player;
    assert.equal(player.wielded_entity_id, null);
    assert.equal(player.right_hand_entity_id, null);
    assert.equal(
      (await game.ui.state()).cursor,
      null,
      "vaporization must clear the host-side cursor entity reference",
    );
  },
);

for (const advanced of ADVANCED_EXITS) {
  test(
    `earth ${advanced.name} exit shares VaporizeInventory cleanup`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "earth.mis",
        port: Number(process.env.SHOCK2_E2E_PORT ?? 8197),
      });
      await game.step({ frames: 5 });

      // Use one genuine world item as compact setup for each Advanced exit.
      // The primary Basic scenario above proves ordinary acquisition; these
      // cases isolate the three other authored trap compositions.
      const juice = await exactlyOne(game, BASIC_JUICE, "control Juice bottle");
      await game.player.give(juice.id);
      assert.equal((await game.player.inventory()).count, 1);

      await crossEarthTrainingTripwire(game, advanced.tripwire, advanced.destination);
      assert.equal(
        (await game.player.inventory()).count,
        0,
        `${advanced.name} exit must vaporize the same generic carried-item set`,
      );
      assert.equal(
        (await game.entities.byTemplate(BASIC_JUICE)).length,
        0,
        `${advanced.name} exit must destroy the carried entity, not only detach it`,
      );
    },
  );
}

test(
  "loading at the Earth Technical lobby return does not replay tripwire ENTER",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8198),
    });
    await game.step({ frames: 5 });

    // Carry a real mission item through the genuine Technical exit so this
    // setup also proves the authored cleanup + return teleport completed
    // before saving.
    const juice = await exactlyOne(game, BASIC_JUICE, "control Juice bottle");
    await game.player.give(juice.id);
    await crossEarthTrainingTripwire(
      game,
      TECHNICAL_EXIT_TRIPWIRE,
      TECHNICAL_EXIT_DESTINATION,
    );
    assert.equal(
      (await game.player.inventory()).count,
      0,
      "Technical exit should clean up training inventory before the save",
    );

    const lobbyPosition = await game.player.position();
    const technicalEntryDestination = await exactlyOne(
      game,
      TECHNICAL_ENTRY_DESTINATION,
      "Technical entry teleport destination",
    );
    const [entryX, , entryZ] = technicalEntryDestination.position;
    assert.ok(
      Math.hypot(lobbyPosition.x - entryX, lobbyPosition.z - entryZ) > 100,
      `genuine Technical exit should return to the lobby, got ${JSON.stringify(lobbyPosition)}`,
    );

    await game.step({ frames: 10 });
    const saveName = `earth_technical_lobby_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);

    // Loading is synchronous: first prove the serialized position itself was
    // restored, then step the newly-built physics world. Before #547 that
    // first step creates a fresh overlap with entry tripwire 314, replays its
    // ENTER, and sends the player back to destination 324.
    const loadedPosition = await game.player.position();
    assert.ok(
      Math.hypot(
        loadedPosition.x - lobbyPosition.x,
        loadedPosition.z - lobbyPosition.z,
      ) < 1,
      `load should initially restore the saved lobby position: saved=${JSON.stringify(lobbyPosition)} loaded=${JSON.stringify(loadedPosition)}`,
    );
    await game.step({ frames: 5 });
    const settledPosition = await game.player.position();
    assert.ok(
      Math.hypot(
        settledPosition.x - lobbyPosition.x,
        settledPosition.z - lobbyPosition.z,
      ) < 3,
      `initial overlap reconstruction must not replay ENTER: lobby=${JSON.stringify(lobbyPosition)} afterStep=${JSON.stringify(settledPosition)}`,
    );
    assert.equal(
      (await game.player.inventory()).count,
      0,
      "the cleaned inventory should remain empty after load",
    );

    // Suppression is only for the overlap reconstructed by load. Leave the
    // real entry sensor with bounded collision-valid movement, then walk back
    // into its authored center: a later genuine ENTER must still activate 324.
    const technicalEntry = await exactlyOne(
      game,
      TECHNICAL_ENTRY_TRIPWIRE,
      "Technical lobby entry tripwire",
    );
    const [sensorX, sensorY, sensorZ] = technicalEntry.position;
    const outside = {
      x: sensorX + 4,
      y: sensorY + 0.5,
      z: sensorZ,
    };
    const leave = await game.player.moveTo(outside);
    assert.ok(leave.moved, `player should leave entry sensor: ${JSON.stringify(leave)}`);
    await game.step({ frames: 8 });
    const outsidePosition = await game.player.position();
    assert.ok(
      Math.hypot(outsidePosition.x - sensorX, outsidePosition.z - sensorZ) > 3,
      `load-reconstructed overlap should end without activating the entry wiring: outside=${JSON.stringify(outside)} actual=${JSON.stringify(outsidePosition)}`,
    );
    const reenter = await game.player.moveTo({
      x: sensorX,
      y: sensorY + 0.5,
      z: sensorZ,
    });
    assert.ok(
      reenter.moved,
      `player should genuinely re-enter entry sensor: ${JSON.stringify(reenter)}`,
    );
    await game.step({ frames: 12 });
    const reenteredPosition = await game.player.position();
    assert.ok(
      Math.hypot(reenteredPosition.x - entryX, reenteredPosition.z - entryZ) < 3,
      `genuine leave/re-entry should still activate destination 324: destination=${JSON.stringify(technicalEntryDestination.position)} actual=${JSON.stringify(reenteredPosition)}`,
    );
  },
);
