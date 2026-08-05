import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable earth.mis mission-object ids for two authored Weapons Training display
// weapons. Runtime entity ids are rediscovered every launch.
const PISTOL_OBJ = 246;
const LASER_PISTOL_OBJ = 253;

// Regression for #777. Flat wielding a second weapon emitted
// `VirtualHandEffect::DropItem` for the one it displaced, so the previous
// weapon was re-physicalized at the player's feet with no HUD feedback and
// vanished from the inventory. The original returns it to the backpack; only an
// explicit drop takes a weapon out of the player's possession.
//
// This drives the world-pickup auto-wield, which applies the flat controller's
// effects directly and had no recovery behind it.
test(
  "earth: picking up a second weapon holsters the wielded one instead of ejecting it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8523),
    });
    await game.step({ frames: 30 });

    const [pistol] = await game.entities.byTemplate(PISTOL_OBJ);
    const [laserPistol] = await game.entities.byTemplate(LASER_PISTOL_OBJ);
    assert.ok(pistol?.name === "Pistol", "expected earth Weapons Training pistol 246");
    assert.ok(
      laserPistol?.name === "Laser Pistol",
      "expected earth Weapons Training laser pistol 253",
    );

    await earthWorldUse(game, pistol);
    assert.equal(
      (await game.info()).player.wielded_entity_id,
      pistol.id,
      "world-use should auto-wield the first weapon",
    );

    // The swap: a second world weapon displaces the wielded pistol.
    await earthWorldUse(game, laserPistol);
    await game.step({ frames: 2 });

    assert.equal(
      (await game.info()).player.wielded_entity_id,
      laserPistol.id,
      "world-use of a second weapon should wield it",
    );

    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.entity_id === laserPistol.id)?.location,
      "left_hand",
      `the laser pistol should be wielded, got ${JSON.stringify(inventory.items)}`,
    );
    // Pre-fix this was `undefined`: the displaced pistol left the player's
    // possession entirely, ejected onto the floor.
    assert.equal(
      inventory.items.find((item) => item.entity_id === pistol.id)?.location,
      "inventory",
      `the displaced pistol must return to the backpack, got ${JSON.stringify(inventory.items)}`,
    );

    // ...and it must not be an orphaned world object at the player's feet.
    assert.equal(
      (await game.physics.bodies({ entityId: pistol.id })).bodies.length,
      0,
      "a holstered weapon must have no world physics body",
    );

    // Explicit drop still works: lift the holstered pistol out of the use-mode
    // strip and throw it at the bare 3D view - the original's only way to put a
    // carried weapon back into the world.
    const clickAt = async (screen: [number, number]) => {
      await game.input.set("pointer.position", screen);
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 1);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
    };
    const clickElement = (element: UiElement) => {
      const [x, y, width, height] = element.screen_rect;
      return clickAt([x + width / 2, y + height / 2]);
    };

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const ui = await game.ui.state();
    assert.equal(ui.mode, "use");
    const slot = ui.strip?.elements.find(
      (element) => element.kind === "button" && element.label === "Pistol",
    );
    assert.ok(slot, `the holstered pistol should be in the strip, got ${JSON.stringify(ui.strip)}`);

    await clickElement(slot); // lift onto the cursor
    assert.equal(
      (await game.ui.state()).cursor?.entity_id,
      pistol.id,
      "the pistol should be on the cursor",
    );
    await clickAt([0.5, 0.6]); // bare 3D view: throw
    await game.step({ frames: 30 });

    assert.equal(
      (await game.player.inventory()).items.find((item) => item.entity_id === pistol.id)
        ?.location,
      undefined,
      "an explicit throw should still remove the weapon from the inventory",
    );
    assert.ok(
      (await game.physics.bodies({ entityId: pistol.id })).bodies.length > 0,
      "an explicitly thrown weapon should regain a world physics body",
    );
  },
);
