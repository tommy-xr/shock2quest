import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { vrClimbPull } from "./helpers/vr-climb.js";
import { teleportVerified } from "./helpers/teleport.js";

// `GET /v1/physics/grip` answers "could a hand hold on here?" against the
// debug_ladder stations (see shock2vr/src/scenes/debug_ladder.rs). The two
// classes must be told apart on the SHIPPED geometry: an authored ladder face
// is a Ladder, a block top above the feet is a Ledge, and a plain wall and
// the floor are nothing. A hand above a thin ladder cap can hook its side.
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
    // The scene's ladders inherit the Ladders template's mask 27 - the four
    // vertical sides, no caps.
    name: "hook an authored side from above the ladder cap",
    point: [-6.9, 6.45, 0],
    expected: "ladder",
  },
  // The block the ladder leans on: near face x = -7, top y = 6.
  { name: "ledge block lip", point: [-7.2, 6.05, 0], expected: "ledge" },
  // The ladderless mantle block: top y = 3.
  { name: "mantle block lip", point: [-7.2, 3.05, 24], expected: "ledge" },
  { name: "mantle lip from below", point: [-6.95, 2.99, 24], expected: "ledge" },
  { name: "mantle corner diagonal", point: [-6.9, 3.05, 24], expected: "ledge" },
  { name: "mantle lip too far below", point: [-6.95, 2.5, 24], expected: null },
  { name: "mantle lip too far away", point: [-6.5, 2.99, 24], expected: null },
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

test(
  "medsci1: a shipped ladder's broad face is a Ladder grip",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 30 });

    // Discover by name - runtime entity ids are not stable across launches.
    const { entities } = await game.entities.list({ filter: "Rick Ladder 16" });
    assert.ok(entities.length > 0, "medsci1 should place Rick Ladder 16s");

    // These instances override the template's 27 with 54 (the broad faces
    // plus both caps). Either mask must grip the broad face, so probe a ring
    // around each ladder's own origin and require a Ladder somewhere on it.
    const RING = 0.12;
    const kinds = new Set<string>();
    for (const ladder of entities) {
      for (const [dx, dz] of [
        [RING, 0],
        [-RING, 0],
        [0, RING],
        [0, -RING],
      ]) {
        const { grip } = await game.physics.grip([
          ladder.position[0] + dx,
          ladder.position[1],
          ladder.position[2] + dz,
        ]);
        if (grip?.kind) kinds.add(grip.kind);
      }
    }
    assert.ok(
      kinds.has("ladder"),
      `expected a Ladder grip on a shipped ladder, saw ${[...kinds]}`,
    );
  },
);


test("debug_ladder (VR): a hand below the mantle corner can hold and pull", {
  skip: !e2eEnabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  await teleportVerified(game, { x: -6.35, y: 1.5, z: 24 });
  await game.step({ frames: 30 });
  const pull = await vrClimbPull(game, { grabAt: [-6.95, 2.99, 24], pull: [0, -0.3, 0], frames: 30 });
  const climb = (await game.info()).player.climb;
  assert.equal(climb.grips[0]?.kind, "ledge");
  assert.ok(pull.after[1] - pull.before[1] > 0.2, "corner grip must move the body");
});
