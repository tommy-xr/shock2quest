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

// `right_hand.position` is pawn-local, and the held Wrench's contact volume
// sits wherever its VR grip puts it relative to the hand (for the melee `_h`
// wield, on the rendered weapon head - see `vr_config::melee_contact_offset`).
// Staging is expressed relative to the pane rather than as bare magic
// coordinates; the pane is tall enough that the sweep crosses it either way.
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
    // Two frames per sample, not one: the Wrench's authored contact volume is
    // a 4.6cm sphere (PropPhysDimensions radius0), and the sweep's last sample
    // lands it right on the pane plane. At one frame per sample whether the
    // contact is generated came down to how much physics free-ran between the
    // HTTP requests - the same swing passed or failed purely on request
    // latency. Two frames gives the contact a real overlap window instead of a
    // tangential touch; nothing else about the gesture changes.
    await game.step({ frames: 2 });
  }
}

async function sweepHeldWrenchAcrossXPane(game: GameServer): Promise<void> {
  for (const x of [-0.4, -0.8, -1.2, -1.6, -1.9, -2.1]) {
    await game.input.set("right_hand.position", [x, 1.6, 0]);
    await game.step({ frames: 2 });
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

// Regression for #955. TriggerPull is an edge, so an expired or rate-limited
// pull must never become armed later merely because the level stays held. The
// shipped one-HP panes let the sequence prove harmless contacts and the next
// accepted swing without relying on a debug damage shim.
test(
  "a held VR melee trigger expires and trigger chatter cannot rearm it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT_SWING_WINDOW ?? 8144),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const wrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);
    const pane = await byMissionId(
      game,
      "Window 2",
      BREAKABLE_PANE_MISSION_ID,
    );

    await game.player.teleport({
      x: wrench.position[0],
      y: wrench.position[1] - 1.4,
      z: wrench.position[2] + 1.2,
    });
    await game.input.lookAtWorldPoint(wrench.position);
    await game.input.set("right_hand.position", [0, 1.4, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);

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

    // Hold past the 11-frame authored contact duration before touching the
    // pane. This is the permanent-arm bug from the issue.
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 30 });
    await sweepHeldWrench(game);
    assert.equal(
      await paneStillExists(game, pane.id),
      true,
      "contact after the held-trigger attack window expires must be harmless",
    );

    // A release/re-pull still inside the 31-frame swing cadence is rejected.
    // Finishing that held contact after the cooldown expires must not arm it
    // late: only a newly accepted rising edge starts another swing.
    await game.input.set("right_hand.position", HAND_REST);
    await game.step({ frames: 10 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await sweepHeldWrench(game);
    assert.equal(
      await paneStillExists(game, pane.id),
      true,
      "trigger chatter inside one authored swing must not create another hit",
    );

    // After a complete release, stage at another authored one-HP pane. Using a
    // fresh physical target avoids asking Rapier to synthesize another
    // CollisionStarted edge for the thin pane we already crossed twice.
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    const freshPane = await byMissionId(
      game,
      "Window 2",
      DROP_PANE_MISSION_ID,
    );
    await game.player.teleport({
      x: freshPane.position[0] + 2,
      y: freshPane.position[1] - 1.6,
      z: freshPane.position[2] + 0.4,
    });
    await game.input.lookAtWorldPoint(freshPane.position);
    await game.input.set("right_hand.position", [0, 1.6, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 5 });
    const beforeFreshSwing =
      (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await sweepHeldWrenchAcrossXPane(game);
    const freshSwingMessages = (await game.messages.recent()).messages.filter(
      (message) => message.sequence > beforeFreshSwing,
    );
    assert.equal(
      await paneStillExists(game, freshPane.id),
      false,
      `the next accepted swing should damage a fresh pane: ${JSON.stringify(freshSwingMessages)}`,
    );
  },
);

// Regression for the #943 review: `set_held_melee` was reachable only from
// `VirtualHandEffect::HoldItem`, which only a fresh VR world grab emits.
// `restore_held_item_interaction` goes through `VrInteraction::grab` (which
// returns no effects) and then unconditionally strips held-item physics, so a
// wrench carried through a save/load came back with NO collider and silently
// never reported contacts again - the feature was dead on the path a player
// actually uses to carry a weapon.
test(
  "a melee weapon carried through a save/load keeps its contact body and still damages",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT_SAVE ?? 8139),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const wrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);

    await game.player.teleport({
      x: wrench.position[0],
      y: wrench.position[1] - 1.4,
      z: wrench.position[2] + 1.2,
    });
    await game.input.lookAtWorldPoint(wrench.position);
    await game.input.set("right_hand.position", [0, 1.4, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0);
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);

    await game.save("melee-holdthrough");
    await game.load("melee-holdthrough");
    await game.step({ frames: 5 });

    // Runtime ids are reassigned by the load, so rediscover everything.
    const restoredWrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      restoredWrench.id,
      "the Wrench should still be held after the load",
    );

    // The actual regression: the restored weapon must still have a live,
    // controller-driven contact body.
    const bodies = (await game.physics.bodies({ entityId: restoredWrench.id })).bodies;
    assert.equal(
      bodies[0]?.body_type,
      "kinematic",
      `a restored held melee weapon must keep its kinematic contact body: ${JSON.stringify(bodies)}`,
    );

    // ...and it must still actually damage an authored one-HP pane.
    const pane = await byMissionId(game, "Window 2", BREAKABLE_PANE_MISSION_ID);
    await game.player.teleport({
      x: pane.position[0],
      y: pane.position[1] - 1.04,
      z: pane.position[2] - 2.6,
    });
    await game.step({ frames: 2 });
    await game.input.lookAtWorldPoint(pane.position);
    await game.input.set("right_hand.position", HAND_REST);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await sweepHeldWrench(game);

    assert.equal(
      await paneStillExists(game, pane.id),
      false,
      "a wrench carried through a save/load should still break the pane",
    );
  },
);

