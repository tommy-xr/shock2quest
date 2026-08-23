import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { carriedNaniteTotal } from "./helpers/earth-replicator.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8198);

const RESURRECTION_BUTTON = 909;
const RESURRECTION_TARGET = 187;
const RESURRECTION_COST = 10;
const RESPAWN_DELAY_FRAMES = 5 * 60;

type LifeState = "alive" | "dead" | "game_over" | "respawning";

function lifeState(player: object): LifeState | undefined {
  return (player as { life_state?: LifeState }).life_state;
}

async function killPlayer(game: GameServer): Promise<void> {
  const before = await game.info();
  assert.ok(before.player.entity_id !== null, "mission should expose the player entity");
  assert.ok(before.player.hit_points !== null, "player should have hit points");
  await game.entities.sendMessage(before.player.entity_id, {
    type: "Damage",
    amount: before.player.hit_points + 100,
  });
  await game.step({ frames: 1 });
}

test(
  "player death becomes terminal without an activated resurrection station",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 5 });

    await killPlayer(game);
    const dead = await game.info();
    assert.equal(dead.player.hit_points, 0, "lethal damage should clamp HP at zero");
    assert.equal(
      lifeState(dead.player),
      "dead",
      "death without an active QBR must expose a terminal loss signal",
    );

    const deathPosition = dead.player.position;
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const afterInput = await game.info();
    assert.equal(lifeState(afterInput.player), "dead");
    assert.ok(
      Math.hypot(
        afterInput.player.position[0] - deathPosition[0],
        afterInput.player.position[1] - deathPosition[1],
        afterInput.player.position[2] - deathPosition[2],
      ) < 0.001,
      `dead players must not keep moving: before=${JSON.stringify(deathPosition)} after=${JSON.stringify(afterInput.player.position)}`,
    );
  },
);

// `death_camera::FALL_SECONDS`, in fixed-timestep frames.
const FALL_FRAMES = Math.ceil(0.9 * 60);

/** The camera's up axis in pawn space - `camera_rotation` is `[w, x, y, z]`. */
function cameraUp([w, x, y, z]: [number, number, number, number]): [number, number, number] {
  // R * (0, 1, 0)
  return [2 * (x * y - w * z), 1 - 2 * (x * x + z * z), 2 * (y * z + w * x)];
}

test(
  "dying drops the camera to the floor and rolls it onto its side",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 4,
    });
    await game.step({ frames: 5 });

    const alive = (await game.info()).player;
    assert.ok(alive.camera_offset[1] > 1.0, "a live player renders from a standing eye");
    assert.ok(
      Math.abs(cameraUp(alive.camera_rotation)[1] - 1.0) < 0.01,
      `a live camera is upright, got up=${JSON.stringify(cameraUp(alive.camera_rotation))}`,
    );

    await killPlayer(game);

    // The fall ramps in rather than snapping: one frame after death the camera
    // has barely left the tracked pose.
    const justDied = (await game.info()).player;
    assert.equal(lifeState(justDied), "dead");
    assert.ok(
      Math.abs(justDied.camera_offset[1] - alive.camera_offset[1]) < 0.05,
      `the death camera must ease in, not cut: ${justDied.camera_offset[1]} vs ${alive.camera_offset[1]}`,
    );

    await game.step({ frames: Math.floor(FALL_FRAMES / 2) });
    const midFall = (await game.info()).player.camera_offset[1];

    await game.step({ frames: FALL_FRAMES });
    const fallen = (await game.info()).player;
    // Strictly between the standing eye and where it settles - the fall is a
    // ramp, not a cut. (Compared against the settled height, which is only
    // known after the fall lands, so this assertion runs here.)
    assert.ok(
      midFall < alive.camera_offset[1] - 0.2 && midFall > fallen.camera_offset[1] + 0.05,
      `mid-fall the camera should be between the standing eye ${alive.camera_offset[1]} and the settled ${fallen.camera_offset[1]}, got ${midFall}`,
    );
    assert.equal(lifeState(fallen), "dead", "the fall settles inside the death sequence");
    assert.ok(
      fallen.camera_offset[1] < alive.camera_offset[1] - 1.0,
      `the fallen camera lies near the floor, got ${fallen.camera_offset[1]} (alive ${alive.camera_offset[1]})`,
    );
    // Lying on its side: the view's up axis now points along the ground.
    const up = cameraUp(fallen.camera_rotation);
    assert.ok(
      Math.abs(up[1]) < 0.02,
      `the fallen camera has rolled onto its side, got up=${JSON.stringify(up)}`,
    );
    assert.ok(Math.hypot(up[0], up[2]) > 0.98, `and its up axis is horizontal: ${JSON.stringify(up)}`);
  },
);

