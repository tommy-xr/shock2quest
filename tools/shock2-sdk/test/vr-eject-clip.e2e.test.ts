import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// Ejecting a magazine puts its rounds back in the backpack reserve as clips of
// the ammo type they already are, and drops nothing on the floor.
//
// It has NO controller button: the lower face buttons are jump now, and in VR
// the eject is reached through the weapon settings MFD's UNLOAD. The action
// itself is unchanged and still reachable from the flat key, HTTP and the SDK,
// which is what these tests drive.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8676);

/** The debug pistol, and the standard clip its rounds come back as. */
const PISTOL = -17;
const STD_CLIP = -31;
/** An energy weapon: its charge has no clip archetype to return to. */
const LASER_PISTOL = -22;

function propOf(
  detail: { properties: { name: string; value: string }[] },
  name: string,
): number | undefined {
  const p = detail.properties.find((x) => x.name === name);
  return p === undefined ? undefined : Number(p.value);
}

async function ammoOf(game: GameServer, weapon: number): Promise<number> {
  const value = propOf(await game.entities.detail(weapon), "Ammo");
  assert.ok(value !== undefined, "a weapon should expose an Ammo property");
  return value;
}

/** Every standard clip the player is carrying. */
async function carriedClips(game: GameServer): Promise<EntitySummary[]> {
  const carried = new Set(
    (await game.player.inventory()).items.map((item) => item.entity_id),
  );
  return (await game.entities.list({ limit: 300 })).entities.filter(
    (entity) => entity.template_id === STD_CLIP && carried.has(entity.id),
  );
}

/** Standard clips lying in the world (the scene stocks some of its own). */
async function looseClips(game: GameServer): Promise<number> {
  const carried = new Set((await carriedClips(game)).map((clip) => clip.id));
  return (await game.entities.list({ limit: 300 })).entities.filter(
    (entity) => entity.template_id === STD_CLIP && !carried.has(entity.id),
  ).length;
}

/** Total standard rounds the player carries in reserve, across all stacks. */
async function reserveRounds(game: GameServer): Promise<number> {
  let total = 0;
  for (const clip of await carriedClips(game)) {
    total += propOf(await game.entities.detail(clip.id), "StackCount") ?? 0;
  }
  return total;
}

/** Spawn `template` and take it in the VR RIGHT hand. */
async function grabWeapon(
  game: GameServer,
  template: number,
): Promise<EntitySummary> {
  // Diffed against the entity list: `debug_weapons` already stocks one of every
  // gun, so a plain template search could hand back the scenery one.
  const weapon = await cycleToWeapon(game, (e) => e.template_id === template);
  await aimVrHandAt(game, weapon.position as Vec3, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    weapon.id,
    "the VR right hand must hold the weapon",
  );
  return weapon;
}

async function eject(game: GameServer): Promise<void> {
  await game.input.trigger("EjectClip");
  await game.step({ frames: 5 });
}

test(
  "ejecting a gun's magazine puts its rounds in the backpack",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    const pistol = (await grabWeapon(game, PISTOL)).id;

    const loaded = await ammoOf(game, pistol);
    const looseBefore = await looseClips(game);
    assert.ok(loaded > 0, "the debug pistol starts loaded");
    assert.equal(
      await reserveRounds(game),
      0,
      "and carries no reserve of its own",
    );

    // No face button ejects any more - the gun hand's lower button jumps -
    // so a press must leave the magazine exactly where it is.
    await game.input.trigger("RightHandLowerButton");
    await game.step({ frames: 5 });
    assert.equal(
      await ammoOf(game, pistol),
      loaded,
      "the gun hand's lower button must not eject anything",
    );

    await eject(game);
    assert.equal(await ammoOf(game, pistol), 0, "the magazine is emptied");
    assert.equal(
      await reserveRounds(game),
      loaded,
      "the reserve gained exactly the ejected rounds",
    );
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol,
      "the gun stays in the hand",
    );
    assert.equal(
      await looseClips(game),
      looseBefore,
      "the eject leaves no clip lying in the world",
    );

    // Ejecting an empty gun again is a no-op, not a second free clip.
    await eject(game);
    assert.equal(await ammoOf(game, pistol), 0);
    assert.equal(
      await reserveRounds(game),
      loaded,
      "an empty magazine mints nothing",
    );

    // Reload from the rounds just ejected: they are genuinely usable again.
    // The spare clip is what leaves a stack behind for the merge case below -
    // a reload that drains the ejected clip destroys it.
    await game.player.spawnItem(STD_CLIP);
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    const refilled = await ammoOf(game, pistol);
    assert.ok(refilled > 0, "the pistol refills from its own ejected rounds");

    // The merge path: with a stack already carried the rounds join it instead
    // of minting a second clip.
    const stacksBefore = (await carriedClips(game)).length;
    const reserveBefore = await reserveRounds(game);
    await eject(game);
    assert.equal(await ammoOf(game, pistol), 0);
    assert.equal(
      await reserveRounds(game),
      reserveBefore + refilled,
      "the whole magazine came back to reserve",
    );
    assert.equal(
      (await carriedClips(game)).length,
      stacksBefore,
      "merging into the carried stack rather than minting another",
    );
  },
);

test(
  "an energy weapon's charge cannot be ejected",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    const laser = (await grabWeapon(game, LASER_PISTOL)).id;

    const before = await ammoOf(game, laser);
    const carriedBefore = (await game.player.inventory()).items.length;

    await eject(game);
    assert.equal(
      await ammoOf(game, laser),
      before,
      "an energy weapon's charge stays where it is",
    );
    assert.equal(
      (await game.player.inventory()).items.length,
      carriedBefore,
      "and nothing is minted into the backpack",
    );
  },
);
