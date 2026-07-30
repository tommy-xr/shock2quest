import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CARGO_LIFT_OBJECT = 669;
const CARGO_LIFT_BOTTOM_BUTTON_OBJECT = 480;
const CARGO_LIFT_MIDDLE_BUTTON_OBJECT = 481;
const CARGO_LIFT_TOP_BUTTON_OBJECT = 620;

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
