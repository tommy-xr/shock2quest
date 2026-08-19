import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, lookQuat } from "../src/index.js";
import type {
  EntityDetailResult,
  EntitySummary,
  PhysicsBodySummary,
  Vec3,
} from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Production regression for #978. The MedSci1 Wrench is a concrete mission
// object (990), but its authored melee class is the gamesys Wrench (-928).
// Carrying it through save/load and a deck transition exercises the exact
// identity split that made the campaign weapon physically shove creatures
// without sending Damage.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8585);

const MEDSCI1_WRENCH = 990;
const WRENCH_CORPSE = 1177;
const WRENCH_ARCHETYPE = -928;
const MONKEY = 543;
const PIPE_HYBRID = 1293;
const SHOTGUN_HYBRID = 1392;
const BREAKABLE_PANE = 237;
const PANEL_SIZE_PX: Vec3 = [188, 296, 0];
const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;

const add = (a: Vec3, b: Vec3): Vec3 => a.map((x, i) => x + b[i]) as Vec3;
const sub = (a: Vec3, b: Vec3): Vec3 => a.map((x, i) => x - b[i]) as Vec3;
const scale = (a: Vec3, factor: number): Vec3 =>
  a.map((x) => x * factor) as Vec3;
const dot = (a: number[], b: number[]): number =>
  a.reduce((sum, x, i) => sum + x * b[i], 0);
const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const norm = (a: Vec3): Vec3 => scale(a, 1 / Math.sqrt(dot(a, a)));
const qnorm = (q: number[]): [number, number, number, number] =>
  q.map((value) => value / Math.sqrt(dot(q, q))) as [
    number,
    number,
    number,
    number,
  ];
const qconj = ([x, y, z, w]: number[]): [number, number, number, number] => [
  -x,
  -y,
  -z,
  w,
];
function qmul(a: number[], b: number[]): [number, number, number, number] {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}
function qrotate(q: number[], v: Vec3): Vec3 {
  return qmul(qmul(q, [...v, 0]), qconj(q)).slice(0, 3) as Vec3;
}
function qFromTo(from: Vec3, to: Vec3): [number, number, number, number] {
  const a = norm(from);
  const b = norm(to);
  const d = dot(a, b);
  if (d < -0.999999) {
    const axis =
      Math.abs(a[0]) < 0.9
        ? norm(cross(a, [1, 0, 0]))
        : norm(cross(a, [0, 1, 0]));
    return [axis[0], axis[1], axis[2], 0];
  }
  return qnorm([...cross(a, b), 1 + d]);
}

async function byMissionId(
  game: GameServer,
  name: string,
  missionId: number,
): Promise<EntitySummary> {
  const entity = (
    await game.entities.list({ filter: name, limit: 30 })
  ).entities.find((candidate) => candidate.template_id === missionId);
  assert.ok(entity, `expected ${name} mission object ${missionId}`);
  return entity;
}

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find(
    (candidate) => candidate.name === "HitPoints",
  );
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

// Hand-local vector from the tracked hand to the held weapon's contact
// collider, measured live (`measureHeldContactOffset`) rather than hardcoded:
// it is the weapon's VR grip, and the melee `_h` wield moved it from "0.4
// along the fingers" to "on the rendered weapon head". The gesture below is
// about where the *weapon* is, so it must follow whatever the grip says.
let heldContactOffset: Vec3 = [0, 0, 0];

async function measureHeldContactOffset(game: GameServer): Promise<void> {
  const held = (await game.info()).player.right_hand_entity_id;
  assert.ok(held, "a weapon must be held before its contact offset is measured");
  const local: Vec3 = [0, 1.0, 0];
  await game.input.set("right_hand.position", local);
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 2 });
  const body = (await game.physics.bodies({ entityId: held })).bodies[0];
  assert.ok(body, "the held weapon must have a contact body");
  const info = await game.info();
  const handWorld = add(
    info.player.position,
    qrotate(info.player.rotation, local),
  );
  heldContactOffset = qrotate(
    qconj(info.player.rotation),
    sub(body.position, handWorld),
  );

  // Staging follows this measurement, so assert its shape or the gesture would
  // silently compensate for a misplaced collider and still pass. The wielded
  // Wrench's contact volume belongs out on the rendered weapon head - roughly
  // 0.8 units from the palm, standing up out of the fist. A collider left in
  // the hand (the pre-`_h` melee grips) or parked along the fingers (the old
  // 0.4 wrench grip) fails here.
  const reach = Math.sqrt(dot(heldContactOffset, heldContactOffset));
  assert.ok(
    reach > 0.6 && reach < 1.0,
    `the Wrench's contact volume should sit on its head, ~0.8 from the hand: ${JSON.stringify(heldContactOffset)}`,
  );
  assert.ok(
    heldContactOffset[1] > 0.9 * reach,
    `the Wrench's contact volume should stand up out of the fist: ${JSON.stringify(heldContactOffset)}`,
  );
}

