import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, PlayedSound, Vec3 } from "../src/index.js";
import {
  aimVrHandAt,
  aimVrHandAtCanvas,
  add,
  quatConjugate,
  quatRotate,
  scale,
  sub,
  type Quat,
} from "./helpers/vr-hand.js";
import { fireOnce } from "./helpers/weapon.js";

// The physical VR reload (R3a): a clip pulled out of the cyber-interface strip
// with the free hand and carried to the gun the other hand holds loads it -
// instantly, because the motion is the reload cost.
//
// Negative-first: on the parent every scenario here fails at the insert. The
// clip simply rests against the gun, the magazine stays where it was, and
// nothing is consumed - VR had no reload at all except the Quest's face
// button, which this change removes.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8636);

/** The debug pistol and the ammo the scenarios carry to it. */
const PISTOL = -17;
const STD_CLIP = -31;
const HE_CLIP = -32;
/** Shotgun shells: real ammo, but nothing the pistol's projectiles link to. */
const PELLET_SHOT_BOX = -42;

/** Mirrors `reload::CLIP_INSERT_EXIT_RADIUS` - what the clip must start
 * OUTSIDE of for the insert to be a genuine entry into the magazine zone. */
const CLIP_INSERT_EXIT_RADIUS = 0.35;

function propOf(
  detail: { properties: { name: string; value: string }[] },
  name: string,
): number | undefined {
  const p = detail.properties.find((x) => x.name === name);
  return p === undefined ? undefined : Number(p.value);
}

function ammoOf(detail: {
  properties: { name: string; value: string }[];
}): number {
  const value = propOf(detail, "Ammo");
  assert.ok(value !== undefined, "a weapon should expose an Ammo property");
  return value;
}

