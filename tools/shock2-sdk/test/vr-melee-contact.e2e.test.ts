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
// Aim the weapon's fitted (handle-to-head) volume through the pane center. The
// old pose-teleported collider could skim the upper frame and still jump into
// the glass; a solver-driven body correctly stops on that frame instead.
const HAND_Y = 0.45;
// Retract far enough that the wrench's full fitted cuboid, not merely its
// body origin, clears the pane before the next contact edge.
const HAND_REST: Vec3 = [0.7, HAND_Y, 0.3];
const HAND_SWEEP_END: Vec3 = [0.05, HAND_Y, 3.4];
// 3.2 units in one second: a 3.2 u/s swing, clear of the free-swing gate
// (`melee_free_swing`, 2.0 world units/s).
const HAND_SWEEP_FRAMES = 60;

async function sweepHeldWrench(game: GameServer): Promise<void> {
  for (let frame = 1; frame <= HAND_SWEEP_FRAMES; frame += 1) {
    const t = frame / HAND_SWEEP_FRAMES;
    const hand: Vec3 = [
      HAND_REST[0] + (HAND_SWEEP_END[0] - HAND_REST[0]) * t,
      HAND_Y,
      HAND_REST[2] + (HAND_SWEEP_END[2] - HAND_REST[2]) * t,
    ];
    await game.input.set("right_hand.position", hand);
    // Advance the tracked target every simulation frame, like a real
    // controller sample. This matters now that the visible/contact weapon is
    // a spring-driven body instead of a pose-teleported kinematic.
    await game.step({ frames: 1 });
  }
}

async function paneStillExists(game: GameServer, runtimeId: number): Promise<boolean> {
  return (
    await game.entities.list({ filter: "Window 2", limit: 20 })
  ).entities.some((entity) => entity.id === runtimeId);
}

test(
  "a VR swing deals the Wrench's authored damage, and a dropped one does not",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
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
    await game.step({ frames: 30 });

    // Swing the wrench through the pane. The blow is billed on the swing's
    // own closing speed - there is no button to arm it - so what this proves
    // is that the authored WeaponBash reaches a shipped one-HP pane at all.
    // (That it is NOT billed for merely being carried into one is the walking
    // regression below.)
    const beforeAttack =
      (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
    await sweepHeldWrench(game);

    const sweptWrench = (await game.physics.bodies({ entityId: wrench.id }))
      .bodies[0];
    assert.ok(sweptWrench, "the held Wrench should retain its physics body");
    // The swept drive stops the weapon on WORLD geometry only. A breakable
    // pane is an entity, and deliberately does not block: a swing has to
    // travel *into* what it is hitting, and a pane you are meant to smash
    // would otherwise stop the swing dead. So the weapon reaches its tracked
    // target here.
    //
    // An earlier revision drove a fully dynamic body and asserted the
    // opposite (that the pane held the weapon back). That drive spun the
    // weapon out of the player's hand on any contact and was replaced.
    assert.ok(
      sweptWrench.position[2] > pane.position[2] - 0.1,
      `the swept Wrench should reach its tracked target through a non-world pane: ${JSON.stringify(sweptWrench)}`,
    );

    const attackMessages = (await game.messages.recent()).messages.filter(
      (message) => message.sequence > beforeAttack && message.to.entity_id === pane.id,
    );
    assert.ok(
      attackMessages.some((message) => message.payload === "Damage"),
      `a swung physical contact should damage the pane: ${JSON.stringify(attackMessages)}`,
    );
    assert.ok(
      attackMessages.some((message) => message.payload === "Slay"),
      `the one-HP pane should break: ${JSON.stringify(attackMessages)}`,
    );
    assert.equal(
      await paneStillExists(game, pane.id),
      false,
      "the swung wrench contact should remove the pane",
    );

    // A drop closes the window and restores the Wrench's ordinary dynamic
    // loose-prop body. Drop it while overlapping a second authored pane: the
    // physical contact remains real, but it must not deal melee damage.
    const dropPane = await byMissionId(
      game,
      "Window 2",
      DROP_PANE_MISSION_ID,
    );
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

// Regression for the free-swing measurement: a held weapon rides the player, so
// its world velocity is the player's own locomotion (measured at 10 units/s,
// five times the swing gate). Walking a still hand into a shipped one-HP pane
// therefore destroyed it - a free authored WeaponBash for walking down a
// corridor with a wrench out.
test(
  "walking a held wrench into a pane is not a swing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const wrench = await byMissionId(game, "Wrench", WRENCH_MISSION_ID);
    const pane = await byMissionId(game, "Window 2", BREAKABLE_PANE_MISSION_ID);

    await game.player.teleport({
      x: wrench.position[0],
      y: wrench.position[1] - 1.4,
      z: wrench.position[2] + 1.2,
    });
    await game.input.lookAtWorldPoint(wrench.position);
    await game.input.set("right_hand.position", [0, 1.4, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.id,
      "the real authored Wrench should be grabbed",
    );

    // Stand back from the pane with the wrench held out ahead, and hold the
    // hand perfectly still for the rest of the run: every unit the weapon
    // covers from here is the player walking, not the player swinging.
    await game.player.teleport({
      x: pane.position[0],
      y: pane.position[1] - 1.04,
      z: pane.position[2] - 5.0,
    });
    await game.input.lookAtWorldPoint(pane.position);
    await game.input.set("right_hand.position", [0.05, HAND_Y, 1.2]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.id,
      "the Wrench should still be held at the walk-in staging pose",
    );

    // Walk into it, then keep walking after the pane is reached - the frame
    // the player is stopped by the wall behind it is the awkward one.
    await game.input.set("right_hand.thumbstick", [0.0, 1.0]);
    await game.step({ frames: 90 });

    // The wrench must have been carried THROUGH the pane, or "nothing was
    // billed" would be satisfied by never touching it.
    const carriedWrench = (await game.physics.bodies({ entityId: wrench.id }))
      .bodies[0];
    assert.ok(carriedWrench, "the held Wrench should retain its physics body");
    assert.ok(
      carriedWrench.position[2] > pane.position[2],
      `the walk should carry the Wrench past the pane plane: ${JSON.stringify(carriedWrench)}`,
    );
    assert.equal(
      await paneStillExists(game, pane.id),
      true,
      "walking a still hand into the pane must not bill a swing",
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
    // swept contact body.
    const bodies = (await game.physics.bodies({ entityId: restoredWrench.id })).bodies;
    assert.equal(
      bodies[0]?.body_type,
      "kinematic",
      `a restored held melee weapon must keep its swept contact body: ${JSON.stringify(bodies)}`,
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
      "a rejected second-hand grab must leave the live swept melee body intact",
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
