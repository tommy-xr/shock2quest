import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";

// The VR frontend panel is placed ONCE, from the head pose, and then
// world-locked: turning the head moves it in view instead of dragging it along.
// Only sustained divergence (>60 degrees of gaze yaw, held ~1s) recenters it.
//
// Headless observation: the menu's rollover blip (MROLLOV1) fires when the
// controller ray enters a widget, so "does aiming at this world position still
// hit the Load Game button?" is answerable from the audio log. Where the panel
// is, is therefore measurable without reading pixels.
//
// Negative-first: against the previous gaze-glued panel, the first test's
// "after turning the head, the panel is still at the OLD world position"
// assertion fails - the panel had already followed the gaze.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const ROLLOVER = "sfx/mrollov1.wav";

const CANVAS_W = 640;
const CANVAS_H = 480;
const PANEL_DISTANCE = 2;
const PANEL_SIZE = { x: 2, y: 1.5 };

/** Normalized canvas center of menu entry `index` (MAINR.BIN's button stack). */
const menuEntry = (index: number): [number, number] => [
  (400 + 179 / 2) / CANVAS_W,
  (20 + index * 76 + 60 / 2) / CANVAS_H,
];

const NEW_GAME = menuEntry(0);
const LOAD_GAME = menuEntry(1);

type Vec3 = [number, number, number];

/**
 * The runtime's head convention: `head.look` yaw 0 gazes along -X.
 */
function gaze(yawDegrees: number): Vec3 {
  const yaw = (yawDegrees * Math.PI) / 180;
  return [-Math.cos(yaw), 0, -Math.sin(yaw)];
}

/**
 * Where a canvas point sits in the world on a panel placed from a head looking
 * along `yawDegrees` - the same yaw-only, gravity-aligned placement
 * `FrontendPanelAnchor` makes.
 */
function panelPoint(
  yawDegrees: number,
  [u, v]: [number, number],
  origin: Vec3 = [0, PLAYER_EYE_HEIGHT_WORLD, 0],
): Vec3 {
  const [gx, , gz] = gaze(yawDegrees);
  const center: Vec3 = [
    origin[0] + gx * PANEL_DISTANCE,
    origin[1],
    origin[2] + gz * PANEL_DISTANCE,
  ];
  // The panel faces back at the head, so the viewer's right is up x (-gaze).
  const right: Vec3 = [-gz, 0, gx];
  return [
    center[0] + right[0] * (u - 0.5) * PANEL_SIZE.x,
    center[1] + (0.5 - v) * PANEL_SIZE.y,
    center[2] + right[2] * (u - 0.5) * PANEL_SIZE.x,
  ];
}

/** A controller rotation whose -Z ray runs along `direction`. */
function aimAlong([dx, , dz]: Vec3): [number, number, number, number] {
  const theta = Math.atan2(-dx, -dz);
  return [0, Math.sin(theta / 2), 0, Math.cos(theta / 2)];
}

/**
 * Aim the right controller at a canvas point on the panel a head at
 * `panelYaw` would have placed, standing `PANEL_DISTANCE` back along the ray.
 */
async function aimAt(
  game: GameServer,
  panelYaw: number,
  point: [number, number],
  origin?: Vec3,
): Promise<void> {
  const direction = gaze(panelYaw);
  const target = panelPoint(panelYaw, point, origin);
  await game.input.set("right_hand.rotation", aimAlong(direction));
  await game.input.set("right_hand.position", [
    target[0] - direction[0] * PANEL_DISTANCE,
    target[1],
    target[2] - direction[2] * PANEL_DISTANCE,
  ]);
}

/** Point the controller at nothing, so the next entry is a fresh rollover. */
async function aimAway(game: GameServer): Promise<void> {
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.input.set("right_hand.position", [0, PLAYER_EYE_HEIGHT_WORLD, 0]);
  await game.step({ frames: 5 });
}

async function lastSequence(game: GameServer): Promise<number> {
  const { sounds } = await game.audio.recent();
  return sounds.length === 0 ? 0 : sounds[sounds.length - 1].sequence;
}

async function rollovers(game: GameServer, since: number): Promise<number> {
  const { sounds } = await game.audio.recent();
  return sounds.filter((s) => s.sequence > since && s.sample === ROLLOVER).length;
}

/** Did aiming there land on a menu entry? */
async function hovers(
  game: GameServer,
  panelYaw: number,
  point: [number, number],
  origin?: Vec3,
): Promise<boolean> {
  await aimAway(game);
  const since = await lastSequence(game);
  await aimAt(game, panelYaw, point, origin);
  await game.step({ frames: 5 });
  return (await rollovers(game, since)) > 0;
}

test(
  "the VR frontend panel stays where it was placed while the head turns",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      port: 8171,
      debugFlags: ["--vr"],
    });
    // Scene entry places the panel from the default head (yaw 0).
    await game.step({ frames: 10 });

    assert.ok(
      await hovers(game, 0, LOAD_GAME),
      "the panel should be placed in front of the entry head pose",
    );

    // Turn 40 degrees - inside the recenter threshold - and hold it well past
    // the hold time. The panel must not have moved: the OLD world position
    // still hovers, and where a gaze-glued panel would now be does not.
    await game.input.set("head.look", [40, 0]);
    await game.step({ frames: 180 });

    assert.ok(
      await hovers(game, 0, NEW_GAME),
      "the panel must still be at its placement pose after the head turns",
    );
    assert.equal(
      await hovers(game, 40, NEW_GAME),
      false,
      "the panel must NOT have followed the gaze",
    );
  },
);

test(
  "a sustained turn away recenters the VR panel, a glance does not",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      port: 8172,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    // A glance well past the yaw threshold, but shorter than the hold time.
    await game.input.set("head.look", [120, 0]);
    await game.step({ frames: 30 });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 30 });
    assert.ok(
      await hovers(game, 0, LOAD_GAME),
      "a half-second glance must not drag the panel along",
    );

    // Sustained: past the hold time plus the ease.
    await game.input.set("head.look", [120, 0]);
    await game.step({ frames: 60 + 30 });

    assert.ok(
      await hovers(game, 120, LOAD_GAME),
      "the panel should have re-placed in front of the new gaze",
    );
    assert.equal(
      await hovers(game, 0, LOAD_GAME),
      false,
      "and left its original placement",
    );
  },
);

test(
  "walking away from the VR panel re-places it at the tracked head position",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      port: 8173,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    // Facing the same way throughout: only the head's POSITION changes, so
    // this exercises the distance half of the recenter rule (and proves the
    // panel is hung off the tracked head, not off the pawn origin).
    const moved: Vec3 = [0, PLAYER_EYE_HEIGHT_WORLD, 3];
    await game.input.set("head.position", moved);
    await game.step({ frames: 60 + 30 });

    assert.ok(
      await hovers(game, 0, LOAD_GAME, moved),
      "the panel should follow the head to where it walked",
    );
    assert.equal(
      await hovers(game, 0, LOAD_GAME),
      false,
      "and leave the position it was originally placed from",
    );
  },
);
