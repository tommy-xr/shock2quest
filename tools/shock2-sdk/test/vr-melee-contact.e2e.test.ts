import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

// Production regression for #942. This deliberately uses a shipped Wrench and
// shipped one-HP panes in command2: no debug spawn, damage message, or mission
// runtime id is involved. Runtime ids shuffle, so every object is discovered
// by its stable mission template id.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const WRENCH_MISSION_ID = 786;
const BREAKABLE_PANE_MISSION_ID = 82;
const DROP_PANE_MISSION_ID = 1477;

async function byMissionId(
  game: GameServer,
  name: string,
  missionId: number,
): Promise<EntitySummary> {
  const entity = (
    await game.entities.list({ filter: name, limit: 20 })
  ).entities.find((candidate) => candidate.template_id === missionId);
  assert.ok(entity, `expected ${name} mission object ${missionId}`);
  return entity;
}

// `right_hand.position` is pawn-local and the held grip sits 0.4 behind the
// hand, so the wrench's world z is `player.z + local_z - 0.4`. Staging is
// expressed relative to the pane rather than as bare magic coordinates.
const HAND_Y = 1.03;
const HAND_REST: Vec3 = [0.55, HAND_Y, 1.1];
const HAND_SWEEP: Vec3[] = [
  [0.5, HAND_Y, 1.4],
  [0.42, HAND_Y, 1.7],
  [0.34, HAND_Y, 2.0],
  [0.26, HAND_Y, 2.3],
  [0.18, HAND_Y, 2.6],
  [0.1, HAND_Y, 2.8],
  [0.05, HAND_Y, 3.0],
];

async function sweepHeldWrench(game: GameServer): Promise<void> {
  for (const hand of HAND_SWEEP) {
    await game.input.set("right_hand.position", hand);
    await game.step({ frames: 1 });
  }
}

async function paneStillExists(game: GameServer, runtimeId: number): Promise<boolean> {
  return (
    await game.entities.list({ filter: "Window 2", limit: 20 })
  ).entities.some((entity) => entity.id === runtimeId);
}

test(
  "VR authored melee damages only during a trigger-gated physical contact window",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8138),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const wrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);
    const pane = await byMissionId(
      game,
      "Window 2",
      BREAKABLE_PANE_MISSION_ID,
    );

    // Pick up the authored world Wrench through VirtualHand's production
    // selection ray. Its authored contact body must remain live while held.
    await game.player.teleport({
      x: wrench.position[0],
      y: wrench.position[1] - 1.4,
      z: wrench.position[2] + 1.2,
    });
    // Aim with the SDK look-at helper, not raw `head.look`: it composes pawn
    // rotation with the real eye height from `/v1/info`, so the subject is
    // genuinely in frame. Hand-tuned yaw/pitch is how this scenario previously
    // produced screenshots that framed neither the wrench nor the pane.
    await game.input.lookAtWorldPoint(wrench.position);
    await game.input.set("right_hand.position", [0, 1.4, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.id,
      "the real authored Wrench should be grabbed",
    );

    // Stage square in front of the issue's real one-HP pane, far enough back
    // that the pane, the swing and the alcove behind it are all in frame.
    await game.player.teleport({
      x: pane.position[0],
      y: pane.position[1] - 1.04,
      z: pane.position[2] - 2.6,
    });
    await game.step({ frames: 2 });
    await game.input.lookAtWorldPoint(pane.position);
    await game.input.set("right_hand.position", HAND_REST);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 1);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });

    // Negative first: physically sweep through the pane without pulling the
    // trigger. Before the fix the authored Wrench had no collision damage at
    // all; an always-hot workaround would incorrectly destroy it here.
    await sweepHeldWrench(game);
    assert.equal(
      await paneStillExists(game, pane.id),
      true,
      "idle held-Wrench contact must remain harmless",
    );

    // Leave contact, open the attack window with the production VR trigger,
    // then make a fresh controller-driven physics contact.
    await game.input.set("right_hand.position", HAND_REST);
    await game.step({ frames: 5 });
    const beforeAttack =
      (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await sweepHeldWrench(game);

    const attackMessages = (await game.messages.recent()).messages.filter(
      (message) => message.sequence > beforeAttack && message.to.entity_id === pane.id,
    );
    assert.ok(
      attackMessages.some((message) => message.payload === "Damage"),
      `trigger + physical contact should damage the pane: ${JSON.stringify(attackMessages)}`,
    );
    assert.ok(
      attackMessages.some((message) => message.payload === "Slay"),
      `the one-HP pane should break: ${JSON.stringify(attackMessages)}`,
    );
    assert.equal(
      await paneStillExists(game, pane.id),
      false,
      "the armed wrench contact should remove the pane",
    );

    // A drop closes the window and restores the Wrench's ordinary dynamic
    // loose-prop body. Drop it while overlapping a second authored pane: the
    // physical contact remains real, but it must not deal melee damage.
    const dropPane = await byMissionId(
      game,
      "Window 2",
      DROP_PANE_MISSION_ID,
    );
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    await game.player.teleport({
      x: dropPane.position[0] + 2,
      y: dropPane.position[1] - 1.6,
      z: dropPane.position[2] + 0.4,
    });
    await game.input.set("right_hand.position", [-2, 1.6, 0]);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 10 });

    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      null,
      "the Wrench should leave the hand",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: wrench.id })).bodies[0]?.body_type,
      "dynamic",
      "a dropped Wrench should regain ordinary loose-prop physics",
    );
    assert.equal(
      await paneStillExists(game, dropPane.id),
      true,
      "drop contact must remain harmless",
    );
  },
);
