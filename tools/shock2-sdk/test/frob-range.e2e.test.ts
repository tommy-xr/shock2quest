import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable hydro2 mission-object id for the bulkhead button leading to hydro1.
// Runtime entity ids are rediscovered on every launch.
const HYDRO1_BULKHEAD_BUTTON = 998;

async function pulseUse(game: GameServer): Promise<void> {
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 30 });
}

test(
  "hydro2: a visible bulkhead button only frobs within the retail reach",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8237),
    });
    await game.step({ frames: 5 });

    const [button] = await game.entities.byTemplate(HYDRO1_BULKHEAD_BUTTON);
    assert.ok(button, "expected hydro2 Bulk_On_Button object 998");

    // Retail GAMEPARAM authors Frob Dist = 50, interpreted by PickSetFocus as
    // squared SS2 units. In this engine's /2.5 world scale that is
    // sqrt(50) / 2.5 = 2.828 world units. Stage along the button's clear face
    // beyond that reach, while keeping it visible and centered under the real
    // flat crosshair.
    await game.player.teleport({
      x: button.position[0] - 4.5,
      y: button.position[1],
      z: button.position[2],
    });
    await game.step({ frames: 5 });
    const farAim = await game.player.aimAt(button, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(farAim.target_confirmed, true, "far button must be unobstructed");
    const farPlayer = await game.info();
    const farDistance = Math.hypot(
      farAim.world_point[0] - farPlayer.player.position[0],
      farAim.world_point[1] -
        (farPlayer.player.position[1] + farPlayer.player.camera_offset[1]),
      farAim.world_point[2] - farPlayer.player.position[2],
    );
    assert.ok(
      farDistance > 2.83,
      `fixture must put the visible button beyond retail frob reach, got ${farDistance}`,
    );

    await pulseUse(game);
    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "hydro2.mis",
      "ordinary use must not trigger a visible but out-of-reach level transition",
    );

    // The same production aim/use path must remain functional at arm's reach.
    await game.player.teleport({
      x: button.position[0] - 2.5,
      y: button.position[1],
      z: button.position[2],
    });
    await game.step({ frames: 5 });
    const nearAim = await game.player.aimAt(button, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(nearAim.target_confirmed, true, "near button must be unobstructed");
    const nearPlayer = await game.info();
    const nearDistance = Math.hypot(
      nearAim.world_point[0] - nearPlayer.player.position[0],
      nearAim.world_point[1] -
        (nearPlayer.player.position[1] + nearPlayer.player.camera_offset[1]),
      nearAim.world_point[2] - nearPlayer.player.position[2],
    );
    assert.ok(
      nearDistance < 2.83,
      `fixture must put the visible button within retail frob reach, got ${nearDistance}`,
    );

    await pulseUse(game);
    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "hydro1.mis",
      "the same bulkhead button should transition when used within reach",
    );
  },
);
