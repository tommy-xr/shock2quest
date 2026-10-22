import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";
import { add, cross, holdDebugMug, normalize, quatMultiply, quatRotate, scale } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
const METERS_PER_WORLD_UNIT = 0.3048 * 2.5;

/** A tracked right hand in metres, palm down, fingers forward (-Z): an
 * XR_EXT_hand_tracking joint set. `pinch` brings the thumb and index tips
 * together; `left` mirrors it into a left hand. */
function trackedHand(origin: Vec3, { pinch = false, left = false } = {}): Vec3[] {
  const joints: Vec3[] = [
    [0, 0, -0.05], [0, 0, 0],
    [-0.02, -0.01, -0.02], [-0.045, -0.015, -0.045], [-0.06, -0.015, -0.07], [-0.07, -0.015, -0.09],
  ];
  for (const x of [-0.025, -0.008, 0.008, 0.024]) {
    for (const z of [-0.02, -0.09, -0.13, -0.155, -0.175]) joints.push([x, -0.01 * Math.abs(z), z]);
  }
  if (pinch) joints[10] = joints[5]!;
  return joints.map(([x, y, z]) => [
    origin[0] + (left ? -x : x) / METERS_PER_WORLD_UNIT,
    origin[1] + y / METERS_PER_WORLD_UNIT,
    origin[2] + z / METERS_PER_WORLD_UNIT,
  ]);
}

const distance = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
const saveState = (grip: { pose_tool?: { staged: boolean; last_save_error: string | null } | null }) => ({
  staged: grip.pose_tool?.staged,
  last_save_error: grip.pose_tool?.last_save_error,
});