// Regression for #954. Keeping the real held-melee collider from #943 made it
// selectable by both controller rays. The other hand could then acquire the
// same runtime entity; releasing either hand restored dynamic physics while
// the other continued hard-setting its transform every frame.
test(
  "a held VR melee weapon cannot hover or transfer into the other hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT_HELD_RAY ?? 8140),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const wrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);
    await game.player.teleport({
      x: wrench.position[0],
      y: wrench.position[1] - 1.4,
      z: wrench.position[2] + 1.2,
    });
    await game.input.lookAtWorldPoint(wrench.position);
    await game.input.set("right_hand.position", [0, 1.4, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0);
    // The authored Wrench's held collider sits 0.2 above the right-hand ray.
    // Put the left production ray through its center instead of using a debug
    // grab command; this is the exact dual-hand path reported in #954.
    await game.input.set("left_hand.position", [0, 1.6, 0]);
    await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 2 });

    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.right_hand_entity_id, wrench.id);

    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 2 });
    const afterRejectedGrab = await game.info();
    assert.equal(
      afterRejectedGrab.player.wielded_entity_id,
      null,
      "the left hand must not acquire the Wrench already held on the right",
    );
    assert.equal(afterRejectedGrab.player.right_hand_entity_id, wrench.id);
    assert.equal(
      (await game.physics.bodies({ entityId: wrench.id })).bodies[0]?.body_type,
      "kinematic",
      "a rejected second-hand grab must leave #943's live held-melee body intact",
    );

    await game.input.set("left_hand.squeeze", 0);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
    const afterRelease = await game.info();
    assert.equal(afterRelease.player.wielded_entity_id, null);
    assert.equal(afterRelease.player.right_hand_entity_id, null);
    assert.equal(
      (await game.physics.bodies({ entityId: wrench.id })).bodies[0]?.body_type,
      "dynamic",
      "the sole owner releasing the Wrench should restore ordinary loose-prop physics",
    );
  },
);
