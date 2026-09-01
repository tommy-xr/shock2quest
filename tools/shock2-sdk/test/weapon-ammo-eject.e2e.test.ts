import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/index.js";

// End-to-end coverage for magazine unload: cycling ammo on a LOADED weapon
// ejects the magazine back to the backpack first, as clips of the ammo type the
// rounds already are, and then switches. Cycling used to require an empty gun
// precisely because there was nowhere to put loaded rounds, so a half-full
// magazine had to be fired off before the type could change.
//
// The two placements the eject has to get right are both exercised here: with
// no matching stack carried it mints the ammo type's canonical clip archetype,
// and with one carried the rounds merge into it rather than cluttering the
// backpack with a second stack.
//
// (`weapon-reload` and `weapon-ammo-type` used to assert the old refusal; that
// assertion moved here, inverted.)
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The full-size standard clip (authored size 12). */
const STD_CLIP = -31;
/** The small standard clip (authored size 6) - the Assault Rifle's *first*
 * authored choice, which must NOT win when the ejected rounds don't fit its
 * authored size (issue #1171). */
const SMALL_STD_CLIP = -1358;

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

/** Every carried clip of `template`, by runtime id. */
async function carriedClips(
  game: GameServer,
  template: number,
): Promise<EntitySummary[]> {
  const carried = new Set(
    (await game.player.inventory()).items.map((item) => item.entity_id),
  );
  return (await game.entities.list()).entities.filter(
    (entity) => entity.template_id === template && carried.has(entity.id),
  );
}

/** Every standard clip the player is carrying, by runtime id. */
async function carriedStandardClips(game: GameServer): Promise<EntitySummary[]> {
  return carriedClips(game, STD_CLIP);
}

/** Total standard rounds held in reserve (across however many stacks). */
async function reserveRounds(game: GameServer): Promise<number> {
  let total = 0;
  for (const clip of await carriedStandardClips(game)) {
    total += stackOf(await game.entities.detail(clip.id));
  }
  return total;
}

async function cycleAmmo(game: GameServer): Promise<string | null> {
  await game.input.trigger("CycleAmmo");
  await game.step({ frames: 2 });
  return (await game.info()).player.wielded_ammo_type;
}

test(
  "cycling a loaded weapon returns its rounds to the backpack",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
    });

    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    const pistol = (await game.info()).player.wielded_entity_id;
    assert.ok(pistol !== null, "the debug pistol should be wielded");
    assert.equal((await game.info()).player.wielded_ammo_type, "std");

    const loaded = ammoOf(await game.entities.detail(pistol));
    assert.ok(loaded > 0, "the debug pistol starts loaded");
    assert.equal(await reserveRounds(game), 0, "and carries no reserve of its own");

    // --- No matching stack: the ejected rounds mint the clip archetype.
    assert.equal(await cycleAmmo(game), "ap", "a loaded magazine no longer blocks the swap");
    assert.equal(
      ammoOf(await game.entities.detail(pistol)),
      0,
      "the magazine is empty until the new type is reloaded",
    );
    const minted = await carriedStandardClips(game);
    assert.equal(minted.length, 1, "the ejected rounds become one carried standard clip");
    assert.equal(
      stackOf(await game.entities.detail(minted[0].id)),
      loaded,
      "carrying exactly the rounds that came out",
    );

    // --- Back to standard and reload from the clip that was just ejected: the
    // rounds are genuinely usable again, not a cosmetic stack.
    assert.equal(await cycleAmmo(game), "he");
    assert.equal(await cycleAmmo(game), "std");
    const spare = await game.player.spawnItem(STD_CLIP);
    const reserveBeforeReload = await reserveRounds(game);
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    const reloaded = ammoOf(await game.entities.detail(pistol));
    assert.equal(reloaded, loaded, "the pistol refills from its own ejected rounds");
    assert.equal(
      await reserveRounds(game),
      reserveBeforeReload - reloaded,
      "reserve drains by exactly the rounds loaded",
    );
    assert.ok(
      (await reserveRounds(game)) > 0,
      "the spare clip leaves a stack behind for the merge case",
    );

    // --- With a matching stack carried, an eject merges into it instead of
    // minting a second one.
    await game.step({ frames: 130 });
    const stacksBefore = (await carriedStandardClips(game)).length;
    const reserveBeforeEject = await reserveRounds(game);
    assert.equal(await cycleAmmo(game), "ap");
    assert.equal(ammoOf(await game.entities.detail(pistol)), 0);
    assert.equal(
      await reserveRounds(game),
      reserveBeforeEject + reloaded,
      "the whole magazine came back to reserve",
    );
    assert.equal(
      (await carriedStandardClips(game)).length,
      stacksBefore,
      "merging into the carried stack rather than minting another",
    );
    assert.ok(spare.entity_id, "the spare clip is the stack that absorbed it");

    // (Save/load of the selection an eject leaves behind is `weapon-reload`'s,
    // on earth.mis - debug scenes have no serializable level to save into.)
  },
);

test(
  "the ejected clip is the archetype whose authored size fits the rounds",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // The Assault Rifle's standard bullet authors [Small Standard Clip,
    // Standard Clip] - small first. Its magazine holds far more than the small
    // box's authored 6, so minting the data's first choice would label the
    // ejected rounds a "Small Standard Clip" (issue #1171). The eject must
    // instead mint the smallest archetype that holds the count - and the
    // largest when, topped past every box size, nothing does.
    await using game = await GameServer.launch({
      mission: "debug_weapons",
    });

    await game.step({ frames: 5 });
    // DebugCycleWeapon's roster is Pistol, then Assault Rifle.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });

    const rifle = (await game.info()).player.wielded_entity_id;
    assert.ok(rifle !== null, "the assault rifle should be wielded");
    assert.equal((await game.info()).player.wielded_ammo_type, "std");

    // Top the rifle up past the largest box's authored size (12): a small
    // spare clip reloads on top of the starting magazine.
    await game.player.spawnItem(SMALL_STD_CLIP);
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    const loaded = ammoOf(await game.entities.detail(rifle));
    assert.ok(loaded > 12, `the rifle holds more than any box (${loaded})`);
    assert.equal(
      (await carriedClips(game, SMALL_STD_CLIP)).length,
      0,
      "the spare was consumed - the eject has nothing to merge into",
    );

    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal(ammoOf(await game.entities.detail(rifle)), 0, "the magazine was ejected");

    const small = await carriedClips(game, SMALL_STD_CLIP);
    const full = await carriedClips(game, STD_CLIP);
    assert.equal(
      small.length,
      0,
      "the rifle's first authored choice, the small box, cannot hold the rounds",
    );
    assert.equal(full.length, 1, "the largest box absorbs the whole magazine");
    assert.equal(stackOf(await game.entities.detail(full[0].id)), loaded);
  },
);
