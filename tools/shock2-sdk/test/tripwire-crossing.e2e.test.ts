import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Position } from "../src/index.js";

// End-to-end test for sensor ENTER/EXIT along a validated player move
// (POST /v1/player/move, game.player.moveTo) - GitHub #654.
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: the validated move covered its whole hop inside one atomic
// commit while sensors were polled once per stepped frame, so a hop that
// PASSED THROUGH a trigger volume between two frames fired nothing at all. The
// "crosses" case below therefore failed (note_6_1 stayed "unknown") before the
// swept sensor poll was added. Automated playtests navigate with this call, so
// a skipped trigger reads back as a false "this trigger is broken" finding -
// and can walk past a real progression wire unnoticed.
//
// A hop that ENDS INSIDE a volume always worked (the next frame's poll catches
// it); that case is asserted too, so the fix cannot regress it. Walking the
// same volume with the thumbstick is the reference behaviour both hops must
// match. (The no-double-fire invariant is asserted on the raw event stream by
// `validated_move_fires_sensors_crossed_along_the_hop` in physics/mod.rs.)
//
// command1's arrival Tripwire (mission object 2308) feeds an EmailTrap that
// grants the Note_6_1 objective. Entities are discovered by stable mission
// object id / name - runtime entity ids differ every launch.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8222);

const TRIPWIRE_OBJECT_ID = 2308;
const OBJECTIVE = "note_6_1";
// The wire is a 1.6-deep box centred on its position, so anything beyond 0.8
// (plus the player's ~0.32 radius) from the centre is clear of it. The two
// offsets keep the crossing hop (APPROACH + BEYOND = 4.0) inside the 5.0-unit
// clamp on a validated move, so a clamp can never be mistaken for a block.
const WIRE_HALF_DEPTH = 0.8;
const APPROACH = 2.4;
const BEYOND = 1.6;

interface Arrival {
  wire: Position;
  start: Position;
}

/**
 * Open the arrival lift doors that stand between the command1 spawn and the
 * tripwire, then park the player one short hop in front of the wire (outside
 * it, so any crossing below is a genuine edge).
 */
async function arriveAtTripwire(game: GameServer): Promise<Arrival> {
  await game.step({ frames: 5 });

  const wires = await game.entities.byTemplate(TRIPWIRE_OBJECT_ID);
  assert.equal(
    wires.length,
    1,
    `expected the command1 arrival tripwire (object ${TRIPWIRE_OBJECT_ID}), ` +
      `got ${JSON.stringify(wires.map((w) => w.name))}`,
  );
  const [wx, wy, wz] = wires[0].position;

  // The lift doors are closed on arrival and block the corridor to the wire.
  const doors = await game.entities.list({ filter: "Elevator Door" });
  assert.ok(
    doors.entities.length > 0,
    "expected the command1 arrival lift doors between the spawn and the wire",
  );
  for (const door of doors.entities) {
    await game.entities.sendMessage(door.id, { type: "TurnOn" });
  }
  await game.step({ frames: 180 });

  const spawn = await game.player.position();
  const start: Position = { x: wx, y: spawn.y, z: wz - APPROACH };
  await game.player.teleport(start);
  await game.step({ frames: 10 });

  const settled = await game.player.position();
  assert.ok(
    Math.abs(settled.z - wz) > WIRE_HALF_DEPTH,
    `the approach must start outside the wire, got z=${settled.z} vs wire z=${wz}`,
  );
  assert.equal(
    await game.quests.get(OBJECTIVE),
    "unknown",
    "the arrival objective must not be granted before the wire is crossed",
  );

  return { wire: { x: wx, y: wy, z: wz }, start: settled };
}

test(
  "command1: a validated move that passes THROUGH the arrival tripwire fires it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: basePort,
    });
    const { wire, start } = await arriveAtTripwire(game);

    const move = await game.player.moveTo({
      x: wire.x,
      y: start.y,
      z: wire.z + BEYOND,
    });
    await game.step({ frames: 30 });

    assert.equal(move.blocked, false, "the corridor through the wire should be clear");
    assert.ok(
      move.new_position[2] - wire.z > WIRE_HALF_DEPTH,
      `the hop must end past the wire (a pass-through, not a landing inside), ` +
        `ended at z=${move.new_position[2]} vs wire z=${wire.z}`,
    );
    assert.equal(
      await game.quests.get(OBJECTIVE),
      "incomplete",
      "crossing the wire with a validated move must fire it (#654)",
    );
  },
);

test(
  "command1: a validated move that ENDS INSIDE the arrival tripwire fires it once",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: basePort + 1,
    });
    const { wire, start } = await arriveAtTripwire(game);

    const move = await game.player.moveTo({ x: wire.x, y: start.y, z: wire.z });
    await game.step({ frames: 30 });

    assert.ok(
      Math.abs(move.new_position[2] - wire.z) < WIRE_HALF_DEPTH,
      `the hop must stop inside the wire, ended at z=${move.new_position[2]} ` +
        `vs wire z=${wire.z}`,
    );
    assert.equal(
      await game.quests.get(OBJECTIVE),
      "incomplete",
      "a hop ending inside the wire must fire it",
    );

    const emails = (await game.audio.recent()).sounds.filter((sound) =>
      sound.tags.some(([key, value]) => key === "kind" && value === "email"),
    );
    assert.equal(
      emails.length,
      1,
      `the email should play exactly once, got ${JSON.stringify(emails)}`,
    );
  },
);

test(
  "command1: walking the arrival tripwire with the thumbstick fires it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: basePort + 2,
    });
    const { wire, start } = await arriveAtTripwire(game);

    await game.input.lookAtWorldPoint([wire.x, start.y, wire.z + APPROACH]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 10 });

    const end = await game.player.position();
    assert.ok(
      end.z - wire.z > WIRE_HALF_DEPTH,
      `walking forward should carry the player past the wire, got z=${end.z} ` +
        `vs wire z=${wire.z}`,
    );
    assert.equal(
      await game.quests.get(OBJECTIVE),
      "incomplete",
      "walking through the wire must fire it (the reference behaviour)",
    );
  },
);