// This is the production play-through gesture: orient the physical right hand
// along the eye-to-target ray, wind up two units away, pull the trigger, and
// sweep the weapon's contact volume to 0.33 units. No damage/script message is
// injected by the test.
async function poseWrench(
  game: GameServer,
  target: Vec3,
  distance: number,
  frames = 4,
): Promise<void> {
  const info = await game.info();
  const pawn = info.player.position;
  const pawnQ = info.player.rotation;
  const eye = add(pawn, [0, info.player.camera_offset[1], 0]);
  const toward = norm(sub(target, eye));
  // `lookQuat`, not the file's shortest-arc `qFromTo`: the shortest arc from -Z
  // has no roll control and rolls up to ~180 degrees for directions near world
  // +z - which is exactly where these staged targets sit. That was cosmetic
  // while the rotation only spun the hand; now it also carries the ~0.8-unit
  // contact offset below, where a spurious roll turns "up out of the fist"
  // into "sideways" and the swing misses for reasons nothing in the test says.
  const worldQ = lookQuat(toward);
  // `distance` is the contact volume's distance from the target, so back the
  // hand off by wherever the grip puts that volume.
  const worldHand = sub(
    sub(target, scale(toward, distance)),
    qrotate(worldQ, heldContactOffset),
  );
  const localPosition = qrotate(qconj(pawnQ), sub(worldHand, pawn));
  const localRotation = qnorm(qmul(qconj(pawnQ), worldQ));

  await game.input.lookAtWorldPoint(target, {
    eyeHeight: info.player.camera_offset[1],
  });
  await game.input.set("right_hand.position", localPosition);
  await game.input.set("right_hand.rotation", localRotation);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames });
}

async function bodyFor(
  game: GameServer,
  entityId: number,
): Promise<PhysicsBodySummary> {
  const body = (await game.physics.bodies({ entityId })).bodies[0];
  assert.ok(body, `entity ${entityId} should have one primary physics body`);
  return body;
}

const distance = (a: Vec3, b: Vec3): number =>
  Math.sqrt(a.reduce((sum, value, index) => sum + (value - b[index]) ** 2, 0));

function assertBoundedActor(
  before: PhysicsBodySummary,
  after: PhysicsBodySummary,
  label: string,
  bounds: { displacement: number; speed: number } = {
    displacement: 8,
    speed: 10,
  },
): void {
  assert.ok(
    [...after.position, ...after.velocity].every(Number.isFinite),
    `${label}: actor physics must stay finite: ${JSON.stringify(after)}`,
  );
  assert.ok(
    distance(after.position, before.position) < bounds.displacement,
    `${label}: held Wrench displaced the actor out of its lane: ${JSON.stringify(
      {
        before: before.position,
        after: after.position,
      },
    )}`,
  );
  assert.ok(
    Math.max(...after.velocity.map(Math.abs)) < bounds.speed,
    `${label}: held Wrench gave the actor runaway linear velocity: ${JSON.stringify(after.velocity)}`,
  );
}

async function damageMessagesSince(
  game: GameServer,
  sequence: number,
  targetId: number,
): Promise<number> {
  return (await game.messages.recent()).messages.filter(
    (message) =>
      message.sequence > sequence &&
      message.to.entity_id === targetId &&
      message.payload === "Damage",
  ).length;
}