// `death_camera::LOOK_RECOVERY_SECONDS`, in fixed-timestep frames.
const LOOK_RECOVERY_FRAMES = Math.ceil(0.6 * 60);

/** Angle (degrees) between two unit quaternions `[w, x, y, z]`. */
function angleBetween(
  a: [number, number, number, number],
  b: [number, number, number, number],
): number {
  const dot = Math.abs(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]);
  return (2 * Math.acos(Math.min(1, dot)) * 180) / Math.PI;
}

test(
  "the fall ignores head movement, then hands looking back to the player",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 6,
    });
    await game.step({ frames: 5 });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 2 });

    await killPlayer(game);
    // The moment the fall lands, the camera belongs to the death and not to the
    // head: the recovery ramp has not started, so turning the head does nothing.
    // (Earlier in the fall the tracked head still holds its share of the blend -
    // that is the fall easing in, not the player steering it.)
    await game.step({ frames: FALL_FRAMES });
    const landed = (await game.info()).player.camera_rotation;
    await game.input.set("head.look", [90, 0]);
    await game.step({ frames: 1 });
    const stillPinned = (await game.info()).player.camera_rotation;
    assert.ok(
      angleBetween(landed, stillPinned) < 15,
      `a 90 degree head turn must not move the just-landed camera, moved ${angleBetween(landed, stillPinned).toFixed(1)} degrees`,
    );

    // Once it has landed and the recovery ramp has run, head movement moves the
    // view again - relative to the fallen frame. A camera welded to the death
    // pose is the most nauseating state in VR, so it must not stay welded.
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: FALL_FRAMES + LOOK_RECOVERY_FRAMES });
    const settled = (await game.info()).player.camera_rotation;
    await game.input.set("head.look", [90, 0]);
    await game.step({ frames: 2 });
    const looked = (await game.info()).player.camera_rotation;
    const turned = angleBetween(settled, looked);
    assert.ok(
      turned > 80 && turned < 100,
      `a settled death camera should follow a 90 degree head turn, got ${turned.toFixed(1)} degrees`,
    );
    assert.equal(lifeState((await game.info()).player), "dead", "still dead, just able to look");
  },
);

test(
  "a QBR reconstruction puts the camera back on the player's feet",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 5,
    });
    await game.step({ frames: 5 });

    await game.player.spawnItem("20 Nanites");
    const [button] = await game.entities.byTemplate(RESURRECTION_BUTTON);
    assert.ok(button, "medsci1 should contain its authored QBR scanner button");
    await game.entities.sendMessage(button.id, { type: "Frob" });
    await game.step({ frames: 2 });

    const alive = (await game.info()).player;
    await killPlayer(game);
    await game.step({ frames: FALL_FRAMES });

    const dying = (await game.info()).player;
    assert.equal(lifeState(dying), "respawning", "the fall plays for a QBR death too");
    assert.ok(
      dying.camera_offset[1] < alive.camera_offset[1] - 1.0,
      `a reconstructing player still falls, got ${dying.camera_offset[1]}`,
    );

    await game.step({ frames: RESPAWN_DELAY_FRAMES });
    const revived = (await game.info()).player;
    assert.equal(lifeState(revived), "alive");
    assert.ok(
      Math.abs(revived.camera_offset[1] - alive.camera_offset[1]) < 0.01,
      `reconstruction must hand the camera back, got ${revived.camera_offset[1]} (was ${alive.camera_offset[1]})`,
    );
    assert.ok(
      Math.abs(cameraUp(revived.camera_rotation)[1] - 1.0) < 0.01,
      `and upright, got up=${JSON.stringify(cameraUp(revived.camera_rotation))}`,
    );
  },
);

