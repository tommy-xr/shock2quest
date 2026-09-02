import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";

// `GET /v1/physics/grip` answers "could a hand hold on here?" against the
// debug_ladder stations (see shock2vr/src/scenes/debug_ladder.rs). The two
// classes must be told apart on the SHIPPED geometry: an authored ladder face
// is a Ladder, a block top above the feet is a Ledge, and a ladder's top cap,
// a plain wall and the floor are nothing at all.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

type Probe = {
  name: string;
  point: Vec3;
  expected: "ladder" | "ledge" | null;
};

const PROBES: Probe[] = [
  // The ledge station's 16' ladder: model bounds put its near face at
  // x ≈ -6.83, its top cap at y = 6.4.
  { name: "ledge ladder face", point: [-6.8, 3.0, 0], expected: "ladder" },
  {
    name: "ledge ladder top cap (mask 27 excludes it)",
    point: [-6.9, 6.45, 0],
    expected: null,
  },
  // The block the ladder leans on: near face x = -7, top y = 6.
  { name: "ledge block lip", point: [-7.2, 6.05, 0], expected: "ledge" },
  // The ladderless mantle block: top y = 3.
  { name: "mantle block lip", point: [-7.2, 3.05, 24], expected: "ledge" },
  { name: "plain wall face", point: [-6.9, 3.0, -16], expected: null },
  { name: "floor at the player's feet", point: [0, 0.05, 0], expected: null },
];

test(
  "debug_ladder: a hand finds ladder faces and ledges, and nothing else",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    // Let the player settle onto the floor - the ledge test is relative to
    // their feet.
    await game.step({ frames: 60 });
    // The reported position is the capsule CENTER; on this floor that is
    // ~1.24, i.e. feet at y ~= 0 - which is what the ledge test is relative to.
    const player = await game.player.position();
    assert.ok(
      Math.abs(player.y - 1.24) < 0.2,
      `expected the player standing on the floor, got y=${player.y}`,
    );

    for (const probe of PROBES) {
      const { grip } = await game.physics.grip(probe.point);
      assert.equal(
        grip?.kind ?? null,
        probe.expected,
        `${probe.name} at ${probe.point.join(",")}: got ${JSON.stringify(grip)}`,
      );
    }

    // A ladder grip identifies its entity and faces the player (+X here);
    // a ledge grip faces up.
    const ladder = (await game.physics.grip([-6.8, 3.0, 0])).grip!;
    assert.ok(ladder.entity_id !== null, "a ladder grip names its entity");
    assert.ok(
      ladder.normal[0] > 0.9,
      `expected a +X ladder face, got ${ladder.normal.join(",")}`,
    );
    const ledge = (await game.physics.grip([-7.2, 6.05, 0])).grip!;
    assert.ok(
      ledge.normal[1] > 0.9,
      `expected an up-facing ledge, got ${ledge.normal.join(",")}`,
    );

    // The ledge class is feet-relative: the same block top is not a hold for
    // a player already standing on it.
    const standingOnIt = await game.physics.grip([-7.2, 6.05, 0], {
      feetY: 6.0,
    });
    assert.equal(standingOnIt.grip, null);
  },
);
