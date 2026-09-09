import assert from "node:assert/strict";

import type { EntitySummary, GameServer, Vec3 } from "../../src/index.js";

/** The `Ammo` property of a weapon's entity detail - its loaded rounds. */
export function ammoOf(detail: {
  properties: { name: string; value: string }[];
}): number {
  const ammo = detail.properties.find((p) => p.name === "Ammo");
  assert.ok(ammo, "weapon should expose an Ammo property");
  return Number(ammo.value);
}

/** The live shared firing frame, before projectile forward-offset/clamping. */
export function muzzleFrameOf(detail: {
  properties: { name: string; value: string }[];
}): { position: Vec3; forward: Vec3 } {
  const property = detail.properties.find((p) => p.name === "WeaponMuzzle");
  assert.ok(property, "weapon should expose its shared muzzle frame");
  const frame = JSON.parse(property.value) as { position: Vec3; forward: Vec3 };
  assert.equal(frame.position.length, 3);
  assert.equal(frame.forward.length, 3);
  assert.ok([...frame.position, ...frame.forward].every(Number.isFinite));
  assert.ok(
    Math.abs(Math.hypot(...frame.forward) - 1) < 1e-4,
    "muzzle direction is unit length",
  );
  return frame;
}

/** Step until the wielded gun is out of the between-shots wait its fire setting
 * imposes (`shot_interval_ms`), so the next pull actually fires. A pull inside
 * the wait is silently ignored, and the waits are long: 1 s on the shotgun,
 * 3 s on the laser's overcharge. */
export async function waitForShotReady(game: GameServer): Promise<void> {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const remaining = (await game.info()).player.wielded_gun_cooldown_ms;
    // Absent for a runtime predating the field, null when nothing is wielded.
    if (typeof remaining !== "number" || remaining <= 0) return;
    await game.step({ frames: Math.ceil((remaining / 1000) * 60) + 1 });
  }
  throw new Error("the wielded gun never came out of its between-shots wait");
}

/** One edge-triggered pull (weapons fire on the rising edge) and release,
 * stepping a frame for each so the next pull is a fresh edge. Does NOT wait out
 * the gun's between-shots interval - use `fireOnce` unless the point is a pull
 * that lands too early. */
export async function pullTrigger(game: GameServer): Promise<void> {
  await game.input.set("right_hand.trigger", 1.0);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0.0);
  await game.step({ frames: 1 });
}

/** Fire one round: wait out the previous shot's interval, then pull. */
export async function fireOnce(game: GameServer): Promise<void> {
  await waitForShotReady(game);
  await pullTrigger(game);
}

/** Trigger `DebugCycleWeapon` until it spawns an entity matching `match`, and
 * return that freshly created entity. The lookup is diffed against the entity
 * list before each trigger: the `debug_weapons` bench stocks one of every gun,
 * so a by-name or by-template search over the whole scene is ambiguous.
 * Debug-scene scale only: /v1/entities sorts by distance and truncates at the
 * limit, so in a large mission a far-away spawn could fall off the list. */
export async function cycleToWeapon(
  game: GameServer,
  match: (e: EntitySummary) => boolean,
  {
    cycles = 16,
    settleFrames = 10,
  }: { cycles?: number; settleFrames?: number } = {},
): Promise<EntitySummary> {
  for (let i = 0; i < cycles; i += 1) {
    const before = new Set(
      (await game.entities.list({ limit: 300 })).entities.map((e) => e.id),
    );
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: settleFrames });
    const spawned = (await game.entities.list({ limit: 300 })).entities.find(
      (e) => !before.has(e.id) && match(e),
    );
    if (spawned) return spawned;
  }
  throw new Error("DebugCycleWeapon did not spawn a matching weapon");
}
