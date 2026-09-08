import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Hydro3 mission object 382 is authored as a loose audio log resting on the
// sloped face of Computer Console 208. The console's six-submodel OBB physics
// is currently represented by one coarse cuboid, which encloses the disc and
// wins every production hand ray before LogDiscScript can receive Frob (#1101).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const COMPUTER_CONSOLE = 208;
const GLORY_AUDIO_LOG = 382;

test(
  "Quest VR can collect Hydro3 audio log 382 from console 208 without weakening its physical hull",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro3.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8645),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const [log] = await game.entities.byTemplate(GLORY_AUDIO_LOG);
    const [console] = await game.entities.byTemplate(COMPUTER_CONSOLE);
    assert.ok(log, "hydro3 must contain loose audio log mission object 382");
    assert.ok(console, "hydro3 must contain Computer Console mission object 208");

    const [consoleBody] = (await game.physics.bodies({ entityId: console.id })).bodies;
    assert.ok(consoleBody, "console 208 must retain its authored physical body");
    assert.equal(consoleBody.body_type, "kinematic");
    assert.equal(consoleBody.is_sensor, false);
    assert.ok(consoleBody.collision_groups.includes("entity"));

    await teleportVerified(game, { x: 17.4825, y: 3.244, z: 3.4797 });
    const aim = await aimVrHandAt(game, log.position, 0.7);

    // The ordinary physics query must keep seeing the authored hull. The VR
    // interaction path alone resolves the console against its visible mesh,
    // so this also guards against a broad "ray through entities" workaround.
    const hit = await game.raycast({
      start: aim.start,
      end: aim.target,
      collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
      max_distance: 1,
    });
    assert.equal(hit.entity_id, console.id, "the console must keep its physical ray hull");

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    assert.equal(
      (await game.physics.bodies({ entityId: log.id })).bodies.length,
      0,
      "LogDiscScript must remove the loose disc from the world",
    );
    assert.deepEqual((await game.info()).player.collected_logs, [
      { deck: 3, log: 12, read: false },
    ]);
  },
);
