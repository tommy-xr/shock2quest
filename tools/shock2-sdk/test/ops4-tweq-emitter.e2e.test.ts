import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8188);

const TRIPWIRE = 662;
const EMITTER = 682;
const JUNCTION = 685;
const DESTROY_TRAP = 664;
const BARRIERS = [545, 658, 659];
const GRUB = -182;

async function waitForGrubCount(game: GameServer, expected: number): Promise<void> {
  for (let frame = 0; frame < 120; frame += 1) {
    if ((await game.entities.byTemplate(GRUB)).length === expected) return;
    await game.step({ frames: 1 });
  }
  assert.equal(
    (await game.entities.byTemplate(GRUB)).length,
    expected,
    `expected ${expected} emitted Grubs within two seconds`,
  );
}

test(
  "ops4.mis: tripwire 662 emits three launched Grubs and persists completion",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const midBurstSave = `ops4_tweq_mid_${Date.now()}`;
    const completedSave = `ops4_tweq_done_${Date.now()}`;

    {
      await using game = await GameServer.launch({
        mission: "ops4.mis",
        port: basePort,
      });
      await game.step({ frames: 5 });

      assert.equal((await game.entities.byTemplate(TRIPWIRE)).length, 1);
      assert.equal((await game.entities.byTemplate(EMITTER)).length, 1);
      assert.equal((await game.entities.byTemplate(GRUB)).length, 0);
      const audioBefore = await game.audio.recent();
      const sequenceBefore = audioBefore.sounds.at(-1)?.sequence ?? 0;

      // Cross the real tripwire through the same collision-valid authored
      // corridor used by the Junction escape regression. No script message is
      // injected into the tripwire or emitter.
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

      const live = (await game.entities.list()).entities;
      const junction = live.find((entity) => entity.template_id === JUNCTION);
      const destroyTrap = live.find((entity) => entity.template_id === DESTROY_TRAP);
      assert.ok(junction, "expected authored Junction Box 685");
      assert.ok(destroyTrap, "expected authored Destroy Trap 664");
      for (const objectId of BARRIERS) {
        assert.ok(
          live.some((entity) => entity.template_id === objectId),
          `tripwire must deploy barrier ${objectId}`,
        );
      }

      // Three ordinary points are the Junction's authored HP. This follows the
      // production death-trigger link to Destroy Trap 664 and reopens the lane.
      await game.entities.sendMessage(junction.id, { type: "Damage", amount: 3 });
      await game.step({ frames: 4 });
      const escapedWorld = (await game.entities.list()).entities;
      assert.ok(!escapedWorld.some((entity) => entity.id === junction.id));
      assert.ok(!escapedWorld.some((entity) => entity.id === destroyTrap.id));
      for (const objectId of BARRIERS) {
        assert.ok(
          !escapedWorld.some((entity) => entity.template_id === objectId),
          `Destroy Trap 664 must remove barrier ${objectId}`,
        );
      }
      for (const waypoint of [
        { x: 34.3, y: -14.596, z: -104.85 },
        { x: 35.3, y: -14.596, z: -105.51 },
        { x: 36.1, y: -14.596, z: -105.95 },
      ]) {
        const exit = await game.player.moveTo(waypoint);
        assert.equal(exit.blocked, false, "the Junction escape lane must reopen");
        await game.step({ frames: 2 });
      }

      await waitForGrubCount(game, 1);
      const [firstGrub] = await game.entities.byTemplate(GRUB);
      assert.ok(firstGrub, "the first emission must resolve case-insensitive 'grub'");
      const bodies = await game.physics.bodies({ entityId: firstGrub.id });
      assert.equal(bodies.bodies.length, 1, "emitted Grub must have one live physics body");
      assert.equal(bodies.bodies[0].body_type, "dynamic");
      const [vx, , vz] = bodies.bodies[0].velocity;
      assert.ok(vx < -8, `emitter 682 must launch along authored world -X, got vx=${vx}`);
      assert.ok(Math.abs(vz) < 2, `emitter 682 must not rotate velocity into Z, got vz=${vz}`);

      assert.equal((await game.save(midBurstSave)).success, true);
      const newSounds = (await game.audio.recent()).sounds.filter(
        (sound) => sound.sequence > sequenceBefore,
      );
      assert.ok(
        newSounds.every((sound) => sound.sample.toLowerCase() !== "exphe2"),
        `Grub emission must not substitute HE audio: ${JSON.stringify(newSounds)}`,
      );
    }

    // A fresh process must restore one consumed frame and emit only the two
    // remaining Grubs. The completed DestroyObject emitter is absent.
    {
      await using game = await GameServer.launch({
        mission: "ops1.mis",
        port: basePort + 1,
      });
      assert.equal((await game.load(midBurstSave)).success, true);
      assert.equal((await game.info()).mission, "ops4.mis");
      const [restoredGrub] = await game.entities.byTemplate(GRUB);
      assert.ok(restoredGrub, "the first emitted Grub must survive the save");
      const restoredBodies = await game.physics.bodies({ entityId: restoredGrub.id });
      assert.equal(restoredBodies.bodies.length, 1);
      assert.equal(
        restoredBodies.bodies[0].body_type,
        "dynamic",
        "the launched-physics marker must survive save/load",
      );

      await waitForGrubCount(game, 3);
      assert.equal(
        (await game.entities.byTemplate(EMITTER)).length,
        0,
        "DestroyObject completion must consume emitter 682",
      );
      await game.step({ frames: 120 });
      assert.equal(
        (await game.entities.byTemplate(GRUB)).length,
        3,
        "completed emitter must not produce a fourth Grub",
      );
      assert.equal((await game.save(completedSave)).success, true);
    }

    // Completion is persisted too: revisiting the saved state neither restores
    // the emitter nor rearms its burst.
    {
      await using game = await GameServer.launch({
        mission: "ops1.mis",
        port: basePort + 2,
      });
      assert.equal((await game.load(completedSave)).success, true);
      assert.equal((await game.entities.byTemplate(EMITTER)).length, 0);
      assert.equal((await game.entities.byTemplate(GRUB)).length, 3);
      await game.step({ frames: 120 });
      assert.equal((await game.entities.byTemplate(GRUB)).length, 3);
    }
  },
);
