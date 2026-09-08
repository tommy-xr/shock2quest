import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary, Quat, Vec3 } from "../src/types.js";

// The late-Engineering Floor Hatch is a real rotating door controlled by two
// ordinary BaseButtons:
//
//   button 130 ---SwitchLink---> Floor Hatch 85 <---SwitchLink--- button 152
//
// Object 85 also inherits a zero-valued P$TransDoor. RotDoor must take
// precedence, exactly as Looking Glass's GetDoorProperty did. Before #861 the
// unparsed P$RotDoor left StdDoor to initialize from the zero translation,
// moving both the hatch and its body to world origin.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const exactCampaignSave = process.env.SHOCK2_ROTDOOR_EXACT_SAVE;
const HATCH = 85;
const SECOND_SHIPPED_HATCH = 462;
const UPPER_CONTROL = 152;
const AUTHORED_CLOSED: Vec3 = [0.114864826, -12.75, -23.712574];
const SECOND_AUTHORED_CLOSED: Vec3 = [44.72176, -8.258417, -33.899513];

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

function rotationDistance(a: Quat, b: Quat): number {
  const dot = Math.abs(a.reduce((sum, value, axis) => sum + value * b[axis], 0));
  return 1 - Math.min(1, dot);
}

async function byMissionId(
  game: GameServer,
  missionId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = (await game.entities.list({ limit: 5_000 })).entities.filter(
    (entity) => entity.template_id === missionId,
  );
  assert.equal(matches.length, 1, `expected exactly one ${label} (${missionId})`);
  return matches[0];
}

async function hatchDetail(game: GameServer): Promise<EntityDetailResult> {
  return game.entities.detail((await byMissionId(game, HATCH, "Floor Hatch")).id);
}

async function pressUpperControl(game: GameServer): Promise<void> {
  const control = await byMissionId(game, UPPER_CONTROL, "upper hatch control");
  const link = (await game.entities.detail(control.id)).outgoing_links.find(
    (candidate) => candidate.link_type === "SwitchLink",
  );
  const hatch = await byMissionId(game, HATCH, "Floor Hatch");
  assert.equal(link?.target_id, hatch.id, "the real upper control must target Hatch 85");

  // Setup only: stand on the authored upper side. Interaction itself is the
  // production flat camera + ordinary squeeze rising edge, not a message.
  await game.player.teleport({ x: -3, y: -12, z: -22.3 });
  await game.step({ frames: 1 });
  const aim = await game.player.aimAt(control, {
    hitbox: "center",
    visibility: "required",
  });
  assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
  // The 95-degree swing completes in about 1.7 seconds. Stop well before the
  // hatch's authored 10-second DoorTimer closes it again.
  await game.step({ frames: 180 });
}

async function assertBodyFollows(game: GameServer, hatch: EntitySummary): Promise<void> {
  const body = (await game.physics.bodies({ entityId: hatch.id })).bodies;
  assert.equal(body.length, 1, "Floor Hatch should own one kinematic body");
  assert.ok(
    distance(body[0].position, hatch.position) < 0.01,
    `hatch body must follow its live transform: ${JSON.stringify(body[0].position)} vs ${JSON.stringify(hatch.position)}`,
  );
}

test(
  "eng1.mis: linked control rotates Floor Hatch 85 and open/closed saves restore",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8185),
    });
    await game.step({ frames: 30 });

    const closed = await hatchDetail(game);
    assert.ok(
      distance(closed.position, AUTHORED_CLOSED) < 0.01,
      `Hatch 85 must initialize at its authored shaft, got ${JSON.stringify(closed.position)}`,
    );
    await assertBodyFollows(game, await byMissionId(game, HATCH, "Floor Hatch"));

    // A second concrete shipped P$RotDoor must take precedence too. This
    // catches a Hatch-85-only exception even if the campaign blocker passes.
    const second = await byMissionId(game, SECOND_SHIPPED_HATCH, "second Floor Hatch");
    assert.ok(
      distance(second.position, SECOND_AUTHORED_CLOSED) < 0.01,
      `eng1 Hatch 462 must also initialize from P$RotDoor, got ${JSON.stringify(second.position)}`,
    );
    await assertBodyFollows(game, second);

    const closedSave = `rotating-door-closed-${process.pid}`;
    assert.equal((await game.save(closedSave)).success, true);

    await pressUpperControl(game);
    const open = await hatchDetail(game);
    assert.ok(
      rotationDistance(open.rotation, closed.rotation) > 0.1,
      `linked control must rotate the hatch: ${JSON.stringify(closed.rotation)} -> ${JSON.stringify(open.rotation)}`,
    );
    await assertBodyFollows(game, await byMissionId(game, HATCH, "Floor Hatch"));

    const openSave = `rotating-door-open-${process.pid}`;
    assert.equal((await game.save(openSave)).success, true);
    assert.equal((await game.load(openSave)).success, true);
    await game.step({ frames: 30 });
    const restoredOpen = await hatchDetail(game);
    assert.ok(distance(restoredOpen.position, open.position) < 0.01);
    assert.ok(rotationDistance(restoredOpen.rotation, open.rotation) < 0.001);

    // The same production press starts the authored 10-second door cycle;
    // after its hold, StdDoor closes the hatch rotationally without a debug
    // message or synthetic TurnOff.
    await game.step({ frames: 660 });
    const reclosed = await hatchDetail(game);
    assert.ok(distance(reclosed.position, closed.position) < 0.01);
    assert.ok(rotationDistance(reclosed.rotation, closed.rotation) < 0.001);

    assert.equal((await game.load(closedSave)).success, true);
    await game.step({ frames: 30 });
    const restoredClosed = await hatchDetail(game);
    assert.ok(distance(restoredClosed.position, closed.position) < 0.01);
    assert.ok(rotationDistance(restoredClosed.rotation, closed.rotation) < 0.001);
    await assertBodyFollows(game, await byMissionId(game, HATCH, "Floor Hatch"));
  },
);

test(
  "exact campaign frontier: the supported upper approach opens Hatch 85",
  { skip: !e2eEnabled || !exactCampaignSave, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8185) + 1,
    });
    assert.equal((await game.load(exactCampaignSave!)).success, true);
    await game.step({ frames: 30 });
    assert.equal((await game.info()).mission, "eng1.mis");

    const closed = await hatchDetail(game);
    assert.ok(distance(closed.position, AUTHORED_CLOSED) < 0.01);
    await assertBodyFollows(game, await byMissionId(game, HATCH, "Floor Hatch"));

    // Walk from the saved supported frontier around the hatch lip to the same
    // upper-side control approach. The bounded SDK primitive shape-casts the
    // real player capsule; it cannot cross the sealed hatch or world geometry.
    const approach = await game.player.moveTo({ x: -3, y: -12, z: -22.3 });
    assert.ok(
      approach.distance_moved > 2,
      `saved player must walk toward the upper control: ${JSON.stringify(approach)}`,
    );
    const control = await byMissionId(game, UPPER_CONTROL, "upper hatch control");
    const aim = await game.player.aimAt(control, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      aim.target_confirmed,
      true,
      `saved frontier must have a production interaction ray to control 152: ${JSON.stringify(aim)}`,
    );
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 180 });

    const open = await hatchDetail(game);
    assert.ok(rotationDistance(open.rotation, closed.rotation) > 0.1);
    await assertBodyFollows(game, await byMissionId(game, HATCH, "Floor Hatch"));
  },
);
