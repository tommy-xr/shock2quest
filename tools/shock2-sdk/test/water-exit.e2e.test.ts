import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Issue #680: rec1's Athletics pool is authored water. Without water-medium
// locomotion the player was pinned on the pool floor below the west-wall duct.
const POOL_FLOOR = { x: 12.26, y: -8.0, z: -231.1 };
const WEST_DUCT = [0, -6.5, -231.1] as const;

test(
  "rec1 pool: neutral buoyancy, then swim up and mantle into the west duct",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 0),
    });
    assert.equal((await game.player.teleport(POOL_FLOOR)).success, true);
    await game.step({ frames: 30 });
    const floor = await game.player.position();
    assert.ok(floor.y < -7.5, `must start on the pool floor, got ${JSON.stringify(floor)}`);

    // Jump alone swims up off the floor; releasing it leaves the player afloat.
    await game.input.set("jump", 1);
    await game.step({ frames: 20 });
    await game.input.set("jump", 0);
    const risen = await game.player.position();
    await game.step({ frames: 30 });
    const floating = await game.player.position();
    assert.ok(risen.y > floor.y + 0.3, `held jump must swim up, got ${JSON.stringify(risen)}`);
    assert.ok(
      Math.abs(floating.y - risen.y) < 0.1,
      `water must hold the player neutrally buoyant, ${JSON.stringify(risen)} -> ${JSON.stringify(floating)}`,
    );

    // Held forward + jump toward the duct: swim up, then mantle the lip.
    await game.input.lookAtWorldPoint([...WEST_DUCT]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.input.set("jump", 1);
    await game.step({ frames: 160 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.input.set("jump", 0);
    await game.step({ frames: 5 });

    const after = await game.player.position();
    assert.ok(
      after.y > -4.5 && after.x < 10,
      `held swim+jump must reach the west duct, ended ${JSON.stringify(after)}`,
    );
  },
);

// Quest jump is a hand's lower face button, not a held runtime channel:
// holding it must keep swimming up, then tread water at the surface.
test(
  "rec1 pool (VR): holding the lower face button swims up and treads water",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 0),
      debugFlags: ["--vr"],
    });
    assert.equal((await game.player.teleport(POOL_FLOOR)).success, true);
    await game.step({ frames: 30 });
    const floor = await game.player.position();

    await game.input.hold("LeftHandLowerButton");
    await game.step({ frames: 60 });
    const risen = await game.player.position();
    assert.ok(
      risen.y > floor.y + 0.6,
      `a held lower button must keep swimming up, ${JSON.stringify(floor)} -> ${JSON.stringify(risen)}`,
    );

    // Surfaced: further holding settles instead of bobbing through the surface.
    await game.step({ frames: 240 });
    const samples: number[] = [];
    for (let i = 0; i < 6; i++) {
      await game.step({ frames: 5 });
      samples.push((await game.player.position()).y);
    }
    await game.input.release("LeftHandLowerButton");
    const spread = Math.max(...samples) - Math.min(...samples);
    assert.ok(spread < 0.05, `treading water must be steady, y samples ${JSON.stringify(samples)}`);
  },
);