test(
  "an activated resurrection station revives the player after charging 10 nanites",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 1,
    });
    await game.step({ frames: 5 });

    await game.player.spawnItem("20 Nanites");
    const nanitesBefore = await carriedNaniteTotal(game);
    assert.ok(nanitesBefore >= RESURRECTION_COST);

    const [button] = await game.entities.byTemplate(RESURRECTION_BUTTON);
    assert.ok(button, "medsci1 should contain its authored QBR scanner button");
    const [target] = await game.entities.byTemplate(RESURRECTION_TARGET);
    assert.ok(target, "QBR button should link to its authored teleport target");
    const targetPosition = (await game.entities.detail(target.id)).position;

    await game.entities.sendMessage(button.id, { type: "Frob" });
    await game.step({ frames: 2 });

    // Activation is the scanner's authored switched model, which must survive
    // a save/load before a later death can rely on it.
    const saveName = `player_death_qbr_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 2 });

    await killPlayer(game);

    const dying = await game.info();
    assert.equal(lifeState(dying.player), "respawning");
    assert.equal(dying.player.hit_points, 0);
    assert.equal(
      await carriedNaniteTotal(game),
      nanitesBefore - RESURRECTION_COST,
      "death should charge the retail 10-nanite reconstruction cost once",
    );

    await game.step({ frames: RESPAWN_DELAY_FRAMES });
    const revived = await game.info();
    assert.equal(lifeState(revived.player), "alive");
    assert.equal(
      revived.player.hit_points,
      Math.floor((revived.player.max_hit_points ?? 0) / 2),
      "QBR reconstruction should restore half of maximum health",
    );
    assert.ok(
      Math.hypot(
        revived.player.position[0] - targetPosition[0],
        revived.player.position[1] - targetPosition[1],
        revived.player.position[2] - targetPosition[2],
      ) < 0.25,
      `player should respawn at the QBR teleport target: target=${JSON.stringify(targetPosition)} actual=${JSON.stringify(revived.player.position)}`,
    );
  },
);

// The game-over screen is the original load-game screen (`GAMELOD.PCX`), so
// its "Load" button sits at the `GAMELODR.BIN` rect [527, 161, 96x62] on the
// 640x480 canvas. The runtime renders 4:3, so aspect-preserving placement maps
// canvas coordinates straight onto normalized screen coordinates.
const GAME_OVER_LOAD_BUTTON: [number, number] = [(527 + 96 / 2) / 640, (161 + 62 / 2) / 480];
const DEATH_SEQUENCE_FRAMES = 3 * 60;

async function click(game: GameServer, [x, y]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", [x, y]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

test(
  "terminal death reaches the game-over screen, whose load path resumes play",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 2,
    });
    await game.step({ frames: 5 });

    // The screen offers the most recent save, so make ours the newest one.
    const saveName = `player_death_game_over_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);

    const audioBefore = (await game.audio.recent()).sounds.length;
    await killPlayer(game);

    const dead = await game.info();
    assert.equal(lifeState(dead.player), "dead");
    assert.equal(dead.mission, "medsci1.mis", "the death sequence plays out in the mission");
    // The authored `PlayerDeath0..4` schemas resolve to the retail player death
    // samples (`XXpdieNN`), so assert on the resolved sample, not just a count.
    const played = (await game.audio.recent()).sounds.slice(audioBefore);
    assert.ok(
      played.some((sound) => /pdie/i.test(sound.sample)),
      `death should play the authored player death vocalization, got ${JSON.stringify(
        played.map((sound) => sound.sample),
      )}`,
    );

    // The death sequence hands off to the game-over screen.
    await game.step({ frames: DEATH_SEQUENCE_FRAMES + 30 });
    const gameOver = await game.info();
    assert.equal(
      gameOver.mission,
      "game_over",
      "terminal death must reach the game-over screen instead of hanging in the dead mission",
    );
    assert.equal(lifeState(gameOver.player), "game_over");

    // ... and its "Load" button is a real recovery path back into play. The
    // screen restores the most recent save in the shared `<data>/saves`
    // directory, so assert on being playable again rather than on which
    // mission another test's save may have left as the newest.
    await click(game, GAME_OVER_LOAD_BUTTON);
    const resumed = await game.info();
    assert.notEqual(
      resumed.mission,
      "game_over",
      "loading from the game-over screen must return to a playable mission",
    );
    assert.equal(lifeState(resumed.player), "alive");
    assert.ok((resumed.player.hit_points ?? 0) > 0, "the resumed player is alive with health");
  },
);

test(
  "the game-over screen still honors the quick-load action (the runtime-agnostic way out)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 3,
    });
    await game.step({ frames: 5 });

    // Quick-save writes `<data>/saves/save1.sav`, creating the directory if the
    // install has never saved.
    await game.input.trigger("QuickSave");
    await game.step({ frames: 2 });

    await killPlayer(game);
    await game.step({ frames: DEATH_SEQUENCE_FRAMES + 30 });
    assert.equal((await game.info()).mission, "game_over");

    // The screen's buttons are pointer-driven, but a headset has no pointer, so
    // discrete actions must still reach the global effect handler.
    await game.input.trigger("QuickLoad");
    await game.step({ frames: 5 });
    const resumed = await game.info();
    assert.equal(
      resumed.mission,
      "medsci1.mis",
      "quick-load on the game-over screen must restore the quick save",
    );
    assert.equal(lifeState(resumed.player), "alive");
  },
);
