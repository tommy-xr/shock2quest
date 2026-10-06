import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// #1962: these two ordinary jump approaches used to complete a scripted mantle
// below the stock Many stomach bank. Staging is solely an isolated regression
// fixture at the observed campaign Agility 3; no campaign save, equipment grant,
// or script message is required.
const approaches = [
  {
    name: "east bank",
    start: { x: 206.30144, y: -5.443322, z: 186.31438 },
    moves: [
      [[210, -2, 188], 60, 1],
      [[214, 0, 188], 35, 1],
      [[211, 1, 190], 30, 1],
      [[214, 0, 190], 30, 0],
    ],
  },
  {
    name: "diagonal bank",
    start: { x: 204.579, y: -5.879, z: 190.404 },
    moves: [
      [[209.8211, 0, 195.6211], 60, 1],
      [[209.8211, 0, 195.6211], 35, 1],
      [[209.8211, 0, 195.6211], 25, 0],
    ],
  },
] as const;

for (const approach of approaches) {
  test(
    `many: ${approach.name} mantle stays above solid terrain`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "many.mis",
        port: Number(process.env.SHOCK2_E2E_PORT ?? 0),
      });
      await game.player.setStats({ agility: 3 });
      assert.equal((await game.player.teleport(approach.start)).success, true);
      await game.input.set("crouch", 0);

      const samples: unknown[] = [];
      const assertAboveBank = async () => {
        const player = (await game.info()).player;
        const [x, y, z] = player.position;
        // Y1 is below this bank's ceiling and above its sloped/flat floor.
        // Query the actual WORLD triangles, not an approximate slope formula.
        const floor = await game.raycast({
          start: [x, 1, z],
          end: [x, -40, z],
          collision_groups: ["world"],
        });
        assert.ok(floor.hit && floor.hit_point, "the bank fixture must have a solid floor");
        samples.push({ position: player.position, floor: floor.hit_point, vaulting: player.climb.vaulting });
        assert.ok(
          player.climb.vaulting || y >= floor.hit_point[1] - 0.1,
          `mantle entered solid terrain: player ${JSON.stringify(player.position)}, floor ${JSON.stringify(floor.hit_point)}; samples ${JSON.stringify(samples)}`,
        );
        // Authored enemies remain active on the dry shore. Survival and the
        // expanded WORLD-floor pose are the geometry invariant; unchanged HP
        // would conflate this mantle regression with ordinary mission combat.
        assert.equal(player.life_state, "alive");
      };

      for (const [target, frames, jump] of approach.moves) {
        await game.input.lookAtWorldPoint([...target]);
        await game.input.set("right_hand.thumbstick", [0, 1]);
        await game.input.set("jump", jump);
        for (let frame = 0; frame < frames; frame += 5) {
          await game.step({ frames: Math.min(5, frames - frame) });
          await assertAboveBank();
        }
        await game.input.set("right_hand.thumbstick", [0, 0]);
        await game.input.set("jump", 0);
        if (jump) await game.step({ frames: 1 });
      }
      // A transition that looks safe while compressed can escape on expansion.
      for (let frame = 0; frame < 60; frame += 5) {
        await game.step({ frames: 5 });
        await assertAboveBank();
      }
      assert.equal((await game.info()).player.climb.vaulting, false);
    },
  );
}
