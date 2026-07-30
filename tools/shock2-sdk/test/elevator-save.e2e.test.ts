import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CARGO_LIFT_OBJECT = 669;
const BOTTOM_NODE_Y = -12.6;
const MIDDLE_NODE_Y = -5.1;
const TOP_NODE_Y = 2.1;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

async function object(game: GameServer, missionObjectId: number, label: string) {
  return only(await game.entities.byTemplate(missionObjectId), label);
}

async function squeeze(game: GameServer, entity: EntitySummary): Promise<void> {
  const aim = await game.player.aimAt(entity, {
    hitbox: "surface",
    visibility: "required",
  });
  assert.equal(aim.target_confirmed, true);
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 2 });
}

async function saveLoad(game: GameServer, station: string): Promise<void> {
  const saveName = `eng2_cargo_lift_${station}_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  await game.step({ frames: 30 });
}

async function walkTo(
  game: GameServer,
  target: { x: number; y: number; z: number },
): Promise<void> {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const before = await game.player.position();
    if (Math.hypot(before.x - target.x, before.z - target.z) < 0.3) return;
    await game.player.moveTo(target);
    await game.step({ frames: 5 });
  }
  const position = await game.player.position();
  assert.ok(
    Math.hypot(position.x - target.x, position.z - target.z) < 0.3,
    `ordinary movement should reach ${JSON.stringify(target)}, got ${JSON.stringify(position)}`,
  );
}

async function assertLiftAt(
  game: GameServer,
  expectedY: number,
  label: string,
): Promise<EntitySummary> {
  const lift = await object(
    game,
    CARGO_LIFT_OBJECT,
    "Cargo 2B East lift mission object 669",
  );
  const detail = await game.entities.detail(lift.id);
  assert.ok(
    Math.abs(detail.position[1] - expectedY) < 0.1,
    `${label}: expected lift y=${expectedY}, got ${detail.position[1]}`,
  );
  return lift;
}

// Engineering campaign `engineering · none · legacy · seed 1378752978`
// saved beside Sanger after riding Cargo 2B East lift 669 to its top station.
// Loading preserved the platform position but lost BaseElevator's station
// index: top button 620 then went to the middle, and repeated presses could
// never reach the authored bottom exit.
test(
  "eng2 Cargo lift preserves its authored station across save/load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
      port: Number(process.env.SHOCK2_E2E_ELEVATOR_SAVE_PORT ?? 8141),
    });
    await game.step({ frames: 5 });

    // Setup only: dispatch the real lift through its authored
    // 477 (bottom) -> 478 (middle) -> 479 (top) path.
    const bottomButton = await object(game, 480, "bottom button mission object 480");
    const middleButton = await object(game, 481, "middle button mission object 481");
    await game.entities.sendMessage(bottomButton.id, { type: "Frob" });
    await game.step({ frames: 600 });
    await assertLiftAt(game, MIDDLE_NODE_Y, "setup middle station");
    await game.entities.sendMessage(middleButton.id, { type: "Frob" });
    await game.step({ frames: 600 });
    await assertLiftAt(game, TOP_NODE_Y, "setup top station");

    await game.player.teleport({
      x: 43.9006,
      y: 3.144,
      z: -168.2003,
    });
    await game.step({ frames: 30 });

    // Negative-first campaign repro: after restoring the top station, use the
    // real flat interaction ray and squeeze edge on authored button 620.
    await saveLoad(game, "top");
    await assertLiftAt(game, TOP_NODE_Y, "loaded top station");
    const topButton = await object(game, 620, "top button mission object 620");
    const topPlayer = await game.player.position();
    await squeeze(game, topButton);
    await game.step({ frames: 600 });
    const bottomLift = await assertLiftAt(
      game,
      BOTTOM_NODE_Y,
      "top station should cycle to bottom",
    );
    const bottomPlayer = await game.player.position();
    assert.ok(
      Math.abs(bottomPlayer.y - -11.556) < 0.1,
      `top-to-bottom lift should carry player to y=-11.556, got ${JSON.stringify(bottomPlayer)}`,
    );

    // The authored return leaves the bottom car to the north. This proves the
    // saved player is no longer forced to reverse the one-way crate stack.
    for (const target of [
      { x: 43.9, y: -11.55, z: -163 },
      { x: 43.9, y: -11.55, z: -158 },
    ]) {
      await walkTo(game, target);
    }
    const bottomExit = await game.player.position();
    assert.ok(bottomExit.z > -158.3);

    // Check the other two restored stations as well. Strictly rediscover every
    // concrete entity after each load because runtime IDs are not stable.
    await game.player.teleport({
      x: 43.9006,
      y: -11.556,
      z: -168.2003,
    });
    await game.step({ frames: 30 });
    await saveLoad(game, "bottom");
    await assertLiftAt(game, BOTTOM_NODE_Y, "loaded bottom station");
    await squeeze(
      game,
      await object(game, 480, "loaded bottom button mission object 480"),
    );
    await game.step({ frames: 600 });
    await assertLiftAt(game, MIDDLE_NODE_Y, "bottom station should cycle to middle");

    await saveLoad(game, "middle");
    await assertLiftAt(game, MIDDLE_NODE_Y, "loaded middle station");
    await squeeze(
      game,
      await object(game, 481, "loaded middle button mission object 481"),
    );
    await game.step({ frames: 600 });
    const finalLift = await assertLiftAt(
      game,
      TOP_NODE_Y,
      "middle station should cycle to top",
    );

    t.diagnostic(
      `Cargo lift save/load: top player ${JSON.stringify(topPlayer)} -> ` +
        `bottom lift ${JSON.stringify(bottomLift.position)}, player ` +
        `${JSON.stringify(bottomPlayer)}, north exit ${JSON.stringify(bottomExit)}, ` +
        `final top lift ${JSON.stringify(finalLift.position)}`,
    );
  },
);
