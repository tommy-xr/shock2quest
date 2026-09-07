import assert from "node:assert/strict";

import type { EntitySummary, GameServer, Vec3 } from "../../src/index.js";
import { ammoOf } from "./weapon.js";

export { ammoOf };
import {
  aimVrHandAt,
  add,
  quatConjugate,
  quatRotate,
  sub,
  type Quat,
} from "./vr-hand.js";

/** The debug pistol and the ammo the VR clip scenarios carry to it. */
export const PISTOL = -17;
export const STD_CLIP = -31;

/** Mirrors `reload::CLIP_INSERT_EXIT_RADIUS` at the view model's authored size
 * - what a clip must start OUTSIDE of for an insert to be a genuine entry into
 * the magazine zone. The live radius rides the gun's wield scale
 * (`reload::clip_insert_radii`), so it is read through
 * [`clipInsertExitRadius`] rather than used raw. */
const CLIP_INSERT_EXIT_RADIUS = 0.35;

/** The exit radius as a VR-wielded gun actually gets it: the constant above
 * times the live `gun_scale` dev param, so a test is not pinned to a
 * particular default. */
export async function clipInsertExitRadius(game: GameServer): Promise<number> {
  const params = await game.devParams.list();
  const scale = params.params.find((p) => p.key === "gun_scale")?.value;
  assert.ok(typeof scale === "number", "the runtime must expose the gun_scale dev param");
  return scale * CLIP_INSERT_EXIT_RADIUS;
}

export function propOf(
  detail: { properties: { name: string; value: string }[] },
  name: string,
): number | undefined {
  const p = detail.properties.find((x) => x.name === name);
  return p === undefined ? undefined : Number(p.value);
}

export function stackOf(detail: {
  properties: { name: string; value: string }[];
}): number {
  const value = propOf(detail, "StackCount");
  assert.ok(value !== undefined, "a clip should expose a StackCount property");
  return value;
}

/**
 * What the OFF hand is holding.
 *
 * `PlayerInfo` carries two hand slots and the snapshot spells the LEFT one
 * `wielded_entity_id` - flatscreen wields into that slot, so the name is right
 * there and merely unhelpful here (see `shock2vr::wielded_weapon`). These
 * scenarios put the gun in the right hand and the clip in the left, so this is
 * the clip hand.
 */
export function offHandEntityId(info: {
  player: { wielded_entity_id: number | null };
}): number | null {
  return info.player.wielded_entity_id;
}

export const magnitude = (v: Vec3): number => Math.hypot(v[0], v[1], v[2]);

export async function entityById(
  game: GameServer,
  id: number,
): Promise<EntitySummary | undefined> {
  return (await game.entities.list()).entities.find((e) => e.id === id);
}

/** Spawn the debug pistol and take hold of it with the VR RIGHT hand, then turn
 * that hand off the cyber-interface panel so the free left hand owns the canvas
 * pointer when the interface opens. */
export async function grabPistol(game: GameServer): Promise<EntitySummary> {
  await game.input.trigger("DebugCycleWeapon");
  await game.step({ frames: 10 });
  const pistol = (await game.entities.list()).entities.find(
    (e) => e.template_id === PISTOL,
  );
  assert.ok(pistol, "DebugCycleWeapon must spawn the Pistol");

  await aimVrHandAt(game, pistol.position as Vec3, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    pistol.id,
    "the VR right hand must hold the pistol",
  );

  // Point the gun hand down and away: a hand aimed at the head-anchored panel
  // would compete for the canvas pointer the clip hand needs.
  await game.input.set("right_hand.rotation", [0.5, 0, 0, 0.866]);
  await game.step({ frames: 5 });
  return pistol;
}

/**
 * Move `hand` until the clip it holds sits at `target` (world space).
 *
 * Closed loop rather than an offset table: a held item hangs off its hand by a
 * per-model grip offset the tests have no business knowing, so it reads where
 * the clip actually IS and corrects the hand by that world delta (rotated into
 * the pawn space `/v1/control/input` speaks). Returns the hand position it left
 * the controller at, and whether the clip survived the trip.
 */
export async function steerHeldClip(
  game: GameServer,
  hand: "left" | "right",
  handPosition: Vec3,
  clipId: number,
  target: () => Promise<Vec3>,
): Promise<{ handPosition: Vec3; consumed: boolean }> {
  let position = handPosition;
  for (let attempt = 0; attempt < 6; attempt += 1) {
    const clip = await entityById(game, clipId);
    if (!clip) return { handPosition: position, consumed: true };
    const delta = sub(await target(), clip.position as Vec3);
    if (magnitude(delta) <= 0.03) return { handPosition: position, consumed: false };
    const pawnRotation = (await game.info()).player.rotation as Quat;
    position = add(position, quatRotate(quatConjugate(pawnRotation), delta));
    await game.input.set(`${hand}_hand.position`, position);
    await game.step({ frames: 3 });
  }
  return { handPosition: position, consumed: false };
}

/** Where the weapon's magazine zone is centred: its per-model anchor, carried
 * by the live transform, as the runtime reports it. Read live, since the gun
 * rides the hand. */
export async function magazineAnchor(
  game: GameServer,
  weaponId: number,
): Promise<Vec3> {
  const weapon = await game.entities.detail(weaponId);
  assert.ok(weapon.magazine_anchor, "a held gun must report its magazine anchor");
  return weapon.magazine_anchor;
}

/** Carry the parked clip into the weapon's magazine zone. */
export async function insertClip(
  game: GameServer,
  handPosition: Vec3,
  clipId: number,
  weaponId: number,
): Promise<void> {
  await steerHeldClip(game, "left", handPosition, clipId, () =>
    magazineAnchor(game, weaponId),
  );
  await game.step({ frames: 5 });
}

/** Spawn a clip archetype into the backpack and report its authored stack. */
export async function spawnClip(
  game: GameServer,
  template: number,
): Promise<{ id: number; rounds: number }> {
  const spawned = await game.player.spawnItem(template);
  const rounds = stackOf(await game.entities.detail(spawned.entity_id));
  assert.ok(rounds > 0, "a spawned clip carries rounds");
  return { id: spawned.entity_id, rounds };
}

/** Empty `weapon`'s magazine by firing it dry, and report the capacity it
 * started at - the number a reload has to restore. */
export async function fireDry(
  game: GameServer,
  weaponId: number,
  fire: (game: GameServer) => Promise<void>,
): Promise<number> {
  const capacity = ammoOf(await game.entities.detail(weaponId));
  assert.ok(capacity > 0, "the debug pistol starts loaded, and starts full");
  for (let shot = 0; shot < 40; shot += 1) {
    if (ammoOf(await game.entities.detail(weaponId)) === 0) break;
    await fire(game);
  }
  assert.equal(ammoOf(await game.entities.detail(weaponId)), 0);
  return capacity;
}
