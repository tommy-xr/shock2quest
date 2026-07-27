import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops4.mis: Lift 1 carries a centered player to its lower stop",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops4.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8167),
    });
    await game.step({ frames: 5 });

    const lift = (await game.entities.list({ filter: "Lift 1", limit: 20 })).entities.find(
      (entity) => entity.template_id === 572,
    );
    const button = (await game.entities.list({ filter: "Button #1", limit: 50 })).entities.find(
      (entity) => entity.template_id === 600,
    );
    assert.ok(lift, "expected Ops4 mission object 572");
    assert.ok(button, "expected Ops4 upper lift button 600");

    await game.player.teleport({ x: 45.3, y: -9.74, z: -106.0 });
    await game.step({ frames: 10 });
    const before = await game.info();
    const initialOffset = before.player.position[1] - lift.position[1];

    await game.player.aimAt(button);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 180 });

    const after = await game.info();
    const movedLift = (
      await game.entities.list({ filter: "Lift 1", limit: 20 })
    ).entities.find((entity) => entity.template_id === 572);
    assert.ok(movedLift);
    assert.ok(movedLift.position[1] < -15.7, `lift did not reach lower stop: ${movedLift.position}`);
    const finalOffset = after.player.position[1] - movedLift.position[1];
    assert.ok(
      Math.abs(finalOffset - initialOffset) < 0.15,
      `rider separated from lift: initial offset ${initialOffset}, final offset ${finalOffset}`,
    );
  },
);
