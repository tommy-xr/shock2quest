import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable Hydro3 mission object/template ids, never runtime entity ids.
const NORTH_ARACHNID = 372;
const SOUTH_ARACHNID = 373;

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find(
    (candidate) => candidate.name === "HitPoints",
  );
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

function planarDistance(
  player: [number, number, number],
  entity: EntitySummary,
): number {
  return Math.hypot(
    player[0] - entity.position[0],
    player[2] - entity.position[2],
  );
}

test(
  "Hydro3 Baby Arachnids can be pushed aside and hit by normal weapons",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro3.mis",
    });
    await game.step({ frames: 10 });

    // Setup: stand midway between Hydro3's two authored Baby Arachnids and
    // alert them. Their production chase brings the lightweight live actor
    // capsules to opposite sides of the player, reproducing #819 without a
    // debug-spawned stand-in or a hardcoded runtime entity id.
    await game.player.teleport({ x: 50.55, y: -3.16, z: -26.6 });
    await game.step({ frames: 5 });
    await game.input.trigger("DebugAlertAll");

    let north: EntitySummary | undefined;
    let south: EntitySummary | undefined;
    let bracketed = false;
    for (let elapsed = 0; elapsed < 720; elapsed += 5) {
      await game.step({ frames: 5 });
      [north] = await game.entities.byTemplate(NORTH_ARACHNID);
      [south] = await game.entities.byTemplate(SOUTH_ARACHNID);
      assert.ok(north && south, "Hydro3 should retain both authored Baby Arachnids");
      const player = (await game.info()).player.position;
      const northVector = [
        north.position[0] - player[0],
        north.position[2] - player[2],
      ];
      const southVector = [
        south.position[0] - player[0],
        south.position[2] - player[2],
      ];
      const dot = northVector[0] * southVector[0] + northVector[1] * southVector[1];
      bracketed =
        planarDistance(player, north) < 1.2 &&
        planarDistance(player, south) < 1.2 &&
        dot < 0;
      if (bracketed) break;
    }
    assert.ok(bracketed && north && south, "the two real arachnids should bracket the player");

    // Freeze their chase only after the real AI has established contact. This
    // keeps the wedge deterministic while retaining the production creature
    // bodies/colliders that caused the saved-game trap.
    await game.entities.sendMessage(north.id, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.entities.sendMessage(south.id, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 2 });
    const bracketInfo = await game.info();

    // Negative-first: before player/actor shove transfer, walking directly
    // into either dynamic capsule stops at contact (and the issue's saved
    // frontier measured <0.0001u in every available heading).
    const trapped = (await game.info()).player.position;
    const adjacentNorth = await game.entities.detail(north.id);
    await game.input.lookAtWorldPoint([
      adjacentNorth.position[0],
      trapped[1] + PLAYER_EYE_HEIGHT_WORLD,
      adjacentNorth.position[2],
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const escaped = (await game.info()).player.position;
    const escapeDistance = Math.hypot(
      escaped[0] - trapped[0],
      escaped[2] - trapped[2],
    );
    assert.ok(
      escapeDistance > 1,
      `ordinary locomotion should push past live arachnids, moved ${escapeDistance}; ` +
        `before=${JSON.stringify(trapped)} after=${JSON.stringify(escaped)} ` +
        `hp=${bracketInfo.player.hit_points}`,
    );

    // Provisioning only supplies the loadout; aiming, firing, projectile
    // raycasts, proxy resolution, melee reach, and damage all use production
    // paths. This independently guards the issue's weapon-immunity symptom.
    await game.input.trigger("SpawnDebugItem");
    await game.step({ frames: 5 });
    const pistolBefore = hitPoints(await game.entities.detail(north.id));
    await game.player.aimAt(north, { hitbox: "torso", visibility: "required" });
    await game.step({ frames: 3 });
    await fireOnce(game);
    await game.step({ frames: 5 });
    assert.ok(
      hitPoints(await game.entities.detail(north.id)) < pistolBefore,
      "an aimed standard-pistol shot should damage the Baby Arachnid",
    );

    await game.player.spawnItem(-928); // Wrench
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 3 });
    const liveSouth = await game.entities.detail(south.id);
    await game.player.teleport({
      x: liveSouth.position[0] + 1.08,
      y: escaped[1],
      z: liveSouth.position[2],
    });
    await game.step({ frames: 5 });
    const wrenchBefore = hitPoints(await game.entities.detail(south.id));
    await game.player.aimAt(south, { hitbox: "torso", visibility: "required" });
    await game.step({ frames: 3 });
    await fireOnce(game);
    // leftswing's authored MF_TRIGGER1 is frame 20 at 30 fps (sim frame 41).
    await game.step({ frames: 45 });
    assert.ok(
      hitPoints(await game.entities.detail(south.id)) < wrenchBefore,
      "an aimed wrench swing should damage the Baby Arachnid",
    );
  },
);
