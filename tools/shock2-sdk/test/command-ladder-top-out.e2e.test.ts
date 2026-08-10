import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const GENERATOR_LADDER = 2354;
const SHIELD_GENERATOR = 285;

test(
  "command1: the Generator ladder tops out onto durable upper support",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9477),
    });
    await game.step({ frames: 5 });

    const [ladder] = await game.entities.byTemplate(GENERATOR_LADDER);
    const [generator] = await game.entities.byTemplate(SHIELD_GENERATOR);
    assert.ok(ladder, "command1 must instantiate Generator ladder2354");
    assert.ok(generator, "command1 must instantiate Shield Generator285");
    assert.ok(
      Math.abs(ladder.position[0] - -334.1879) < 0.05,
      `test must resolve the authored Generator ladder: ${JSON.stringify(ladder)}`,
    );

    // Teleport is setup only: begin standing on the real lower Generator-room
    // floor, then use ordinary look and held forward input for the complete
    // west-face climb and release. No Generator interaction is performed.
    await game.input.set("crouch", 0);
    await game.player.teleport({ x: -334.94, y: -7.156, z: 85.34618 });
    await game.step({ frames: 5 });
    const hpBefore = (await game.info()).player.hit_points;
    await game.input.lookAtWorldPoint([-324.94, -7.156, 85.34618]);
    await game.input.set("right_hand.thumbstick", [0, 1]);

    let highestY = Number.NEGATIVE_INFINITY;
    for (let frame = 0; frame < 180; frame += 1) {
      await game.step({ frames: 1 });
      highestY = Math.max(highestY, (await game.player.position()).y);
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });
    const landed = await game.player.position();
    await game.step({ frames: 60 });
    const stable = await game.player.position();
    const support = await game.raycast({
      start: [stable.x, stable.y, stable.z],
      end: [stable.x, stable.y - 2, stable.z],
      collision_groups: ["entity", "world"],
      ignore_sensors: true,
    });
    const hpAfter = (await game.info()).player.hit_points;

    assert.ok(
      highestY > -0.2,
      `ordinary ascent must reach the authored upper-deck underside: ${JSON.stringify({ highestY, landed, stable })}`,
    );
    assert.ok(
      stable.x < -334 &&
        Math.abs(stable.y - 2.444) < 0.08 &&
        Math.abs(stable.y - landed.y) < 0.02 &&
        support.hit &&
        support.hit_point !== null &&
        Math.abs(support.hit_point[1] - 1.2) < 0.05 &&
        support.hit_normal !== null &&
        support.hit_normal[1] > 0.9,
      `ordinary west-face ascent must release durably on upper world support: ${JSON.stringify({ highestY, landed, stable, support })}`,
    );
    assert.equal(hpAfter, hpBefore, "the ladder route must not damage the player");
  },
);