async function armedSweep(
  game: GameServer,
  target: EntitySummary,
): Promise<{ sequence: number; targetId: number; targetPoint: Vec3 }> {
  const live = await game.entities.detail(target.id);
  await game.player.teleport({
    x: live.position[0] + 0.815,
    y: live.position[1] + 0.236,
    z: live.position[2] - 2.245,
  });
  await game.entities.sendMessage(target.id, {
    type: "SetAlertness",
    level: "Lowest",
  });
  await game.step({ frames: 2 });

  const afterTeleport = await game.entities.detail(target.id);
  const targetPoint =
    afterTeleport.aim_points?.find((point) => point.classification === "torso")
      ?.position ?? afterTeleport.position;
  await poseWrench(game, targetPoint, 4);
  await game.step({ frames: 4 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  const sequence =
    (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await poseWrench(game, targetPoint, 0.33);
  return { sequence, targetId: target.id, targetPoint };
}

// The #984 report/review's live-torso incremental approach, with one necessary
// correction: the Wrench's full model bounds can already overlap the actor at
// a 1.5-unit hand-origin pose. Arm from a measured-clear four-unit pose, then
// reacquire the moving torso for every 1.5 -> 1.0 -> 0.7 -> 0.4 -> 0.2 step.
async function reviewedIncrementalSweep(
  game: GameServer,
  target: EntitySummary,
): Promise<{ sequence: number; targetId: number; targetPoint: Vec3 }> {
  const live = await game.entities.detail(target.id);
  await game.player.teleport({
    x: live.position[0] + 0.815,
    y: live.position[1] + 0.236,
    z: live.position[2] - 2.245,
  });
  await game.entities.sendMessage(target.id, {
    type: "SetAlertness",
    level: "Lowest",
  });
  await game.input.set("right_hand.trigger", 0);
  let targetPoint =
    live.aim_points?.find((point) => point.classification === "torso")
      ?.position ?? live.position;
  await poseWrench(game, targetPoint, 4, 4);

  const sequence =
    (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  for (const handDistance of [1.5, 1.0, 0.7, 0.4, 0.2]) {
    const current = await game.entities.detail(target.id);
    targetPoint =
      current.aim_points?.find((point) => point.classification === "torso")
        ?.position ?? current.position;
    await poseWrench(game, targetPoint, handDistance, 1);
  }
  await game.step({ frames: 2 });
  return { sequence, targetId: target.id, targetPoint };
}

async function releaseTrigger(
  game: GameServer,
  targetPoint?: Vec3,
): Promise<void> {
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  if (targetPoint) {
    await poseWrench(game, targetPoint, 4);
  }
}

async function grabAuthoredCorpseWrench(
  game: GameServer,
): Promise<EntitySummary> {
  const corpse = await byMissionId(game, "MS Male Corpse", WRENCH_CORPSE);
  const corpseDetail = await game.entities.detail(corpse.id);
  const contained = corpseDetail.outgoing_links.filter((link) =>
    link.link_type.startsWith("Contains"),
  );
  assert.equal(
    contained.length,
    1,
    "corpse 1177 should contain only its Wrench",
  );
  assert.match(contained[0].target_name, /Wrench/i);
  const wrenchId = contained[0].target_id;

  await teleportVerified(game, {
    x: corpse.position[0] + 1.2,
    y: corpse.position[1] + 0.5,
    z: corpse.position[2],
  });
  await game.step({ frames: 5 });
  const aim = await game.player.aimAt(corpse, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(aim.interaction_target_id, corpse.id);
  await aimVrHandAt(game, aim.world_point);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 5 });

  const uiBodies = (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );
  assert.equal(uiBodies.length, 1, "corpse frob should open one VR loot panel");
  const panel = uiBodies[0];
  const ui = (await game.ui.state()).active_panel;
  assert.ok(ui, "corpse frob should expose its rendered loot panel");
  const wrenchElement = ui.elements.find(
    (element) => element.entity_id === wrenchId,
  );
  assert.ok(wrenchElement, "corpse panel should render Wrench 990");
  const panelFront = qrotate(panel.rotation, [0, 0, -1]);
  const eyeHeight = (await game.info()).player.camera_offset[1];
  await teleportVerified(game, {
    x: panel.position[0] + panelFront[0] * 1.25,
    y: panel.position[1] - eyeHeight,
    z: panel.position[2] + panelFront[2] * 1.25,
  });
  await game.step({ frames: 3 });
  const panelSize: Vec3 = [
    PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
    PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
    0,
  ];
  const [slotX, slotY, slotWidth, slotHeight] = wrenchElement.rect;
  const u = (slotX + slotWidth / 2) / PANEL_SIZE_PX[0];
  const v = (slotY + slotHeight / 2) / PANEL_SIZE_PX[1];
  const localSlot: Vec3 = [
    panelSize[0] * (0.5 - u),
    panelSize[1] * (0.5 - v),
    0,
  ];
  const slotWorld = add(panel.position, qrotate(panel.rotation, localSlot));
  const panelAim = await aimVrHandAt(game, slotWorld, 0.35);
  const panelHit = await game.raycast({
    start: panelAim.start,
    end: panelAim.target,
    collision_groups: ["ui"],
    max_distance: 1,
  });
  assert.equal(
    panelHit.entity_id,
    panel.entity_id,
    "hand ray should hit Wrench's slot",
  );
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 10 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    wrenchId,
    "squeezing the rendered loot slot should physically grab Wrench 990",
  );

  return byMissionId(game, "Wrench", MEDSCI1_WRENCH);
}

async function assertNineDamagePull(
  game: GameServer,
  name: string,
  missionId: number,
  initialHp: number,
): Promise<void> {
  const target = await byMissionId(game, name, missionId);
  assert.equal(
    hitPoints(await game.entities.detail(target.id)),
    initialHp,
    `${name} must start at its authored HP`,
  );

  const { sequence, targetId, targetPoint } = await armedSweep(game, target);
  assert.equal(
    await damageMessagesSince(game, sequence, targetId),
    1,
    `one pull must emit exactly one Damage to ${name}`,
  );
  assert.equal(
    hitPoints(await game.entities.detail(targetId)),
    initialHp - 9,
    `${name} must take exactly 9 HP during the four-frame contact pose`,
  );
  await releaseTrigger(game, targetPoint);
}

async function killShotgunWithThreeBoundedPulls(
  game: GameServer,
): Promise<void> {
  const target = await byMissionId(game, "OG-Shotgun", SHOTGUN_HYBRID);
  assert.equal(hitPoints(await game.entities.detail(target.id)), 24);

  for (const [hit, expectedHp] of [15, 6, 0].entries()) {
    const beforeBody = await bodyFor(game, target.id);
    const { sequence, targetId, targetPoint } = await reviewedIncrementalSweep(
      game,
      target,
    );
    assert.equal(
      await damageMessagesSince(game, sequence, targetId),
      1,
      `shotgun pull ${hit + 1} must emit exactly one Damage`,
    );

    if (expectedHp > 0) {
      assert.equal(
        hitPoints(await game.entities.detail(targetId)),
        expectedHp,
        `same OG-Shotgun must take one authored 9-HP hit on pull ${hit + 1}`,
      );
      assertBoundedActor(
        beforeBody,
        await bodyFor(game, targetId),
        `pull ${hit + 1}`,
      );
    } else {
      const fatalDetail = await game.entities.detail(targetId);
      assert.ok(
        hitPoints(fatalDetail) <= 0,
        "the third normal pull must exhaust the same original OG-Shotgun's HP",
      );
      assertBoundedActor(
        beforeBody,
        await bodyFor(game, targetId),
        `lethal pull ${hit + 1}`,
      );
    }

    await releaseTrigger(game, targetPoint);
    await game.step({ frames: 120 });
    const settledBodies = (await game.physics.bodies({ entityId: targetId }))
      .bodies;
    if (expectedHp > 0) {
      assert.equal(
        settledBodies.length,
        1,
        "the live target must remain in the mission",
      );
      assertBoundedActor(
        beforeBody,
        settledBodies[0],
        `pull ${hit + 1} after 120 ordinary frames`,
        { displacement: 64, speed: 64 },
      );
    } else {
      const corpse = await game.entities.detail(targetId);
      assert.equal(
        corpse.properties.find((property) => property.name === "AIBehavior")
          ?.value,
        "Dead",
        "the third normal pull must leave the same original OG-Shotgun dead",
      );
      assert.equal(
        settledBodies.length,
        1,
        "the dead OG-Shotgun should retain its lootable corpse body",
      );
      assert.deepEqual(settledBodies[0].collision_groups, ["selectable"]);
      assert.equal(settledBodies[0].blocks_actor, false);
      assertBoundedActor(
        beforeBody,
        settledBodies[0],
        "lethal pull after 120 ordinary frames",
        { displacement: 64, speed: 64 },
      );
    }
  }
}

test(
  "MedSci saved mission Wrench keeps canonical 9-damage VR melee across decks and saves",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Fresh canonical control: the same physical gesture against the same
    // authored Monkey establishes the expected 9-HP baseline independently.
    {
      await using control = await GameServer.launch({
        mission: "medsci2.mis",
        port: basePort + 1,
        debugFlags: ["--vr"],
        echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
      });
      const freshWrench = await control.player.spawnItem(WRENCH_ARCHETYPE);
      await control.input.set("right_hand.squeeze", 1);
      await control.input.trigger("EquipWrench");
      await control.step({ frames: 3 });
      assert.equal(
        (await control.info()).player.right_hand_entity_id,
        freshWrench.entity_id,
        "fresh canonical control Wrench should be wielded",
      );
      await measureHeldContactOffset(control);
      const fresh = await byMissionId(control, "Blue Monkey", MONKEY);
      const { sequence, targetId } = await armedSweep(control, fresh);
      assert.equal(
        await damageMessagesSince(control, sequence, targetId),
        1,
        "fresh Wrench should emit one Damage message",
      );
      assert.equal(hitPoints(await control.entities.detail(targetId)), 1);
      await releaseTrigger(control);
    }

    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    // Preserve the exact positive mission identity that exposed #978 by
    // opening corpse 1177's physical VR panel and grabbing Wrench 990.
    const worldWrench = await grabAuthoredCorpseWrench(game);
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      worldWrench.id,
    );
    await measureHeldContactOffset(game);

    // The suite's save cleaner keys off a trailing 13-digit epoch, so every
    // name this test writes must end with `stamp`.
    const stamp = Date.now();
    const saveName = `medsci_saved_vr_melee_${stamp}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 3 });
    let restoredWrench = await byMissionId(game, "Wrench", MEDSCI1_WRENCH);
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      restoredWrench.id,
    );

    await game.transitionLevel("medsci2");
    await game.step({ frames: 5 });
    restoredWrench = await byMissionId(game, "Wrench", MEDSCI1_WRENCH);
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      restoredWrench.id,
      "the concrete MedSci1 Wrench should remain wielded on MedSci2",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: restoredWrench.id })).bodies[0]
        ?.body_type,
      "kinematic",
    );

    const combatBaseline = `medsci_saved_vr_melee_combat_baseline_${stamp}`;
    assert.equal((await game.save(combatBaseline)).success, true);
    await assertNineDamagePull(game, "Blue Monkey", MONKEY, 10);
    assert.equal((await game.load(combatBaseline)).success, true);
    await releaseTrigger(game);
    await assertNineDamagePull(game, "OG-Pipe", PIPE_HYBRID, 12);
    assert.equal((await game.load(combatBaseline)).success, true);
    await releaseTrigger(game);
    // #984's exact acceptance path: one shipped 24-HP target, three fully
    // released and separated physical pulls, no replacement/reload between
    // hits, and two seconds of ordinary simulation after every contact.
    await killShotgunWithThreeBoundedPulls(game);

    // A second save/load proves the canonical identity is not a one-transition
    // accident. A one-HP authored world pane is also a non-creature control.
    const roundTripSave = `medsci_saved_vr_melee_again_${stamp}`;
    assert.equal((await game.save(roundTripSave)).success, true);
    assert.equal((await game.load(roundTripSave)).success, true);
    await game.step({ frames: 3 });
    restoredWrench = await byMissionId(game, "Wrench", MEDSCI1_WRENCH);
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      restoredWrench.id,
    );

    const pane = await byMissionId(game, "Window 2", BREAKABLE_PANE);
    const { sequence, targetId, targetPoint } = await armedSweep(game, pane);
    assert.equal(await damageMessagesSince(game, sequence, targetId), 1);
    assert.equal(
      (
        await game.entities.list({ filter: "Window 2", limit: 30 })
      ).entities.some((entity) => entity.id === targetId),
      false,
      "restored Wrench contact should still destroy an authored world pane",
    );
    await releaseTrigger(game, targetPoint);

    // Drop through the real VR squeeze edge. The held-only solver filter must
    // leave with the old kinematic body: the recreated world Wrench is again a
    // normal dynamic, actor-solid loose prop and remains finite under gravity.
    await game.input.set("right_hand.position", [0, 0.6, -0.6]);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    const dropped = await bodyFor(game, restoredWrench.id);
    assert.equal(dropped.body_type, "dynamic");
    assert.equal(dropped.blocks_actor, true);
    assert.equal(dropped.is_sensor, false);
    await game.step({ frames: 120 });
    const droppedSettled = await bodyFor(game, restoredWrench.id);
    assert.ok(
      [...droppedSettled.position, ...droppedSettled.velocity].every(
        Number.isFinite,
      ),
      `dropped Wrench physics must remain finite: ${JSON.stringify(droppedSettled)}`,
    );
  },
);
