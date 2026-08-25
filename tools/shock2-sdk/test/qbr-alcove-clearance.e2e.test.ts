import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { carriedNaniteTotal } from "./helpers/earth-replicator.js";

// Regression coverage for #801. The hydro2 Resurrection Station is a walk-in
// fixture: the player stands inside its casing on the reconstruction pad. Its
// template carries no P$PhysType anywhere in its inheritance chain, so Dark
// never gives it a physics model - it is decoration in front of the brushwork.
// This engine still builds a model-bounds box for it so it stays frobbable,
// and while that box was solid to the player it filled the whole alcove
// (2.2 x 4.0 x 2.5 world units). The character controller has no
// depenetration pass, so a capsule overlapping one of its faces resolved every
// direction to a zero-length move and no input could recover.
//
// Negative-first: on main both the raw-input and bounded-move assertions below
// report 0 at the wedge coordinates from the issue.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable mission-object ids (`template_id`), not runtime entity ids.
const RESURRECTION_STATION = 423;
const RESURRECTION_BUTTON = 858;
const RESURRECTION_TARGET = 570;
const RESURRECTION_COST = 10;
const RESPAWN_DELAY_FRAMES = 5 * 60;

// The wedge reported in #801, and the reconstruction pad one metre away. The
// pad is NOT an outside-the-box control - it is the box's own x/z centre. It
// moved fine even on main because a capsule buried deep inside a convex box
// finds no contact at all; only poses straddling a face freeze. Asserting both
// keeps the fix honest in either regime.
const WEDGE = { x: 43.72, y: 3.69, z: 31.32 };
const PAD = { x: 44.8, y: 3.7, z: 32.4 };

function planarDistance(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

function distance(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/**
 * Drive real locomotion input across `headings` compass directions and report
 * the furthest the player got from where they started. This is the production
 * path (thumbstick -> character controller), not the debug mover.
 */
async function bestWalkDistance(
  game: GameServer,
  from: { x: number; y: number; z: number },
  headings = 8,
): Promise<number> {
  let best = 0;
  for (let i = 0; i < headings; i += 1) {
    const angle = (2 * Math.PI * i) / headings;
    await game.player.teleport(from);
    await game.step({ frames: 5 });
    const start = (await game.info()).player.position;
    await game.input.set("right_hand.thumbstick", [
      Math.sin(angle),
      Math.cos(angle),
    ]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const end = (await game.info()).player.position;
    best = Math.max(best, planarDistance(start, end));
  }
  return best;
}

test(
  "hydro2: the QBR alcove floor is walkable and its casing stays frobbable",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 5 });

    // The casing keeps its collider - it must stay selectable and raycastable,
    // it just must not block the player capsule.
    const [station] = await game.entities.byTemplate(RESURRECTION_STATION);
    assert.ok(station, "hydro2 should contain its authored Resurrection Station");
    const { bodies } = await game.physics.bodies({ entityId: station.id });
    assert.equal(
      bodies.length,
      1,
      "the Resurrection Station must keep its frob collider",
    );
    // ...and that collider must still answer the mask the frob/selection ray
    // uses, or the machine would become impossible to interact with. Cast from
    // just outside its -x face (past the Regen_Hologram, which is a typeless
    // frobbable too and would otherwise be hit first) into the casing.
    const hit = await game.raycast({
      start: [43.4, station.position[1], station.position[2]],
      end: [station.position[0], station.position[1], station.position[2]],
      collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
      ignore_sensors: true,
    });
    assert.equal(
      hit.entity_id,
      station.id,
      `the selection ray must still hit the Resurrection Station: ${JSON.stringify(hit)}`,
    );

    const padWalk = await bestWalkDistance(game, PAD);
    assert.ok(
      padWalk > 1,
      `the reconstruction pad must stay walkable: moved ${padWalk}`,
    );

    const wedgeWalk = await bestWalkDistance(game, WEDGE);
    assert.ok(
      wedgeWalk > 1,
      `the alcove floor beside the pad must be walkable too: moved ${wedgeWalk}`,
    );

    // The bounded mover is the same character-controller step, so it agrees.
    await game.player.teleport(WEDGE);
    await game.step({ frames: 10 });
    const here = await game.player.position();
    const hop = await game.player.moveTo({ ...here, x: here.x + 1 });
    assert.ok(
      hop.distance_moved > 0,
      `a bounded move out of the alcove must make progress: ${JSON.stringify(hop)}`,
    );
  },
);

test(
  "hydro2: QBR reconstruction leaves the player able to move",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 5 });

    await game.player.spawnItem("20 Nanites");
    assert.ok((await carriedNaniteTotal(game)) >= RESURRECTION_COST);

    const [button] = await game.entities.byTemplate(RESURRECTION_BUTTON);
    assert.ok(button, "hydro2 should contain its authored QBR scanner button");
    const [target] = await game.entities.byTemplate(RESURRECTION_TARGET);
    assert.ok(target, "the QBR button should link to its authored teleport target");
    const targetPosition = (await game.entities.detail(target.id)).position;

    await game.entities.sendMessage(button.id, { type: "Frob" });
    await game.step({ frames: 2 });

    const before = await game.info();
    assert.ok(before.player.entity_id !== null, "mission should expose the player entity");
    await game.entities.sendMessage(before.player.entity_id, {
      type: "Damage",
      amount: (before.player.hit_points ?? 0) + 100,
    });
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.life_state, "respawning");

    await game.step({ frames: RESPAWN_DELAY_FRAMES });
    const revived = await game.info();
    assert.equal(revived.player.life_state, "alive");
    // The authored marker is free, so reconstruction must use it verbatim -
    // in all three axes, so a vertical nudge cannot pass unnoticed. (The
    // tolerance covers the settling that follows reconstruction, not a
    // deliberate step-aside.)
    assert.ok(
      distance(revived.player.position, targetPosition) < 0.25,
      `reconstruction should use the authored marker: target=${JSON.stringify(targetPosition)} actual=${JSON.stringify(revived.player.position)}`,
    );

    const spawn = revived.player.position;
    const walked = await bestWalkDistance(game, {
      x: spawn[0],
      y: spawn[1],
      z: spawn[2],
    });
    assert.ok(
      walked > 1,
      `a reconstructed player must be able to walk away: moved ${walked}`,
    );
  },
);
