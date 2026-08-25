import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops4.mis: destroying Junction 685 removes the authored trap barrier",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops4.mis",
    });
    await game.step({ frames: 5 });

    const all = (await game.entities.list()).entities;
    const junction = all.find((entity) => entity.template_id === 685);
    const destroyTrap = all.find((entity) => entity.template_id === 664);
    const barrierIds = [545, 658, 659]
      .map((templateId) => all.find((entity) => entity.template_id === templateId))
      .map((entity) => {
        assert.ok(entity, "expected every authored force bar");
        return entity.id;
      });
    assert.ok(junction, "expected Junction Box mission object 685");
    assert.ok(destroyTrap, "expected Destroy Trap mission object 664");

    // Cross tripwire 662 by the collision-valid authored curve. This exercises
    // the real trap chain rather than injecting TurnOn into its targets.
    await game.player.teleport({ x: 37.200462, y: -14.596002, z: -104.299965 });
    await game.step({ frames: 2 });
    for (const waypoint of [
      { x: 36.9, y: -14.596, z: -105.43 },
      { x: 36.1, y: -14.596, z: -105.95 },
      { x: 35.3, y: -14.596, z: -105.51 },
      { x: 34.3, y: -14.596, z: -104.85 },
      { x: 32.5, y: -13.8, z: -104.32 },
    ]) {
      const move = await game.player.moveTo(waypoint);
      assert.equal(move.moved, true);
      await game.step({ frames: 2 });
    }
    await game.step({ frames: 5 });

    const liveBefore = (await game.entities.list()).entities;
    for (const id of barrierIds) {
      const bar = liveBefore.find((entity) => entity.id === id);
      assert.ok(bar, "tripwire must leave each force bar alive");
      assert.ok(
        Math.abs(bar.position[0] - 35.701313) < 0.1,
        `tripwire must deploy force bar ${id}, got ${bar.position}`,
      );
    }

    // A bullet delivers Damage through this same script queue. Three points is
    // Junction 685's authored HP; the regression is the death-trigger relay.
    await game.entities.sendMessage(junction.id, { type: "Damage", amount: 3 });
    await game.step({ frames: 4 });

    const liveAfter = (await game.entities.list()).entities;
    assert.ok(!liveAfter.some((entity) => entity.id === junction.id), "junction must be slain");
    assert.ok(
      !liveAfter.some((entity) => entity.id === destroyTrap.id),
      "TriggerDestroy must activate and consume Destroy Trap 664",
    );
    for (const id of barrierIds) {
      assert.ok(
        !liveAfter.some((entity) => entity.id === id),
        `Destroy Trap 664 must remove force bar ${id}`,
      );
    }

    for (const waypoint of [
      { x: 34.3, y: -14.596, z: -104.85 },
      { x: 35.3, y: -14.596, z: -105.51 },
      { x: 36.1, y: -14.596, z: -105.95 },
    ]) {
      const exit = await game.player.moveTo(waypoint);
      assert.equal(exit.moved, true);
      assert.equal(exit.blocked, false, "the authored exit lane must reopen");
      await game.step({ frames: 2 });
    }
    const outside = await game.player.position();
    assert.ok(outside.x > 35.7, `player must cross the former barrier, got ${outside.x}`);
  },
);
