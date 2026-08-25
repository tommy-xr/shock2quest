import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops4.mis: tripwire door 676 stays open across a fresh-process save load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `ops4_open_door_${Date.now()}`;
    let openZ = 0;

    {
      await using game = await GameServer.launch({
        mission: "ops4.mis",
      });
      await game.step({ frames: 5 });
      const door = (
        await game.entities.list({ filter: "Ops Crew", limit: 50 })
      ).entities.find((entity) => entity.template_id === 676);
      assert.ok(door, "expected mission door 676");

      // Locomotion teleports participate in production sensor intersections.
      // Enter tripwire 677 exactly as a player traversal does.
      await game.player.teleport({ x: 2.4, y: -9.6, z: -29.611 });
      await game.step({ frames: 60 });

      const opened = await game.entities.detail(door.id);
      openZ = opened.position[2];
      assert.ok(
        Math.abs(openZ - door.position[2]) > 2.2,
        `tripwire should open door 676: ${door.position[2]} -> ${openZ}`,
      );
      await game.player.teleport({ x: 5.0, y: -7.8, z: -29.4 });
      await game.step({ frames: 2 });
      assert.equal((await game.save(saveName)).success, true);
    }

    {
      await using game = await GameServer.launch({
        mission: "ops1.mis",
      });
      assert.equal((await game.load(saveName)).success, true);
      assert.equal((await game.info()).mission, "ops4.mis");

      const door = (
        await game.entities.list({ filter: "Ops Crew", limit: 50 })
      ).entities.find((entity) => entity.template_id === 676);
      assert.ok(door, "expected restored mission door 676");
      assert.ok(
        Math.abs(door.position[2] - openZ) < 0.01,
        `restored door must remain at saved open endpoint ${openZ}, got ${door.position[2]}`,
      );

      const reverse = await game.player.moveTo({
        x: 0.5,
        y: -7.8,
        z: -29.4,
      });
      assert.equal(reverse.blocked, false, JSON.stringify(reverse));
      assert.ok((await game.player.position()).x < 2.4);
    }
  },
);
