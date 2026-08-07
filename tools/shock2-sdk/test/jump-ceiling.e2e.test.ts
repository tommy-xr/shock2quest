import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CARGO_LIFT_OBJECT = 669;
const CARGO_LIFT_BOTTOM_BUTTON_OBJECT = 480;
const CARGO_LIFT_MIDDLE_BUTTON_OBJECT = 481;
const CARGO_LIFT_TOP_BUTTON_OBJECT = 620;
const ENGINEERING_OG_PIPE_OBJECT = 1666;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

async function pulseJump(game: GameServer): Promise<void> {
  await game.input.setJump(true);
  await game.step({ frames: 1 });
  await game.input.setJump(false);
}

// Engineering campaign `engineering · none · legacy · seed 1378752978`
// reached Cargo 2B's top lift through ordinary play. Walking north from the
// lift toward its top call button is the valid route; jumping west instead
// exposed a collision escape onto the exterior roof.
test(
  "ordinary jump stays below Engineering Cargo 2B's world ceiling",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
      port: Number(process.env.SHOCK2_E2E_ENG2_CEILING_PORT ?? 8140),
    });
    await game.step({ frames: 5 });

    const lift = only(
      await game.entities.byTemplate(CARGO_LIFT_OBJECT),
      "Cargo 2B East lift mission object 669",
    );
    const bottomButton = only(
      await game.entities.byTemplate(CARGO_LIFT_BOTTOM_BUTTON_OBJECT),
      "bottom lift button mission object 480",
    );
    const middleButton = only(
      await game.entities.byTemplate(CARGO_LIFT_MIDDLE_BUTTON_OBJECT),
      "middle lift button mission object 481",
    );
    const topButton = only(
      await game.entities.byTemplate(CARGO_LIFT_TOP_BUTTON_OBJECT),
      "top lift button mission object 620",
    );

    // Setup only: drive the real lift along its authored 477 -> 478 -> 479
    // path and stage at the exact campaign-observed top-stop pose.
    await game.entities.sendMessage(bottomButton.id, { type: "Frob" });
    await game.step({ frames: 600 });
    const middleLift = await game.entities.detail(lift.id);
    assert.ok(
      Math.abs(middleLift.position[1] - -5.1) < 0.1,
      `lift should reach authored middle node y=-5.1, got ${middleLift.position[1]}`,
    );
    await game.entities.sendMessage(middleButton.id, { type: "Frob" });
    await game.step({ frames: 600 });
    const topLift = await game.entities.detail(lift.id);
    assert.ok(
      Math.abs(topLift.position[1] - 2.1) < 0.1,
      `lift should reach authored top node y=2.1, got ${topLift.position[1]}`,
    );

    await game.player.teleport({
      x: 43.9006,
      y: 3.144,
      z: -168.2003,
    });
    await game.step({ frames: 30 });
    const before = await game.player.position();
    assert.ok(
      Math.abs(before.x - 43.9006) < 0.1 &&
        Math.abs(before.y - 3.144) < 0.1 &&
        Math.abs(before.z - -168.2003) < 0.1,
      `player should settle at the campaign lift pose, got ${JSON.stringify(before)}`,
    );

    // Control: the authored route walks north off the same top stop toward
    // button 620. Prove it remains available, then return to the exact setup
    // pose without carrying input or a jump edge into the regression.
    await game.input.lookAtWorldPoint([
      topButton.position[0],
      before.y + 1.6,
      topButton.position[2],
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });
    const northExit = await game.player.position();
    assert.ok(
      northExit.z > -166 && northExit.y < 6.8,
      `ordinary walking should leave the lift north toward top button 620, got ` +
        JSON.stringify(northExit),
    );
    await game.player.teleport({
      x: 43.9006,
      y: 3.144,
      z: -168.2003,
    });
    await game.step({ frames: 30 });
    const jumpStart = await game.player.position();

    // Self-check the parentless world slab the campaign escaped through.
    // Probing from below sees its underside, while the reverse ray finds the
    // exterior roof at the same y. This is level geometry, not a lift entity.
    const ceilingUp = await game.raycast({
      start: [41.5, jumpStart.y, -169.1],
      end: [41.5, 10, -169.1],
      collision_groups: ["world"],
    });
    const roofDown = await game.raycast({
      start: [41.5, 10, -169.1],
      end: [41.5, jumpStart.y, -169.1],
      collision_groups: ["world"],
    });
    assert.ok(
      ceilingUp.hit_point &&
        roofDown.hit_point &&
        Math.abs(ceilingUp.hit_point[1] - 6.8) < 0.1 &&
        Math.abs(roofDown.hit_point[1] - 6.8) < 0.1,
      `expected Cargo 2B world ceiling/roof at y=6.8, got ` +
        `${JSON.stringify({ ceilingUp, roofDown })}`,
    );

    // Exact product-input sequence from the campaign: aim west, hold ordinary
    // forward locomotion, pulse one grounded jump edge, then release.
    await game.input.lookAtWorldPoint([39, jumpStart.y + 1.6, -170]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 180 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 180 });
    const landed = await game.player.position();
    await game.step({ frames: 120 });
    const supported = await game.player.position();

    const ceilingY = ceilingUp.hit_point[1];
    assert.ok(
      landed.y < ceilingY &&
        supported.y < ceilingY &&
        Math.hypot(
          supported.x - landed.x,
          supported.y - landed.y,
          supported.z - landed.z,
        ) < 0.05,
      `one ordinary jump must remain inside below the Cargo 2B ceiling ` +
        `(${JSON.stringify(jumpStart)} -> ${JSON.stringify(landed)} -> ` +
        `${JSON.stringify(supported)}, ceiling y=${ceilingY})`,
    );
    t.diagnostic(
      `Cargo 2B ceiling: lift ${JSON.stringify(topLift.position)}, ` +
        `north exit ${JSON.stringify(northExit)}, player ` +
        `${JSON.stringify(jumpStart)} -> ${JSON.stringify(landed)} -> ` +
        `${JSON.stringify(supported)}, world ceiling y=${ceilingY}`,
    );
  },
);