function stackOf(detail: {
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
function offHandEntityId(info: {
  player: { wielded_entity_id: number | null };
}): number | null {
  return info.player.wielded_entity_id;
}

function tagValue(sound: PlayedSound, tag: string): string | undefined {
  return sound.tags.find(([t]) => t === tag)?.[1];
}

/** The sound-schema events played since `sequence`. */
async function eventsSince(
  game: GameServer,
  sequence: number,
): Promise<string[]> {
  return (await game.audio.recent()).sounds
    .filter((sound) => sound.sequence > sequence)
    .map((sound) => tagValue(sound, "event") ?? "")
    .filter((event) => event !== "");
}

async function lastSound(game: GameServer): Promise<number> {
  return (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
}

const magnitude = (v: Vec3): number => Math.hypot(v[0], v[1], v[2]);

async function entityById(
  game: GameServer,
  id: number,
): Promise<EntitySummary | undefined> {
  return (await game.entities.list()).entities.find((e) => e.id === id);
}

/** Spawn the debug pistol and take hold of it with the VR RIGHT hand, then turn
 * that hand off the cyber-interface panel so the free left hand owns the canvas
 * pointer when the interface opens. */
async function grabPistol(game: GameServer): Promise<EntitySummary> {
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
 * per-model grip offset this test has no business knowing, so it reads where
 * the clip actually IS and corrects the hand by that world delta (rotated into
 * the pawn space `/v1/control/input` speaks). Returns the hand position it left
 * the controller at, and whether the clip survived the trip.
 */
async function steerHeldClip(
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

/**
 * The player-facing half of the gesture: open the cyber interface, squeeze the
 * clip out of its inventory-strip slot into the LEFT hand, close the interface,
 * and park the clip well clear of the gun.
 *
 * The parking step is what makes the insert that follows a genuine ZONE ENTRY
 * rather than an accident of where the strip grab happened to leave the hand.
 */
async function takeClipIntoOffHand(
  game: GameServer,
  clipId: number,
  weaponId: number,
): Promise<Vec3> {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const ui = await game.ui.state();
  assert.equal(ui.mode, "use", "the cyber interface must be open");
  assert.ok(ui.panel_pose, "the open interface must report its panel pose");
  const slot = ui.strip?.elements.find((e) => e.entity_id === clipId);
  assert.ok(
    slot,
    `the clip must have a strip slot: ${JSON.stringify(ui.strip?.elements)}`,
  );

  await aimVrHandAtCanvas(
    game,
    ui.panel_pose,
    [slot.rect[0] + slot.rect[2] / 2, slot.rect[1] + slot.rect[3] / 2],
    { hand: "left", squeeze: 0 },
  );
  // Point first, THEN squeeze: the panel edge-detects each hand's grab, and an
  // edge needs a frame of that hand merely hovering to be an edge from.
  await game.step({ frames: 4 });
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 6 });
  assert.equal(
    offHandEntityId(await game.info()),
    clipId,
    `a squeeze on the strip slot must pull the clip into the off hand (pointer ${JSON.stringify(
      (await game.ui.state()).pointer,
    )}, right hand ${(await game.info()).player.right_hand_entity_id})`,
  );

  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  assert.equal(
    offHandEntityId(await game.info()),
    clipId,
    "closing the interface must not take the clip back",
  );

  // Park it a comfortable arm's drop below the gun's magazine.
  const parked = await steerHeldClip(
    game,
    "left",
    [0, 0, 0],
    clipId,
    async () => add(await magazineAnchor(game, weaponId), [0, -1.2, 0]),
  );
  assert.equal(parked.consumed, false, "parking the clip must not insert it");

  const clip = await entityById(game, clipId);
  assert.ok(clip);
  assert.ok(
    magnitude(sub(clip.position as Vec3, await magazineAnchor(game, weaponId))) >
      CLIP_INSERT_EXIT_RADIUS,
    "the clip must start outside the magazine zone, or the insert proves nothing",
  );
  return parked.handPosition;
}

/** Where the weapon's magazine zone is centred: its per-model anchor, carried
 * by the live transform, as the runtime reports it. Read live, since the gun
 * rides the hand. */
async function magazineAnchor(game: GameServer, weaponId: number): Promise<Vec3> {
  const weapon = await game.entities.detail(weaponId);
  assert.ok(weapon.magazine_anchor, "a held gun must report its magazine anchor");
  return weapon.magazine_anchor;
}

/** Carry the parked clip into the weapon's magazine zone. */
async function insertClip(
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
async function spawnClip(
  game: GameServer,
  template: number,
): Promise<{ id: number; rounds: number }> {
  const spawned = await game.player.spawnItem(template);
  const rounds = stackOf(await game.entities.detail(spawned.entity_id));
  assert.ok(rounds > 0, "a spawned clip carries rounds");
  return { id: spawned.entity_id, rounds };
}

test(
  "carrying a clip into a VR-held weapon's magazine loads it",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await grabPistol(game);
    // The magazine is the pistol's grip, not its model origin: the zone the
    // clip has to reach sits away from where the gun itself is reported.
    const anchorOffset = sub(
      await magazineAnchor(game, pistol.id),
      (await entityById(game, pistol.id))!.position as Vec3,
    );
    assert.ok(
      2 * magnitude(anchorOffset) > CLIP_INSERT_EXIT_RADIUS,
      `the pistol's magazine anchor must sit off its origin (offset ${JSON.stringify(anchorOffset)})`,
    );
    // The debug pistol spawns with a FULL magazine, so what it holds now is its
    // capacity - the number the insert has to restore. Pinned as a precondition
    // rather than assumed, so a data change fails here and not three asserts
    // later with a confusing count.
    const capacity = ammoOf(await game.entities.detail(pistol.id));
    assert.ok(capacity > 0, "the debug pistol starts loaded, and starts full");

    // Empty it, so the insert has somewhere to put rounds.
    for (let shot = 0; shot < 40; shot += 1) {
      if (ammoOf(await game.entities.detail(pistol.id)) === 0) break;
      await fireOnce(game);
    }
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), 0);

    const clip = await spawnClip(game, STD_CLIP);
    assert.ok(
      clip.rounds >= capacity,
      `this scenario needs a clip that can fill the magazine (${clip.rounds} vs ${capacity})`,
    );
    let parked = await takeClipIntoOffHand(game, clip.id, pistol.id);
    const before = await lastSound(game);

    // The zone moved with the anchor: the point mirrored across the model
    // origin is twice the anchor's offset from it - reaching it must NOT
    // load. Approached along the ray out of the anchor (from well beyond the
    // mirror point), so the clip never strays inside the zone on the way.
    const alongAnchorRay = async (steps: number): Promise<Vec3> =>
      sub(await magazineAnchor(game, pistol.id), scale(anchorOffset, steps));
    const far = await steerHeldClip(game, "left", parked, clip.id, () =>
      alongAnchorRay(8),
    );
    const mirrored = await steerHeldClip(game, "left", far.handPosition, clip.id, () =>
      alongAnchorRay(2),
    );
    await game.step({ frames: 5 });
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      0,
      "a clip carried to the far side of the gun's origin must miss the magazine",
    );
    parked = mirrored.handPosition;

    await insertClip(game, parked, clip.id, pistol.id);

    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      capacity,
      "the insert must fill the magazine",
    );
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "std",
      "a standard clip loads standard rounds",
    );
    assert.equal(
      await entityById(game, clip.id),
      undefined,
      "a clip emptied into the gun is consumed",
    );
    assert.equal(
      offHandEntityId(await game.info()),
      null,
      "the consumed clip must leave the hand",
    );
    assert.equal(
      (await game.info()).player.reloading,
      false,
      "the gesture loads instantly - the motion is the reload cost",
    );
    const played = await eventsSince(game, before);
    assert.ok(
      played.includes("reload"),
      `the insert must play the weapon's reload cue, got ${JSON.stringify(played)}`,
    );

    // Second leg: a partly-spent magazine is TOPPED OFF, and the clip keeps the
    // rounds the gun did not need.
    const spent = 5;
    for (let shot = 0; shot < spent; shot += 1) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), capacity - spent);

    const second = await spawnClip(game, STD_CLIP);
    const parkedAgain = await takeClipIntoOffHand(game, second.id, pistol.id);
    await insertClip(game, parkedAgain, second.id, pistol.id);

    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      capacity,
      "the top-off must reach capacity",
    );
    assert.equal(
      offHandEntityId(await game.info()),
      second.id,
      "a clip with rounds left over stays in the hand",
    );
    assert.equal(
      stackOf(await game.entities.detail(second.id)),
      second.rounds - spent,
      "only the missing rounds leave the clip",
    );
  },
);

