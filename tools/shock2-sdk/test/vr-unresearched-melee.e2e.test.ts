import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { EntityDetailResult, Vec3 } from "../src/types.js";

// VR: an unresearched melee weapon cannot be handled. The blow it would land
// deals no damage; it springs out of the hand instead, with a jolt and
// "Research needed to handle <name>". It lands nearby and can be picked up.
const enabled = process.env.SHOCK2_E2E === "1";
const CRYSTAL_SHARD = -28;
const WRENCH = -928;
const FUMBLE = { amplitude: 0.9, duration_ms: 110 }; // haptics::WEAPON_FUMBLE
// Same sweep as melee-hitbox: through the creature at chest height, inside
// the 0.4 s victim cooldown.
const SWING_FRAMES = 20;
const SWING_FROM_X = 0.6;
const SWING_TO_X = -1.6;

type Game = GameServer;

const hitPoints = (detail: EntityDetailResult) =>
  JSON.stringify(detail.properties.find(p => p.name === "HitPoints")?.value);

/** A creature with hitboxes, the player beside it, and its chest height. */
async function standBesideCreature(game: Game) {
  let target: EntityDetailResult | undefined;
  for (const entity of (await game.entities.list()).entities) {
    const detail = await game.entities.detail(entity.id);
    if ((detail.aim_points ?? []).length > 1) {
      target = detail;
      break;
    }
  }
  assert.ok(target, "expected a creature with hitboxes in debug_melee");
  const [tx, ty, tz] = target.position;
  await game.player.teleport({ x: tx + 1.1, y: ty - 1.0, z: tz });
  await game.step({ frames: 20 });
  return { target, chest: ty + 0.3 - (await game.info()).player.position[1] };
}

/** Sweep the right hand through the creature; the Damage messages sent to it. */
async function swing(game: Game, target: number, chest: number) {
  const before = (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
  for (let frame = 1; frame <= SWING_FRAMES; frame += 1) {
    const t = frame / SWING_FRAMES;
    await game.input.set("right_hand.position", [SWING_FROM_X + (SWING_TO_X - SWING_FROM_X) * t, chest, 0]);
    await game.step({ frames: 1 });
  }
  await game.step({ frames: 2 });
  return (await game.messages.recent()).messages.filter(
    m => m.payload === "Damage" && m.sequence > before && m.to.entity_id === target,
  );
}

test("VR: an unresearched shard jumps out of the hand at a blow and deals no damage", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.input.set("right_hand.squeeze", 1);
  const { entity_id: shard } = await game.player.spawnItem(CRYSTAL_SHARD, { hand: "right" });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, shard, "the shard is held");
  const objectState = (await game.entities.detail(shard)).properties.find(p => p.name === "ObjectState");
  assert.equal(objectState?.value, "Unresearched");

  const { target, chest } = await standBesideCreature(game);
  const before = (await game.info()).player.hand_feedback!.haptics!.sequence[1];
  const damage = await swing(game, target.entity_id, chest);
  assert.deepEqual(damage, [], "no damage to the creature");
  assert.equal(hitPoints(await game.entities.detail(target.entity_id)), hitPoints(target));

  const player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, null, "the shard left the hand");
  const haptics = player.hand_feedback!.haptics!;
  assert.equal(haptics.sequence[1], before + 1, "one jolt on the right hand");
  const pulse = haptics.last[1];
  assert.ok(pulse && Math.abs(pulse.amplitude - FUMBLE.amplitude) < 1e-4, JSON.stringify(pulse));
  assert.equal(pulse.duration_ms, FUMBLE.duration_ms);
  const { messages } = await game.ui.state();
  assert.match(messages.at(-1) ?? "", /^Research needed to handle /);

  // It lands as a world object near the player, and the still-closed hand
  // does not snatch it back.
  await game.step({ frames: 90 });
  assert.ok((await game.physics.bodies({ entityId: shard })).bodies.length > 0, "a world object");
  const landed = (await game.entities.detail(shard)).position as Vec3;
  const away = Math.hypot(landed[0] - player.position[0], landed[2] - player.position[2]);
  assert.ok(away < 3, `lands nearby, not across the room: ${away}`);
  assert.equal((await game.info()).player.right_hand_entity_id, null, "no re-grab while still squeezed");

  // Released, it is grabbable again: stand over it and point down at it.
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 3 });
  await game.player.teleport({ x: landed[0] + 0.5, y: player.position[1], z: landed[2] });
  await game.step({ frames: 10 });
  const pawn = (await game.info()).player.position;
  await game.input.set("right_hand.position", [landed[0] - pawn[0], landed[1] + 0.4 - pawn[1], landed[2] - pawn[2]]);
  await game.input.set("right_hand.rotation", [-Math.SQRT1_2, 0, 0, Math.SQRT1_2]);
  await game.step({ frames: 5 });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, shard, "picked up again");
});

test("VR: a researched melee weapon still lands its blow", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.input.set("right_hand.squeeze", 1);
  const { entity_id: held } = await game.player.spawnItem(WRENCH, { hand: "right" });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, held);

  const { target, chest } = await standBesideCreature(game);
  const damage = await swing(game, target.entity_id, chest);
  assert.equal(damage.length, 1, "the blow lands");
  assert.equal((await game.info()).player.right_hand_entity_id, held, "still in hand");
});
