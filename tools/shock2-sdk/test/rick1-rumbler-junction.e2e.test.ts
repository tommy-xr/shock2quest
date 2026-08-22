import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Position } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const reproSave = process.env.SHOCK2_RUMBLER_JUNCTION_SAVE;

function property(
  detail: { properties: Array<{ name: string; value: unknown }> },
  name: string,
): unknown {
  return detail.properties.find((item) => item.name === name)?.value;
}

async function walkTo(
  game: GameServer,
  target: [number, number],
  label: string,
  tolerance = 0.4,
): Promise<Position> {
  let position = await game.player.position();
  let best = Math.hypot(position.x - target[0], position.z - target[1]);
  let stalledFrames = 0;
  await game.input.set("right_hand.thumbstick", [0, 1]);
  for (let frame = 0; frame < 360; frame += 1) {
    if (frame % 6 === 0) {
      await game.input.lookAtWorldPoint([
        target[0],
        position.y + 0.8,
        target[1],
      ]);
    }
    await game.step({ frames: 1 });
    position = await game.player.position();
    const distance = Math.hypot(
      position.x - target[0],
      position.z - target[1],
    );
    if (distance <= tolerance) break;
    if (distance < best - 0.02) {
      best = distance;
      stalledFrames = 0;
    } else {
      stalledFrames += 1;
    }
    assert.ok(
      stalledFrames < 60,
      `${label} stalled at ${JSON.stringify(position)} (${distance.toFixed(3)} from target)`,
    );
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  assert.ok(
    Math.hypot(position.x - target[0], position.z - target[1]) <= tolerance,
    `${label} did not reach ${JSON.stringify(target)}: ${JSON.stringify(position)}`,
  );
  return position;
}

// Rick1's final-Rumbler lure uses the shipped WALK|SMALL_CREATURE link from
// AIPATH cell1408 to cell1400. The live Rumbler authored two 1.5-foot physics
// spheres, but the old runtime replaced that envelope with the five-foot-wide
// animation bounds. Its 2wu-wide capsule therefore oscillated forever at the
// junction instead of following its complete route back to Ladder951.
//
// The save is private campaign state and is supplied only to local/opt-in
// runs; it is never checked in or uploaded. This scenario uses ordinary player
// control throughout: acquire the Rumbler, reverse the intact route, require
// its real body to pass the former waypoint and reach the ladder lip, descend
// Ladder951, and land a real Crystal Shard edge without losing any of 24HP.
test(
  "rick1: authored Rumbler body follows the lure through the narrow junction",
  {
    skip: !e2eEnabled || !reproSave || !existsSync(reproSave),
    timeout: 900_000,
  },
  async () => {
    assert.ok(
      reproSave,
      "SHOCK2_RUMBLER_JUNCTION_SAVE must name the private repro save",
    );
    await using game = await GameServer.launch({
      mission: "rick1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8290),
      debugFlags: ["--save-file", reproSave],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
      repoRoot: process.env.SHOCK2_REPO_ROOT,
    });
    await game.step({ frames: 10 });

    const [rumbler] = await game.entities.byTemplate(2188);
    assert.ok(rumbler, "rick1 should contain authored Rumbler2188");
    const initial = await game.entities.detail(rumbler.id);
    assert.equal(property(initial, "HitPoints"), "220");
    assert.equal((await game.info()).player.hit_points, 24);
    const body = (await game.physics.bodies({ entityId: rumbler.id })).bodies[0];
    assert.ok(body, "Rumbler2188 must retain a live physics body");
    assert.equal(body.body_type, "dynamic");
    assert.equal(body.blocks_player, true);
    assert.equal(body.blocks_actor, true);
    assert.ok(
      body.collision_groups.includes("actor"),
      `Rumbler must remain in the actor collision group: ${JSON.stringify(body)}`,
    );

    const route: Array<[number, number]> = [
      [113.6, -1.6],
      [113.6, -2.4],
      [112.4, -4.4],
      [111.7, -9.2],
      [111.43, -10.02],
    ];
    for (let index = 0; index < route.length; index += 1) {
      await walkTo(game, route[index], `outbound lure leg ${index}`);
    }

    let acquired = false;
    for (let frame = 0; frame < 600; frame += 10) {
      await game.step({ frames: 10 });
      const player = await game.player.position();
      const detail = await game.entities.detail(rumbler.id);
      const distance = Math.hypot(
        detail.position[0] - player.x,
        detail.position[2] - player.z,
      );
      if (
        ["Moderate", "High"].includes(
          String(property(detail, "AIAlertness") ?? ""),
        ) && distance < 2.5
      ) {
        acquired = true;
        break;
      }
    }
    assert.ok(acquired, "the intact lure must acquire Rumbler2188");

    for (let index = route.length - 2; index >= 0; index -= 1) {
      await walkTo(game, route[index], `return lure leg ${index}`);
    }
    await walkTo(game, [111.91, -0.812], "Ladder951 midpoint", 0.4);

    let passedJunction = false;
    let reachedLip = false;
    let lastRumbler = initial;
    for (let frame = 0; frame < 720; frame += 5) {
      await game.step({ frames: 5 });
      lastRumbler = await game.entities.detail(rumbler.id);
      if (lastRumbler.position[2] > -9.5) passedJunction = true;
      const player = await game.player.position();
      const distance = Math.hypot(
        lastRumbler.position[0] - player.x,
        lastRumbler.position[2] - player.z,
      );
      if (lastRumbler.position[2] > -3.2 && distance < 3.0) {
        reachedLip = true;
        break;
      }
    }
    const livePath = (await game.pathfinding.aiPaths()).find(
      (path) => path.entity_id === rumbler.id,
    );
    assert.ok(
      passedJunction,
      `Rumbler must advance past the former cell1408->1400 stall: ` +
        `${JSON.stringify(lastRumbler.position)}, path=${JSON.stringify(livePath)}`,
    );
    assert.ok(
      reachedLip,
      `Rumbler must follow the lure to Ladder951, not merely pass one waypoint: ` +
        `${JSON.stringify(lastRumbler.position)}, path=${JSON.stringify(livePath)}`,
    );

    let player = await game.player.position();
    await game.input.lookAtWorldPoint([player.x - 10, player.y - 17.32, player.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    for (let frame = 0; frame < 300; frame += 1) {
      await game.step({ frames: 1 });
      player = await game.player.position();
      if (player.y < 77.4) break;
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(
      player.y < 77.4 && player.x < 111.2,
      `ordinary WEST/down input must acquire and descend Ladder951: ${JSON.stringify(player)}`,
    );
    assert.equal((await game.info()).player.hit_points, 24);

    // One real attack edge is enough to prove the post-fix lure is tactically
    // usable without turning this controller regression into a brittle 37-hit
    // combat-animation test. The campaign replay owns the complete 220HP kill.
    await game.input.trigger("EquipCrystalShard");
    await game.step({ frames: 5 });
    player = await game.player.position();
    await game.input.lookAtWorldPoint([player.x - 10, player.y + 17.32, player.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    for (let frame = 0; frame < 180; frame += 1) {
      await game.step({ frames: 1 });
      player = await game.player.position();
      if (player.y > 80.1) break;
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.player.aimAt(rumbler.id);
    const beforeHit = Number(property(await game.entities.detail(rumbler.id), "HitPoints"));
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    const afterHit = Number(property(await game.entities.detail(rumbler.id), "HitPoints"));
    assert.equal(
      beforeHit,
      220,
      "the controller fix must not alter the Rumbler's authored health",
    );
    assert.equal(
      afterHit,
      214,
      `one real Crystal Shard edge must deal exactly 6HP: ${beforeHit} -> ${afterHit}`,
    );

    player = await game.player.position();
    await game.input.lookAtWorldPoint([player.x - 10, player.y - 17.32, player.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    for (let frame = 0; frame < 180; frame += 1) {
      await game.step({ frames: 1 });
      player = await game.player.position();
      if (player.y <= 75.8) break;
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(
      player.y <= 75.8,
      `the player must retreat down Ladder951 after the strike: ${JSON.stringify(player)}`,
    );
    await game.step({ frames: 30 });
    assert.equal(
      (await game.info()).player.hit_points,
      24,
      "the player must remain safely separated after executing the ladder lure",
    );
  },
);
