import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const GENERATOR_LADDER = 2354;
const GENERATOR_BOTTOM_RUNG = 2307;
const SHIELD_GENERATOR = 285;

test(
  "command1: the Generator ladder bottoms out onto supported lower terrain",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9472),
    });
    await game.step({ frames: 5 });

    const [ladder] = await game.entities.byTemplate(GENERATOR_LADDER);
    const [bottomRung] = await game.entities.byTemplate(GENERATOR_BOTTOM_RUNG);
    const [generator] = await game.entities.byTemplate(SHIELD_GENERATOR);
    assert.ok(ladder, "command1 must instantiate Generator ladder2354");
    assert.ok(bottomRung, "command1 must instantiate bottom rung2307");
    assert.ok(generator, "command1 must instantiate Shield Generator285");
    assert.ok(
      Math.abs(ladder.position[0] - -334.1879) < 0.05 &&
        Math.abs(bottomRung.position[1] - -7.9) < 0.05,
      `test must resolve the authored Generator column: ${JSON.stringify({ ladder, bottomRung })}`,
    );

    // Teleport is setup only: begin already touching the proven reachable east
    // face, then use ordinary crouch, look, and forward locomotion for the
    // entire descent and reverse ascent.
    await game.input.set("crouch", 1);
    await game.step({ frames: 2 });
    await game.player.teleport({ x: -333.6399, y: -3.9, z: 85.40596 });
    await game.input.lookAtWorldPoint([-344, -15, 85.40596]);
    await game.input.set("right_hand.thumbstick", [0, 1]);

    let crossedLip = false;
    let descent = await game.player.position();
    for (let frame = 0; frame < 180; frame += 1) {
      await game.step({ frames: 1 });
      descent = await game.player.position();
      if (descent.x < -334.2 && descent.y < -7.0) {
        crossedLip = true;
        break;
      }
      if (descent.y < -9.0) break;
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });

    const landed = await game.player.position();
    await game.step({ frames: 60 });
    const stable = await game.player.position();
    const support = await game.raycast({
      start: [stable.x, stable.y, stable.z],
      end: [stable.x, stable.y - 1.0, stable.z],
      collision_groups: ["entity", "world"],
      ignore_sensors: true,
    });
    assert.ok(
      crossedLip &&
        stable.x < -334.2 &&
        Math.abs(stable.y - -7.796) < 0.08 &&
        Math.abs(stable.y - landed.y) < 0.02 &&
        support.hit &&
        support.hit_point !== null &&
        Math.abs(support.hit_point[1] - -8.4) < 0.05 &&
        support.hit_normal !== null &&
        support.hit_normal[1] > 0.9,
      `ordinary east-face descent must settle on lower world support before Generator285: ${JSON.stringify({ crossedLip, descent, landed, stable, support })}`,
    );

    // The retail route requires climbing the same column back out after using
    // the Generator. Exercise the retreat without frobbing Generator285.
    await game.input.lookAtWorldPoint([-324, stable.y + 10, 85.40596]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 40 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const reversed = await game.player.position();
    assert.ok(
      reversed.y - stable.y > 1,
      `supported bottom-out must preserve reverse ascent: ${JSON.stringify({ stable, reversed })}`,
    );
  },
);