test(
  "grip pose tool: a paused workbench floats the item; the other controller stages, discards and saves",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const mug = await holdDebugMug(game);
    const mugPosition = async () =>
      (await game.entities.list()).entities.find((e) => e.id === mug.id)!.position;
    const mugDraws = async () => (await game.scene.objects({ entityId: mug.id })).objects;
    const drawnOnce = (await mugDraws()).length;
    const rightGrip = async () =>
      (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
    const playerY = async () => (await game.info()).player.position[1];
    const path = join(data.root, "vr-grip-overrides.json");

    // Lift the player so the world's clock shows as a fall in progress.
    const start = (await game.info()).player.position;
    await game.player.teleport({ x: start[0], y: start[1] + 3, z: start[2] });
    await game.step({ frames: 2 });
    const pawn = (await game.info()).player;
    const eye = add(pawn.position, [0, pawn.camera_offset[1], 0]);
    const gaze = quatRotate(quatMultiply(pawn.rotation, (await game.input.state()).head.rotation), [0, 0, -1]);

    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 2 });
    const held = await mugPosition();
    const frozenY = await playerY();
    const frozenTime = (await game.step({ frames: 1 })).render_time;
    // Set the controller down: no squeeze, somewhere else entirely.
    const hand = (await game.input.state()).right_hand.position;
    await game.input.setMany({
      "right_hand.squeeze": 0,
      "right_hand.position": [hand[0] + 1, hand[1] - 0.5, hand[2]],
    });
    const stepped = await game.step({ frames: 30 });
    assert.equal(stepped.render_time, frozenTime, "the world clock stops");
    assert.equal(await playerY(), frozenY, "a falling player hangs in the air");
    let grip = await rightGrip();
    assert.equal(grip.entity_id, mug.id, "the posing hand keeps the mug");
    assert.ok(distance(await mugPosition(), held) < 1e-3, "the mug stays where it was held");

    // The mug floats on the bench: 0.45 m ahead of the eye, 0.3 m below it,
    // 0.1 m to the right (the posing hand), and again in three views.
    let draws = await mugDraws();
    assert.equal(draws.length, drawnOnce * 4, "drawn on the bench and in three extra views");
    assert.ok(draws.every((d) => d.source === "grip_workbench"));
    const ahead = normalize([gaze[0], 0, gaze[2]]);
    const right = cross(ahead, [0, 1, 0]);
    const spot = add(add(eye, scale(ahead, 0.45 / METERS_PER_WORLD_UNIT)),
      add(scale(right, 0.1 / METERS_PER_WORLD_UNIT), [0, -0.3 / METERS_PER_WORLD_UNIT, 0]));
    const bench = draws[0]!.position;
    assert.ok(distance(bench, spot) < 0.02 / METERS_PER_WORLD_UNIT, `bench ${bench} vs ${spot}`);
    // The workbench is a scene of its own: none of the mission is drawn (its
    // world, labels, HUD), only the mug, the hands, a floor and a backdrop.
    const benchScene = (await game.scene.objects()).objects;
    const strays = benchScene
      .filter((o) => o.source !== "grip_workbench" && o.source !== "player_hands")
      .map((o) => `${o.source}:${o.name ?? o.model}`);
    assert.equal(strays.length, 0, `no mission objects, saw ${strays.slice(0, 8).join(", ")}`);
    const environment = benchScene.filter((o) => o.source === "grip_workbench" && o.entity_id === null);
    assert.deepEqual(environment.map((o) => o.name).sort(), ["workbench backdrop", "workbench floor"],
      "a floor and a backdrop, and no dim");
    const floor = environment.find((o) => o.name === "workbench floor")!;
    assert.ok(floor.position[1] < bench[1], "the floor is below the bench");
    // Walking round it (head moved and turned) leaves it where it is.
    await game.input.setMany({ "head.position": [0.6, 0, 0.4], "head.look": [70, 20] });
    await game.step({ frames: 5 });
    assert.ok(distance((await mugDraws())[0]!.position, bench) < 1e-4, "the bench is world-locked");
    // The pause menu still opens over the workbench, which stays behind it.
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 2 });
    const paused = (await game.scene.objects()).objects;
    assert.ok(paused.some((o) => o.source === "pause_dim"), "the pause menu draws over the workbench");
    assert.ok(paused.some((o) => o.source === "grip_workbench" && o.entity_id === mug.id),
      "with the workbench behind it");
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 2 });
    const fitted = grip.grip;
    assert.ok(fitted && !("bones" in fitted), "the mug starts on its shipped grip");

    const otherHand = trackedHand([hand[0] - 0.5, hand[1], hand[2]], { left: true });
    const pullTrigger = async () => {
      await game.input.set("left_hand.trigger", 1);
      await game.step({ frames: 2 });
      await game.input.set("left_hand.trigger", 0);
      await game.step({ frames: 2 });
    };
    const named = async (prefix: string) =>
      (await game.scene.objects()).objects.filter((o) => o.name?.startsWith(prefix)).length;
    // The bench plus three views: 4 copies of every joint.
    const VIEWS = 4;
    const JOINTS = 26;

    // Without hand tracking (controller-only, no multimodal) a capture says so.
    await pullTrigger();
    grip = await rightGrip();
    assert.equal(grip.pose_tool?.capture_failure, "no_joints");
    assert.equal(grip.pose_tool?.tip_error_mm, null, "no tracked hand, no tip error");
    // The frozen glove carries a contact sphere on every joint, palm and wrist
    // included; with nothing tracked there are no raw joint markers.
    assert.equal(await named("contact sphere"), JOINTS * VIEWS);
    assert.equal(await named("tracked joint"), 0);

    // A posing hand that shakes by a centimetre never captures.
    for (let frame = 0; frame < 30; frame++) {
      const x = frame % 2 === 0 ? 0.01 / METERS_PER_WORLD_UNIT : 0;
      await game.input.setMany({
        "right_hand.joints": trackedHand([hand[0] + x, hand[1], hand[2]]),
        "left_hand.joints": otherHand,
      });
      await game.step({ frames: 1 });
      if (frame === 25) await game.input.set("left_hand.trigger", 1);
    }
    await game.input.set("left_hand.trigger", 0);
    await game.step({ frames: 2 });
    grip = await rightGrip();
    assert.deepEqual(grip.grip, fitted, "an unsteady hand does not capture");
    assert.match(grip.pose_tool?.capture_failure ?? "", /^jitter max_mm=\d+\.\d$/);

    // Hold still, then the left trigger stages the pose: applied, not saved.
    await game.input.set("right_hand.joints", trackedHand(hand));
    await game.step({ frames: 30 });
    // The tracked hand's glove gets spheres too, and its raw joints markers.
    assert.equal(await named("contact sphere"), 2 * JOINTS * VIEWS);
    assert.equal(await named("tracked joint"), JOINTS * VIEWS);
    // How far each glove fingertip lands from the tracked one.
    const tipError = (await rightGrip()).pose_tool?.tip_error_mm;
    assert.equal(tipError?.length, 5);
    assert.ok(tipError!.every((mm) => Number.isFinite(mm) && mm >= 0), `${tipError}`);
    // The free (left) controller's thumbstick click hides them, and shows them again.
    const clickStick = async () => {
      await game.input.set("left_hand.thumbstick_click", 1);
      await game.step({ frames: 1 });
      await game.input.set("left_hand.thumbstick_click", 0);
      await game.step({ frames: 1 });
    };
    await clickStick();
    assert.equal(await named("contact sphere"), 0);
    assert.equal(await named("tracked joint"), 0);
    assert.equal((await rightGrip()).pose_tool?.contacts_shown, false);
    await clickStick();
    assert.equal(await named("contact sphere"), 2 * JOINTS * VIEWS);
    await pullTrigger();
    grip = await rightGrip();
    assert.ok(grip.grip && "bones" in grip.grip, "the staged capture applies at once");
    assert.deepEqual(saveState(grip), { staged: true, last_save_error: null });
    assert.equal(grip.pose_tool?.capture_failure, null, "a capture clears the last failure");
    assert.ok(!existsSync(path), "staging writes nothing");
    assert.ok(distance(await mugPosition(), held) < 1e-3, "capturing does not move the mug");

    // B (upper) discards it: the grip returns to the one before the capture.
    await game.input.trigger("LeftHandUpperButton");
    await game.step({ frames: 2 });
    grip = await rightGrip();
    assert.deepEqual(grip.grip, fitted, "discarding restores the previous grip");
    assert.deepEqual(saveState(grip), { staged: false, last_save_error: null });
    assert.ok(!existsSync(path));

    // Stage again. A save that fails (the file's path is a directory) keeps
    // the capture staged and reports why.
    await pullTrigger();
    mkdirSync(path);
    await game.input.trigger("LeftHandLowerButton");
    await game.step({ frames: 2 });
    grip = await rightGrip();
    assert.equal(grip.pose_tool?.staged, true, "a failed save stays staged");
    assert.ok(grip.pose_tool?.last_save_error, "and says why");
    rmdirSync(path);

    // A (lower) saves it, and does not jump.
    const standing = (await game.info()).player.position;
    await game.input.trigger("LeftHandLowerButton");
    await game.step({ frames: 10 });
    assert.ok(Math.abs((await game.info()).player.position[1] - standing[1]) < 1e-3, "no jump");
    assert.ok(existsSync(path), "the capture is saved on the device");
    const saved = JSON.parse(readFileSync(path, "utf8")) as {
      entries: { model: string; hand: string; authored: boolean; grip: { bones?: unknown[] } }[];
    };
    assert.equal(saved.entries.length, 1);
    assert.equal(saved.entries[0]!.model, "mug");
    assert.equal(saved.entries[0]!.hand, "right");
    assert.equal(saved.entries[0]!.grip.bones?.length, 24, "a captured grip holds bone rotations");
    grip = await rightGrip();
    assert.ok(grip.grip && "bones" in grip.grip, "the saved capture stays applied");
    assert.equal(grip.pose_tool?.staged, false);
    const savedGrip = grip.grip;

    // A capture still staged on exit is thrown away, in memory and on disk.
    const savedText = readFileSync(path, "utf8");
    await game.input.set("right_hand.joints", trackedHand([hand[0], hand[1] + 0.05, hand[2]]));
    await game.step({ frames: 30 });
    await pullTrigger();
    assert.notDeepEqual((await rightGrip()).grip, savedGrip, "a new capture is staged");
    // Leave with the controller back in hand but not squeezed, its trigger
    // pulled: the mug neither drops nor fires, and the jump is no throw.
    await game.input.setMany({ "right_hand.position": hand, "right_hand.trigger": 1, "right_hand.joints": null, "left_hand.joints": null });
    await game.input.trigger("ToggleGripPoseTool");
    const resumed = await game.step({ frames: 30 });
    assert.ok(resumed.render_time > frozenTime + 0.4, "the world clock runs again");
    assert.ok(await playerY() < frozenY, "and the fall resumes");
    grip = await rightGrip();
    assert.equal(grip.entity_id, mug.id, "the mug is back in the same hand");
    assert.equal(grip.pose_tool, null, "the workbench is closed");
    assert.deepEqual(grip.grip, savedGrip, "exiting discards the staged capture");
    assert.equal(readFileSync(path, "utf8"), savedText, "and writes nothing");
    draws = await mugDraws();
    assert.equal(draws.length, drawnOnce);
    assert.ok(draws.every((d) => d.source === "entity"), "drawn in the world again");
    const missionScene = (await game.scene.objects()).objects;
    assert.ok(!missionScene.some((o) => o.source === "grip_workbench"), "the workbench scene is gone");
    assert.ok(missionScene.some((o) => o.source === "entity" && o.entity_id !== mug.id),
      "the mission renders again");

    // A fresh squeeze, then letting go, drops it.
    await game.input.setMany({ "right_hand.trigger": 0, "right_hand.squeeze": 1 });
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.right_hand_entity_id, mug.id);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 3 });
    assert.notEqual((await game.info()).player.right_hand_entity_id, mug.id);
  },
);