// The Engineering campaign's authored maintenance-maze route reaches this
// corridor from below. An idle OG-Pipe narrows the passage, but an ordinary
// jump used to turn that local obstruction into a scripted crossing through
// the corridor ceiling and leave the player supported on the exterior shell.
test(
  "ordinary jump past Engineering's OG-Pipe stays below the corridor ceiling",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_ENG1_OG_PIPE_PORT ?? 8141),
    });
    await game.step({ frames: 30 });

    const ogPipe = only(
      await game.entities.byTemplate(ENGINEERING_OG_PIPE_OBJECT),
      "maintenance-maze OG-Pipe mission object 1666",
    );
    assert.equal(ogPipe.name, "OG-Pipe");

    // Setup only: stage at the campaign-observed interior approach, then let
    // both player support and the authored dynamic creature settle. Starting
    // the input edge before support is re-established after setup does not
    // exercise a grounded production jump.
    await game.player.teleport({
      x: 4.4,
      y: -15.556003,
      z: -85.00001,
    });
    await game.step({ frames: 30 });
    const before = await game.player.position();
    const settledOgPipe = await game.entities.detail(ogPipe.id);
    assert.ok(
      Math.abs(before.x - 4.4) < 0.1 &&
        Math.abs(before.y - -15.556003) < 0.1 &&
        Math.abs(before.z - -85.00001) < 0.1,
      `player should settle at the authentic maze approach, got ${JSON.stringify(before)}`,
    );
    assert.ok(
      settledOgPipe.position[0] > 5 &&
        settledOgPipe.position[0] < 7 &&
        Math.abs(settledOgPipe.position[1] - -15.1) < 0.15 &&
        settledOgPipe.position[2] > -86 &&
        settledOgPipe.position[2] < -84,
      `stable OG-Pipe 1666 should occupy the authored choke, got ` +
        JSON.stringify(settledOgPipe.position),
    );

    // Self-check the nearby parentless world slab that bounds the corridor.
    // The exploit exits through this ceiling and settles above its y plane.
    const ceiling = await game.raycast({
      start: [7.5, before.y, -84.5],
      end: [7.5, -9, -84.5],
      collision_groups: ["world"],
    });
    assert.ok(
      ceiling.hit_point && Math.abs(ceiling.hit_point[1] - -12.9) < 0.1,
      `expected maintenance-maze world ceiling at y=-12.9, got ${JSON.stringify(ceiling)}`,
    );

    // Exact product-input sequence from the campaign: aim through the choke,
    // hold ordinary forward locomotion, and pulse one grounded Jump edge.
    await game.input.lookAtWorldPoint([8, before.y + 1.6, -84.5]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await pulseJump(game);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });
    const landed = await game.player.position();
    await game.step({ frames: 240 });
    const supported = await game.player.position();

    const ceilingY = ceiling.hit_point[1];
    assert.ok(
      landed.y < ceilingY &&
        supported.y < ceilingY &&
        Math.hypot(
          supported.x - landed.x,
          supported.y - landed.y,
          supported.z - landed.z,
        ) < 0.05,
      `one ordinary jump must remain inside below the OG-Pipe corridor ceiling ` +
        `(${JSON.stringify(before)} -> ${JSON.stringify(landed)} -> ` +
        `${JSON.stringify(supported)}, ceiling y=${ceilingY})`,
    );
    t.diagnostic(
      `OG-Pipe 1666 ${JSON.stringify(settledOgPipe.position)}, player ` +
        `${JSON.stringify(before)} -> ${JSON.stringify(landed)} -> ` +
        `${JSON.stringify(supported)}, world ceiling y=${ceilingY}`,
    );
  },
);
