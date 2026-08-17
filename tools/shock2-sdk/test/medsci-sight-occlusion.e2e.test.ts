import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary, Vec3 } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8560);

// Stable MedSci2 mission-object identities. Runtime entity ids are assigned
// afresh on every launch and are deliberately discovered below.
const SOUTH_CATWALK_MONKEY = 437;
const SOUTH_CATWALK_DOOR = 76;
const SOUTH_CATWALK_WINDOW = 420;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

async function soleMissionObject(
  game: GameServer,
  name: string,
  missionObject: number,
): Promise<EntitySummary> {
  const entity = (await game.entities.list({ filter: name, limit: 100 })).entities.find(
    (candidate) => candidate.template_id === missionObject,
  );
  assert.ok(entity, `MedSci2 mission object ${missionObject} (${name}) should exist`);
  return entity;
}

async function waitForVisible(
  game: GameServer,
  entityId: number,
  expected: boolean,
  frames = 60,
): Promise<EntityDetailResult> {
  for (let frame = 0; frame < frames; frame += 5) {
    await game.step({ frames: 5 });
    const detail = await game.entities.detail(entityId);
    if (property(detail, "AITargetVisible") === String(expected)) return detail;
  }
  const detail = await game.entities.detail(entityId);
  assert.equal(
    property(detail, "AITargetVisible"),
    String(expected),
    `entity ${entityId} did not publish visibility=${expected}`,
  );
  return detail;
}

test(
  "MedSci2 entity door freezes a hidden monster target and opening clears it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const monkey = await soleMissionObject(
      game,
      "Blue Monkey",
      SOUTH_CATWALK_MONKEY,
    );
    const door = await soleMissionObject(
      game,
      "Sci Med Door",
      SOUTH_CATWALK_DOOR,
    );

    const northOfDoor: Vec3 = [17.2, -0.356, -107.0];
    const southOfDoor: Vec3 = [17.2, -0.356, -116.0];
    const terrainRay = await game.raycast({
      start: northOfDoor,
      end: southOfDoor,
      collision_groups: ["world"],
      ignore_sensors: true,
    });
    assert.equal(terrainRay.hit, false, "terrain must leave the doorway open");

    const projectileRay = await game.raycast({
      start: northOfDoor,
      end: southOfDoor,
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.equal(projectileRay.entity_id, door.id);

    // First let the authored monkey acquire the player north of the closed
    // door. Moving due south keeps the target in the same FOV; only the door
    // changes the answer.
    await game.player.teleport({ x: 17.2, y: 0.5, z: -107.0 });
    await game.entities.sendMessage(monkey.id, {
      type: "SetAlertness",
      level: "High",
    });
    const exposed = await waitForVisible(game, monkey.id, true);
    const exposedLastKnown = property(exposed, "AILastKnown");
    assert.ok(exposedLastKnown, "exposure should publish a last-known target");

    await game.player.teleport({ x: 17.2, y: 0.5, z: -116.0 });
    const hidden = await waitForVisible(game, monkey.id, false);
    const hiddenLastKnown = property(hidden, "AILastKnown");
    assert.ok(hiddenLastKnown, "losing sight should retain a last-known target");

    // A second hidden coordinate must not leak through the cover. One frame
    // isolates the awareness refresh from subsequent asynchronous pursuit.
    await game.player.teleport({ x: 17.2, y: 0.5, z: -117.0 });
    await game.step({ frames: 1 });
    const movedBehindCover = await game.entities.detail(monkey.id);
    assert.equal(property(movedBehindCover, "AITargetVisible"), "false");
    assert.equal(property(movedBehindCover, "AILastKnown"), hiddenLastKnown);

    // The same authored door slides fully out of the ray when frobbed. The
    // production projectile mask remains unchanged: closed blocks, open does
    // not.
    await game.entities.sendMessage(door.id, { type: "TurnOn" });
    await game.step({ frames: 90 });
    const openRay = await game.raycast({
      start: northOfDoor,
      end: southOfDoor,
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.notEqual(openRay.entity_id, door.id, "open door must clear its doorway");

    // Return to the unobstructed control point after proving that the authored
    // open pose cleared the same ray; ordinary awareness must still resume.
    await game.player.teleport({ x: 17.2, y: 0.5, z: -107.0 });
    await game.entities.sendMessage(monkey.id, {
      type: "SetAlertness",
      level: "High",
    });
    const reacquired = await waitForVisible(game, monkey.id, true, 180);
    assert.notEqual(property(reacquired, "AILastKnown"), hiddenLastKnown);
  },
);

test(
  "MedSci2 transparent window preserves sight but still blocks projectiles",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const window = await soleMissionObject(
      game,
      "UBWindow",
      SOUTH_CATWALK_WINDOW,
    );

    // Spawn the ordinary debug hybrid north of the authored glass while the
    // player stands on the same sight line. Its initial facing therefore
    // remains valid when the player moves to the far side of the pane.
    const known = new Set(
      (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map(
        (entity) => entity.id,
      ),
    );
    await game.player.teleport({ x: 12.8, y: 0.5, z: -110.5 });
    await game.input.set("head.look", [90, 0]);
    await game.step({ frames: 15 });
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 5 });
    const hybrid = (
      await game.entities.list({ filter: "OG-Pipe", limit: 50 })
    ).entities.find((entity) => !known.has(entity.id));
    assert.ok(hybrid, "SpawnDebugMonster should create the transparent-sight observer");

    await game.player.teleport({ x: 12.8, y: 0.5, z: -116.0 });
    await game.entities.sendMessage(hybrid.id, {
      type: "SetAlertness",
      level: "High",
    });
    const visibleThroughGlass = await waitForVisible(game, hybrid.id, true, 120);
    assert.equal(property(visibleThroughGlass, "AITargetVisible"), "true");

    const projectileRay = await game.raycast({
      start: [12.8, 0.6, -107.0],
      end: [12.8, 0.6, -116.0],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.equal(projectileRay.entity_id, window.id);
  },
);
