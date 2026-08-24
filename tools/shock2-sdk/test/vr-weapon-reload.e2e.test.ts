import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, PlayedSound, Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end coverage for gun handling from a VR-HELD weapon. The reload and
// ammo-cycle machinery is presentation-shared, but until the Quest face-button
// bindings landed there was no way to reach it in the headset: an empty clip
// just dry-fired forever. These scenarios drive the same `Reload` / `CycleAmmo`
// input actions the Quest's right A / B buttons now emit, against a weapon held
// by the production VR hand rather than wielded as a flat viewmodel.
//
// The audible reload cue is the VR-specific half: flat's reload feedback is a
// first-person viewmodel pitch, which a hand-held VR weapon never renders, so
// the weapon's authored "reload" sound schema is the only signal a VR player
// gets that the reload started. `/v1/audio/recent` is the only headless way to
// see it.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8412);

/** Pistol (first DebugCycleWeapon roster entry) and its standard clip. */
const PISTOL = -17;
const STD_CLIP = -31;

function propOf(
  detail: { properties: { name: string; value: string }[] },
  name: string,
): number | undefined {
  const p = detail.properties.find((x) => x.name === name);
  return p === undefined ? undefined : Number(p.value);
}

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const value = propOf(detail, "Ammo");
  assert.ok(value !== undefined, "weapon should expose an Ammo property");
  return value;
}

function stackOf(detail: { properties: { name: string; value: string }[] }): number {
  const value = propOf(detail, "StackCount");
  assert.ok(value !== undefined, "reserve clip should expose a StackCount property");
  return value;
}

/** Rounds still in a reserve clip. An exhausted stack is destroyed outright, so
 * a vanished entity means zero rounds rather than a missing readout. */
async function remainingRounds(game: GameServer, entityId: number): Promise<number> {
  const live = (await game.entities.list()).entities.some((e) => e.id === entityId);
  return live ? stackOf(await game.entities.detail(entityId)) : 0;
}

function tagValue(sound: PlayedSound, tag: string): string | undefined {
  return sound.tags.find(([t]) => t === tag)?.[1];
}

/** Spawn the pistol into the scene and take hold of it with the VR right hand. */
async function grabPistol(game: GameServer): Promise<EntitySummary> {
  // In VR `wield` is a no-op, so DebugCycleWeapon drops each weapon in front of
  // the player as a world pickup. The pistol is the first roster entry.
  await game.input.trigger("DebugCycleWeapon");
  await game.step({ frames: 10 });
  const pistol = (await game.entities.list()).entities.find((e) => e.template_id === PISTOL);
  assert.ok(pistol, "DebugCycleWeapon must spawn the Pistol");

  await aimVrHandAt(game, pistol.position as Vec3, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    pistol.id,
    "the VR right hand must hold the pistol",
  );
  return pistol;
}

/** Fire until the magazine is empty (the pistol holds a dozen rounds). */
async function emptyTheMagazine(game: GameServer, pistol: EntitySummary): Promise<void> {
  for (let shot = 0; shot < 40; shot += 1) {
    if (ammoOf(await game.entities.detail(pistol.id)) === 0) return;
    await fireOnce(game);
  }
  assert.equal(ammoOf(await game.entities.detail(pistol.id)), 0, "the pistol should be empty");
}

test(
  "a VR-held pistol reloads from reserve and announces it audibly",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await grabPistol(game);
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "std",
      "a hand-held weapon is the wielded weapon in VR",
    );

    const capacity = ammoOf(await game.entities.detail(pistol.id));
    assert.ok(capacity > 0, "the debug pistol starts loaded");
    await emptyTheMagazine(game, pistol);

    // Reserve rounds are what a reload consumes. Two standard clips so the
    // reload has to drain the first and dip into the second.
    const first = await game.player.spawnItem(STD_CLIP);
    const second = await game.player.spawnItem(STD_CLIP);
    const perClip = stackOf(await game.entities.detail(first.entity_id));
    assert.ok(perClip > 0, "a spawned standard clip carries rounds");

    const before = await game.audio.recent();
    const lastSequence = before.sounds.at(-1)?.sequence ?? 0;

    await game.input.trigger("Reload");
    await game.step({ frames: 2 });

    const info = await game.info();
    assert.equal(info.player.reloading, true, "the reload action must start a reload in VR");
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      capacity,
      "the magazine refills to capacity from reserve",
    );

    // The audible cue: without it a VR player has no feedback at all, since the
    // reload animation is a flat-only viewmodel pitch.
    const played = (await game.audio.recent()).sounds.filter(
      (s) => s.sequence > lastSequence,
    );
    const cue = played.find((s) => tagValue(s, "event") === "reload");
    assert.ok(
      cue,
      `the reload must play the weapon's authored reload schema, got ${JSON.stringify(
        played.map((s) => ({ sample: s.sample, tags: s.tags })),
      )}`,
    );

    // Reserve accounting: exactly `capacity` rounds left the backpack.
    const drained =
      2 * perClip -
      ((await remainingRounds(game, first.entity_id)) +
        (await remainingRounds(game, second.entity_id)));
    assert.equal(drained, capacity, "reserve stacks drain by exactly the rounds loaded");

    // A reload at capacity is a no-op, so the remainder is never minted away.
    await game.step({ frames: 130 });
    assert.equal((await game.info()).player.reloading, false, "the reload finishes");
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.reloading, false, "a full gun does not reload");
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), capacity);
  },
);

test(
  "a VR-held empty pistol cycles through its ammo types",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await grabPistol(game);
    assert.equal((await game.info()).player.wielded_ammo_type, "std");

    // Loaded rounds have an established projectile identity, so cycling is
    // refused until the magazine is empty.
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "std",
      "a loaded VR-held pistol cannot change ammo type",
    );

    await emptyTheMagazine(game, pistol);

    const sequence: (string | null)[] = [];
    for (let i = 0; i < 3; i += 1) {
      await game.input.trigger("CycleAmmo");
      await game.step({ frames: 2 });
      sequence.push((await game.info()).player.wielded_ammo_type);
    }
    assert.deepEqual(
      sequence,
      ["ap", "he", "std"],
      "the ammo-cycle action advances in ProjectileOptions.order and wraps",
    );
  },
);
