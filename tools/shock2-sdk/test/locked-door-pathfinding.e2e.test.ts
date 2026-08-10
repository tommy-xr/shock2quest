import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Medsci's real Sci Med door (mission object 72) sits on the short route
// between this OG-Pipe and the target point north of the threshold; a much
// longer route around it remains available. Runtime ids vary, so both objects
// are discovered through their stable mission-object template ids.
const SCI_MED_DOOR = 72;
const SOUTH_OG_PIPE = 596;
const NORTH_OF_DOOR = { x: 23.3, y: 0.5, z: 20.0 };
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "medsci1: locking a real door removes it from a pursuing AI's route",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8148),
    });
    await game.step({ frames: 10 });

    const [door] = await game.entities.byTemplate(SCI_MED_DOOR);
    const [pursuer] = await game.entities.byTemplate(SOUTH_OG_PIPE);
    assert.ok(door, `expected Sci Med door object ${SCI_MED_DOOR}`);
    assert.ok(pursuer, `expected living OG-Pipe object ${SOUTH_OG_PIPE}`);

    await game.player.teleport(NORTH_OF_DOOR);
    await game.input.trigger("DebugForceChase");
    await game.step({ frames: 120 });

    const unlocked = (await game.pathfinding.aiPaths()).find(
      (entry) => entry.entity_id === pursuer.id,
    );
    assert.equal(unlocked?.outcome, "Full", JSON.stringify(unlocked));
    assert.ok(
      unlocked!.waypoints.some(
        ([x, _y, z]) => Math.hypot(x - door.position[0], z - door.position[2]) < 4.0,
      ),
      `unlocked route must pass the real door at ${JSON.stringify(door.position)}: ${JSON.stringify(unlocked)}`,
    );

    // Set Dark's live P$Locked property on the concrete door. The next mission
    // update synchronizes that authored object id through GlobalTemplateIdMap
    // into PathfindingService's door state before the AI retries its pursuit.
    await game.entities.sendMessage(door.id, { type: "SetLocked", locked: true });
    // An unrelated pursuing AI can already have opened the shared doorway.
    // Drive the real StdDoor close edge as mission logic would; once closed,
    // the lock makes its BELOW_DOOR cells impassable.
    await game.entities.sendMessage(door.id, { type: "TurnOff" });
    await game.step({ frames: 180 });

    const lockedDoor = await game.entities.detail(door.id);
    assert.equal(
      lockedDoor.properties.find((property) => property.name === "Locked")?.value,
      "true",
    );
    assert.equal(
      lockedDoor.properties.find((property) => property.name === "DoorBlocksPathfinding")?.value,
      "true",
    );

    // The pursuer may still be following the route it obtained before the
    // lock. Restart its pursuit so the assertion observes a fresh query made
    // against the synchronized live door state.
    await game.input.trigger("DebugCalmAll");
    await game.step({ frames: 30 });
    await game.input.trigger("DebugForceChase");
    await game.step({ frames: 120 });

    const locked = (await game.pathfinding.aiPaths()).find(
      (entry) => entry.entity_id === pursuer.id,
    );
    assert.ok(locked, "pursuer should publish its post-lock route attempt");
    assert.equal(locked.outcome, "Full", JSON.stringify(locked));
    assert.ok(
      locked.waypoints.length > unlocked.waypoints.length,
      `locked route must take the longer way around: ${JSON.stringify({ unlocked, locked })}`,
    );
    assert.ok(
      locked.waypoints.every(
        ([x, _y, z]) => Math.hypot(x - door.position[0], z - door.position[2]) >= 4.0,
      ),
      `post-lock route must not cross the door: ${JSON.stringify(locked)}`,
    );
  },
);
