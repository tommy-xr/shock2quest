import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { carriedNaniteTotal } from "./helpers/earth-replicator.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8198);

const RESURRECTION_BUTTON = 909;
const RESURRECTION_TARGET = 187;
const RESURRECTION_COST = 10;
const RESPAWN_DELAY_FRAMES = 5 * 60;

type LifeState = "alive" | "dead" | "respawning";

function lifeState(player: object): LifeState | undefined {
  return (player as { life_state?: LifeState }).life_state;
}

async function killPlayer(game: GameServer): Promise<void> {
  const before = await game.info();
  assert.ok(before.player.entity_id !== null, "mission should expose the player entity");
  assert.ok(before.player.hit_points !== null, "player should have hit points");
  await game.entities.sendMessage(before.player.entity_id, {
    type: "Damage",
    amount: before.player.hit_points + 100,
  });
  await game.step({ frames: 1 });
}

test(
  "player death becomes terminal without an activated resurrection station",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 5 });

    await killPlayer(game);
    const dead = await game.info();
    assert.equal(dead.player.hit_points, 0, "lethal damage should clamp HP at zero");
    assert.equal(
      lifeState(dead.player),
      "dead",
      "death without an active QBR must expose a terminal loss signal",
    );

    const deathPosition = dead.player.position;
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const afterInput = await game.info();
    assert.equal(lifeState(afterInput.player), "dead");
    assert.ok(
      Math.hypot(
        afterInput.player.position[0] - deathPosition[0],
        afterInput.player.position[1] - deathPosition[1],
        afterInput.player.position[2] - deathPosition[2],
      ) < 0.001,
      `dead players must not keep moving: before=${JSON.stringify(deathPosition)} after=${JSON.stringify(afterInput.player.position)}`,
    );
  },
);

test(
  "an activated resurrection station revives the player after charging 10 nanites",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 1,
    });
    await game.step({ frames: 5 });

    await game.player.spawnItem("20 Nanites");
    const nanitesBefore = await carriedNaniteTotal(game);
    assert.ok(nanitesBefore >= RESURRECTION_COST);

    const [button] = await game.entities.byTemplate(RESURRECTION_BUTTON);
    assert.ok(button, "medsci1 should contain its authored QBR scanner button");
    const [target] = await game.entities.byTemplate(RESURRECTION_TARGET);
    assert.ok(target, "QBR button should link to its authored teleport target");
    const targetPosition = (await game.entities.detail(target.id)).position;

    await game.entities.sendMessage(button.id, { type: "Frob" });
    await game.step({ frames: 2 });

    // Activation is the scanner's authored switched model, which must survive
    // a save/load before a later death can rely on it.
    const saveName = `player_death_qbr_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 2 });

    await killPlayer(game);

    const dying = await game.info();
    assert.equal(lifeState(dying.player), "respawning");
    assert.equal(dying.player.hit_points, 0);
    assert.equal(
      await carriedNaniteTotal(game),
      nanitesBefore - RESURRECTION_COST,
      "death should charge the retail 10-nanite reconstruction cost once",
    );

    await game.step({ frames: RESPAWN_DELAY_FRAMES });
    const revived = await game.info();
    assert.equal(lifeState(revived.player), "alive");
    assert.equal(
      revived.player.hit_points,
      Math.floor((revived.player.max_hit_points ?? 0) / 2),
      "QBR reconstruction should restore half of maximum health",
    );
    assert.ok(
      Math.hypot(
        revived.player.position[0] - targetPosition[0],
        revived.player.position[1] - targetPosition[1],
        revived.player.position[2] - targetPosition[2],
      ) < 0.25,
      `player should respawn at the QBR teleport target: target=${JSON.stringify(targetPosition)} actual=${JSON.stringify(revived.player.position)}`,
    );
  },
);
