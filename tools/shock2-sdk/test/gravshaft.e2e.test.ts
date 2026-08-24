import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for room gravity (P$RoomGrav) in earth.mis: the pair of
// gravshafts under the "TO STREET LEVEL" sign is the only route up from the
// subway spawn, so this traversal gates the whole game (#504).
//
// Data (all through the real flow: P$RoomGrav on the room object -> ROOM_DB
// sensor -> internal_room_trigger -> SwitchLink -> CoreRoom -> SetGravity on
// the intersecting player):
//   - The LEFT shaft (x=9.6)  is room "Up"   (obj 270, negative gravity: lifts)
//   - The RIGHT shaft (x=12.0) is room "Down" (obj 227, +20% gravity: descent)
//   - The shaft top             is room "Stop" (obj 228, 0% gravity)
//
// This pins the full chain end-to-end: standing at the bottom of the UP shaft
// must carry the player from the subway floor (y~2) to street level (y~21),
// while the DOWN shaft must NOT lift (that is the faithful behavior the #504
// playtest ran into when it tested x=12 and concluded the shafts were dead).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "earth.mis: the up gravshaft lifts the player to street level (#504)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 30 }); // settle

    // Enter the UP shaft at its base (teleporting into a room volume fires
    // the same sensor intersect as walking in).
    await game.player.teleport({ x: 9.6, y: 2.0, z: 14.4 });

    // The ride to street level takes ~6s at the shaft's lift rate; sample the
    // climb so a failure shows where it stalled.
    const samples: number[] = [];
    for (let i = 0; i < 10; i++) {
      await game.step({ frames: 60 });
      samples.push((await game.player.position()).y);
    }
    const peak = Math.max(...samples);
    const trace = samples.map((y) => y.toFixed(1)).join(" -> ");

    assert.ok(
      peak > 18,
      `the up gravshaft should lift the player from the subway (y~2) to ` +
        `street level (y>18); got y: ${trace} (no rise means the ` +
        `P$RoomGrav -> CoreRoom -> SetGravity -> character-controller chain ` +
        `is broken - see #504)`,
    );

    // Faithful counterpart: the RIGHT shaft is the DOWN shaft (+20% gravity).
    // Standing in it must NOT lift the player - a lift here would mean room
    // gravity data is being misapplied across rooms.
    await game.player.teleport({ x: 12.0, y: 2.0, z: 14.8 });
    await game.step({ frames: 360 });
    const down = await game.player.position();
    assert.ok(
      down.y < 5,
      `the down gravshaft must not lift the player (room "Down" has +20% ` +
        `gravity), got y=${down.y.toFixed(2)}`,
    );
  },
);