test(
  "a clip the weapon does not take is refused, not swallowed",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await grabPistol(game);
    const loaded = ammoOf(await game.entities.detail(pistol.id));
    const ammoType = (await game.info()).player.wielded_ammo_type;

    const shells = await spawnClip(game, PELLET_SHOT_BOX);
    const parked = await takeClipIntoOffHand(game, shells.id, pistol.id);
    const before = await lastSound(game);

    await insertClip(game, parked, shells.id, pistol.id);

    assert.equal(
      offHandEntityId(await game.info()),
      shells.id,
      "a refused clip stays in the hand",
    );
    assert.equal(
      stackOf(await game.entities.detail(shells.id)),
      shells.rounds,
      "a refused clip keeps every round",
    );
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      loaded,
      "the magazine is untouched",
    );
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      ammoType,
      "the loaded ammo type is untouched",
    );
    const played = (await game.audio.recent()).sounds.filter(
      (sound) => sound.sequence > before,
    );
    // `repfail`, the game's refusal chime, resolves to the `noinvrm` sample.
    assert.deepEqual(
      played.map((sound) => sound.sample),
      ["noinvrm"],
      "the refusal must be audible, and nothing else must have happened",
    );
  },
);

test(
  "inserting a clip of another ammo type ejects the loaded rounds and swaps",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 2,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = await grabPistol(game);
    const loaded = ammoOf(await game.entities.detail(pistol.id));
    assert.ok(loaded > 0, "the swap must be made over a LOADED magazine");
    assert.equal((await game.info()).player.wielded_ammo_type, "std");

    const he = await spawnClip(game, HE_CLIP);
    const parked = await takeClipIntoOffHand(game, he.id, pistol.id);

    await insertClip(game, parked, he.id, pistol.id);

    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "he",
      "the inserted clip's type becomes the loaded type",
    );
    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      Math.min(he.rounds, loaded),
      "the magazine holds the new type's rounds",
    );

    // The standard rounds that were loaded come BACK as standard ammo, all of
    // them - the swap costs a reload, it neither converts nor loses rounds.
    const inventory = await game.player.inventory();
    const returned = inventory.items.filter((item) =>
      (item.name ?? "").includes("Standard"),
    );
    let returnedRounds = 0;
    for (const item of returned) {
      returnedRounds += stackOf(await game.entities.detail(item.entity_id));
    }
    assert.equal(
      returnedRounds,
      loaded,
      `every ejected standard round must be in the pack: ${JSON.stringify(inventory.items)}`,
    );
  },
);
