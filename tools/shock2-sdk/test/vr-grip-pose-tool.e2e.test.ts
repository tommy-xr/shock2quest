import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { attachSupportHand, findRepoRoot, GameServer, setHandWorldPose } from "../src/index.js";
import type { Quat, ResolvedGrip, Vec3 } from "../src/types.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";
import { muzzleFrameOf } from "./helpers/weapon.js";
import { pauseEntry, vrClickCanvasPoint } from "./helpers/frontend-menu.js";
import {
  add, aimVrHandAt, cross, holdDebugMug, normalize, quatConjugate, quatFromTo, quatMultiply, quatRotate, scale, sub,
} from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

/** Metres per world unit at the world scale in effect (`world_scale::meters_per_world_unit`). */
async function metresPerWorldUnit(game: GameServer): Promise<number> {
  const param = (await game.devParams.list()).params.find((p) => p.key === "world_scale");
  assert.equal(typeof param?.value, "number");
  return 0.3048 * 2.5 * (param!.value as number);
}

/** A right hand in metres, palm down, fingers forward (-Z): an
 * XR_EXT_hand_tracking joint set. `pinch` brings the thumb and index tips
 * together; `left` mirrors it into a left hand. */
function handInMetres({ pinch = false, left = false } = {}): Vec3[] {
  const joints: Vec3[] = [
    [0, 0, -0.05], [0, 0, 0],
    [-0.02, -0.01, -0.02], [-0.045, -0.015, -0.045], [-0.06, -0.015, -0.07], [-0.07, -0.015, -0.09],
  ];
  for (const x of [-0.025, -0.008, 0.008, 0.024]) {
    for (const z of [-0.02, -0.09, -0.13, -0.155, -0.175]) joints.push([x, -0.01 * Math.abs(z), z]);
  }
  if (pinch) joints[10] = joints[5]!;
  return joints.map(([x, y, z]) => [left ? -x : x, y, z]);
}

/** That hand tracked at `origin` (pawn-local world units, `unit` metres each). */
function trackedHand(origin: Vec3, unit: number, options: { pinch?: boolean; left?: boolean } = {}): Vec3[] {
  return handInMetres(options).map((p) => add(origin, scale(p, 1 / unit)));
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
    // The bench follows the tracked head, not this runtime's rendered eye
    // (which it scales with the world).
    const headPosition: Vec3 = [0, 1, 0];
    await game.input.set("head.position", headPosition);
    await game.step({ frames: 2 });
    const unit = await metresPerWorldUnit(game);
    const pawn = (await game.info()).player;
    const eye = add(pawn.position, quatRotate(pawn.rotation, headPosition));
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
    const spot = add(add(eye, scale(ahead, 0.45 / unit)), add(scale(right, 0.1 / unit), [0, -0.3 / unit, 0]));
    const bench = draws[0]!.position;
    assert.ok(distance(bench, spot) < 0.02 / unit, `bench ${bench} vs ${spot}`);
    // The panel, from the same eye: 0.33 m ahead, 0.47 m down, 0.24 m to the
    // free (left) side, tilted to face the eye with no roll; a 0.3 m wide
    // 400x400 canvas. Button centres from their canvas rects, independent
    // of the code that reports them.
    const panel = (await workbench(game)).tool.panel.panel!;
    const panelCenter = add(eye, add(scale(ahead, 0.33 / unit), add(scale(right, -0.24 / unit), [0, -0.47 / unit, 0])));
    assert.ok(distance(xyz(panel.center), panelCenter) < 0.005 / unit, `panel ${xyz(panel.center)} vs ${panelCenter}`);
    const facing = normalize(sub(eye, panelCenter));
    const panelUp = normalize(sub([0, 1, 0], scale(facing, facing[1])));
    const panelRight = cross(scale(facing, -1), panelUp);
    const onPanel = ([x, y]: [number, number]) =>
      add(panelCenter, add(scale(panelRight, (x / 400 - 0.5) * 0.3 / unit), scale(panelUp, (0.5 - y / 400) * 0.3 / unit)));
    for (const [event, canvasCenter] of [["Exit", [200, 290]], ["ResetView", [297, 246]]] as const) {
      const button = xyz(panel.buttons.find((b) => b.event === event)!.center);
      const expected = onPanel([...canvasCenter]);
      assert.ok(distance(button, expected) < 0.005 / unit, `${event} ${button} vs ${expected}`);
    }
    // The workbench is a scene of its own: none of the mission is drawn (its
    // world, labels, HUD), only the mug, the hands, a floor grid and the
    // control panel. Nothing fills the background, which passthrough shows on
    // Quest.
    const benchScene = (await game.scene.objects()).objects;
    const strays = benchScene
      .filter((o) => !["grip_workbench", "player_hands", "grip_workbench_panel", "grip_workbench_pointer"].includes(o.source ?? ""))
      .map((o) => `${o.source}:${o.name ?? o.model}`);
    assert.equal(strays.length, 0, `no mission objects, saw ${strays.slice(0, 8).join(", ")}`);
    const environment = benchScene.filter((o) => o.source === "grip_workbench" && o.entity_id === null);
    assert.ok(environment.length > 0 && environment.every((o) => o.name === "workbench grid"),
      `only grid lines, and no dim: ${[...new Set(environment.map((o) => o.name))]}`);
    const floor = environment[0]!;
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
    // The free (left) controller's thumbstick grows the mug as drawn on the
    // bench, not only its grip's scale.
    const benchScale = async () => (await mugDraws())[0]!.scale[0];
    const unscaled = (await rightGrip()).grip!.item_scale;
    assert.ok(Math.abs((await benchScale()) - unscaled) < 1e-3, "drawn at its grip's scale");
    await game.input.set("left_hand.thumbstick", [0, 1]);
    await game.step({ frames: 30 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.step({ frames: 1 });
    grip = await rightGrip();
    const scaled = grip.grip!.item_scale;
    assert.ok(scaled > unscaled * 1.2, `${unscaled} -> ${scaled}`);
    assert.ok(Math.abs((await benchScale()) - scaled) < 1e-3, `drawn at ${await benchScale()}, grip ${scaled}`);
    // A resize is an unsaved edit, which Y (the free controller's upper
    // button) discards like a capture.
    assert.equal(grip.pose_tool?.staged, true, "resizing stages the grip");
    await game.input.trigger("LeftHandUpperButton");
    await game.step({ frames: 2 });
    grip = await rightGrip();
    assert.ok(Math.abs(grip.grip!.item_scale - unscaled) < 1e-6, "discarded back to its size");
    const fitted = grip.grip;
    assert.ok(fitted && !("bones" in fitted), "the mug starts on its shipped grip");

    const otherHand = trackedHand([hand[0] - 0.5, hand[1], hand[2]], unit, { left: true });
    const pullTrigger = async () => {
      await game.input.set("left_hand.trigger", 1);
      await game.step({ frames: 2 });
      await game.input.set("left_hand.trigger", 0);
      await game.step({ frames: 2 });
    };
    const named = async (prefix: string) =>
      (await game.scene.objects()).objects.filter((o) => o.name?.startsWith(prefix)).length;

    // Without hand tracking (controller-only, no multimodal) a capture says so.
    await pullTrigger();
    grip = await rightGrip();
    assert.equal(grip.pose_tool?.capture_failure, "no_joints");
    assert.equal(grip.pose_tool?.tip_error_mm, null, "no tracked hand, no tip error");
    // Untracked, the saved grip's glove holds the mug.
    assert.ok((await named("Right grip glove")) > 0, "the saved grip's glove");

    // A posing hand that shakes by 4 cm (2 cm either side of its mean) never
    // captures.
    for (let frame = 0; frame < 30; frame++) {
      const x = frame % 2 === 0 ? 0.04 / unit : 0;
      await game.input.setMany({
        "right_hand.joints": trackedHand([hand[0] + x, hand[1], hand[2]], unit),
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
    await game.input.set("right_hand.joints", trackedHand(hand, unit));
    await game.step({ frames: 30 });
    // The live glove replaces the saved grip's, which would be in the real
    // hand's way.
    assert.equal(await named("Right grip glove"), 0, "the saved glove steps aside");
    assert.ok((await named("Right glove")) > 0, "for the live one");
    // How far each glove fingertip lands from the tracked one: this flat,
    // built hand's thumb to middle are within the glove's reach, so on them.
    const live = (await rightGrip()).pose_tool;
    const tipError = live?.tip_error_mm;
    assert.equal(tipError?.length, 5);
    assert.ok(tipError!.every((mm) => Number.isFinite(mm) && mm >= 0), `${tipError}`);
    assert.ok(tipError!.slice(0, 3).every((mm) => mm < 3), `thumb to middle on the hand: ${tipError}`);
    // About the glove's size.
    assert.ok(Math.abs(live!.hand_size_ratio! - 1) < 0.1, `${live?.hand_size_ratio}`);
    await pullTrigger();
    grip = await rightGrip();
    assert.ok(grip.grip && "bones" in grip.grip, "the staged capture applies at once");
    assert.ok((await named("Right grip glove")) > 0, "the staged grip previews beside the live hand");
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
    await saveOnPanel(game, "left", unit);
    grip = await rightGrip();
    assert.equal(grip.pose_tool?.staged, true, "a failed save stays staged");
    assert.ok(grip.pose_tool?.last_save_error, "and says why");
    rmdirSync(path);

    // X (lower) test-holds it rather than jumping or saving; X again ends it.
    const standing = (await game.info()).player.position;
    await game.input.trigger("LeftHandLowerButton");
    await game.step({ frames: 10 });
    assert.ok(Math.abs((await game.info()).player.position[1] - standing[1]) < 1e-3, "no jump");
    assert.ok((await rightGrip()).pose_tool?.test_hold, "test holding");
    assert.ok(!existsSync(path), "the lower button saves nothing");
    await game.input.trigger("LeftHandLowerButton");
    await game.step({ frames: 2 });
    assert.equal((await rightGrip()).pose_tool?.test_hold, null);
    // Save is on the panel.
    await saveOnPanel(game, "left", unit);
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
    // Saved, nothing staged: the live hand alone again, until its tracking is
    // lost and the saved grip's glove holds the mug once more.
    assert.equal(await named("Right grip glove"), 0);
    await game.input.setMany({ "right_hand.joints": null });
    await game.step({ frames: 2 });
    assert.ok((await named("Right grip glove")) > 0, "tracking lost: the saved glove is back");

    // A capture still staged on exit is saved, on disk and in play.
    const savedText = readFileSync(path, "utf8");
    await game.input.set("right_hand.joints", trackedHand([hand[0], hand[1] + 0.05, hand[2]], unit));
    await game.step({ frames: 30 });
    await pullTrigger();
    const staged = (await rightGrip()).grip;
    assert.notDeepEqual(staged, savedGrip, "a new capture is staged");
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
    assert.notEqual(readFileSync(path, "utf8"), savedText, "exiting saves the staged capture");
    assertClose(grip.grip, staged, 1e-5);
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

test(
  "grip pose tool: the left hand poses what it holds; the right controller stages and saves",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const mug = await holdDebugMug(game, "left");
    const leftGrip = async () =>
      (await game.info()).player.hand_grips.find((g) => g.hand === "left")!;
    assert.equal((await leftGrip()).entity_id, mug.id);
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 2 });
    assert.ok((await leftGrip()).pose_tool, "the workbench poses the left hand");
    // Set the left controller down; its tracked hand poses, steadily.
    const hand = (await game.input.state()).left_hand.position;
    await game.input.setMany({
      "left_hand.squeeze": 0,
      "left_hand.position": [hand[0] - 1, hand[1] - 0.5, hand[2]],
      "left_hand.joints": trackedHand(hand, await metresPerWorldUnit(game), { left: true }),
    });
    await game.step({ frames: 30 });
    assert.equal((await leftGrip()).pose_tool?.tip_error_mm?.length, 5, "the left hand is tracked");
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    const grip = await leftGrip();
    assert.equal(grip.pose_tool?.staged, true, `staged (${grip.pose_tool?.capture_failure})`);
    assert.ok(grip.grip && "bones" in grip.grip);
    // Save (clicked with the right controller) saves it for the left hand.
    await saveOnPanel(game, "right", await metresPerWorldUnit(game));
    const saved = JSON.parse(readFileSync(join(data.root, "vr-grip-overrides.json"), "utf8")) as {
      entries: { model: string; hand: string; grip: { bones?: unknown[] } }[];
    };
    assert.deepEqual(saved.entries.map((e) => [e.model, e.hand]), [["mug", "left"]]);
    assert.equal(saved.entries[0]!.grip.bones?.length, 24);
    assert.equal((await leftGrip()).pose_tool?.staged, false);
  },
);

type PoseTool = NonNullable<Awaited<ReturnType<GameServer["info"]>>["player"]["hand_grips"][number]["pose_tool"]>;
type PanelEvent = NonNullable<PoseTool["panel"]["panel"]>["buttons"][number]["event"];
const xyz = (p: { x: number; y: number; z: number }): Vec3 => [p.x, p.y, p.z];

/** The open workbench (whichever hand it poses), and the conversion of a
 * world point into the pawn-local space input channels take. */
async function workbench(game: GameServer): Promise<{ tool: PoseTool; hand: "left" | "right"; toLocal: (w: Vec3) => Vec3; toLocalRotation: (q: [number, number, number, number]) => [number, number, number, number] }> {
  const { player } = await game.info();
  const grip = player.hand_grips.find((g) => g.pose_tool)!;
  const inverse = quatConjugate(player.rotation);
  return {
    tool: grip.pose_tool!,
    hand: grip.hand as "left" | "right",
    toLocal: (w) => quatRotate(inverse, sub(w, player.position)),
    toLocalRotation: (q) => quatMultiply(inverse, q),
  };
}

/** A controller `standOff` metres in front of a panel button (`miss` metres
 * off it, world space), aimed along the panel's normal at it. */
async function aimAtButton(
  game: GameServer, hand: "left" | "right", event: PanelEvent, unit: number,
  { miss = [0, 0, 0] as Vec3, trigger = 0, standOff = 0.3 } = {},
): Promise<void> {
  const panel = (await workbench(game)).tool.panel.panel!;
  const normal = xyz(panel.normal);
  const target = add(xyz(panel.buttons.find((b) => b.event === event)!.center), scale(miss, 1 / unit));
  await setHandWorldPose(game, (await game.info()).player, hand, add(target, scale(normal, standOff / unit)),
    quatFromTo([0, 0, -1], scale(normal, -1)));
  await game.input.set(`${hand}_hand.trigger`, trigger);
}

/** Pull and release a controller's trigger where it points. */
async function click(game: GameServer, hand: "left" | "right"): Promise<void> {
  await game.input.set(`${hand}_hand.trigger`, 1);
  await game.step({ frames: 2 });
  await game.input.set(`${hand}_hand.trigger`, 0);
  await game.step({ frames: 2 });
}

/** A controller aimed straight up, at nothing. */
async function aimAway(game: GameServer, hand: "left" | "right"): Promise<void> {
  const { toLocalRotation } = await workbench(game);
  await game.input.set(`${hand}_hand.rotation`, toLocalRotation(quatFromTo([0, 0, -1], [0, 1, 0])));
}

/** Click the panel's Save with a controller, then aim it away again so its
 * next press snapshots. */
async function saveOnPanel(game: GameServer, hand: "left" | "right", unit: number): Promise<void> {
  await aimAtButton(game, hand, "Save", unit);
  await game.step({ frames: 1 });
  await click(game, hand);
  await aimAway(game, hand);
  await game.step({ frames: 1 });
}

/** Open the workbench on the mug held in `holding`, its controller set down
 * and its tracked hand held still; the head where the bench is placed from. */
async function openWorkbench(game: GameServer, holding: "left" | "right" = "right") {
  const mug = await holdDebugMug(game, holding);
  await game.input.set("head.position", [0, 1, 0]);
  await game.step({ frames: 2 });
  const unit = await metresPerWorldUnit(game);
  await game.input.trigger("ToggleGripPoseTool");
  await game.step({ frames: 2 });
  const hand = (await game.input.state())[`${holding}_hand`].position;
  await game.input.setMany({
    [`${holding}_hand.squeeze`]: 0,
    [`${holding}_hand.position`]: [hand[0] + (holding === "right" ? 1 : -1), hand[1] - 0.5, hand[2]],
    [`${holding}_hand.joints`]: trackedHand(hand, unit, { left: holding === "left" }),
  });
  await game.step({ frames: 30 });
  return { mug, unit, hand };
}

test(
  "grip pose tool panel: a ray click saves, a fingertip poke discards, the press edge decides",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const path = join(data.root, "vr-grip-overrides.json");
    const { unit } = await openWorkbench(game);
    const tool = async () => (await workbench(game)).tool;
    const haptics = async () => (await game.info()).player.hand_feedback!.haptics!;

    // The panel is drawn while the workbench is open, and names the free
    // (left) controller's own buttons.
    const panelDraws = (await game.scene.objects()).objects.filter((o) => o.source === "grip_workbench_panel");
    assert.ok(panelDraws.length > 0, "the panel is drawn");
    assert.equal((await tool()).panel.status, "Pose, then snapshot");
    // Nothing staged: Save and Discard are disabled.
    assert.deepEqual((await tool()).panel.panel!.buttons.map((b) => b.event), ["SwitchHand", "Slot", "ResetView", "Exit"]);
    const { v, s } = (await tool()).panel.panel!.rotation;
    const panelUp = (metres: number) => scale(quatRotate([v.x, v.y, v.z, s], [0, 1, 0]), metres);

    // A press that starts with the ray on the panel is the panel's: it clicks
    // (here a disabled button) and never snapshots, though the hand is steady.
    await aimAtButton(game, "left", "SwitchHand", unit, { miss: panelUp(0.04) });
    await game.step({ frames: 1 });
    assert.ok((await tool()).panel.hover, "the ray hovers the panel");
    await click(game, "left");
    assert.equal((await tool()).staged, false, "a press on the panel does not snapshot");
    // ...and one that starts off it snapshots, even if it drifts onto it.
    await aimAway(game, "left");
    await game.input.set("left_hand.trigger", 1);
    await game.step({ frames: 2 });
    await aimAtButton(game, "left", "Save", unit, { trigger: 1 });
    await game.step({ frames: 2 });
    await game.input.set("left_hand.trigger", 0);
    await game.step({ frames: 2 });
    assert.equal((await tool()).staged, true, "a press off the panel snapshots");
    assert.equal((await tool()).panel.status, "Unsaved pose");
    assert.ok(!existsSync(path), "dragging onto Save mid-press clicks nothing");

    // A fresh press on Save saves it, and the free controller feels it.
    const before = (await haptics()).sequence[0];
    await click(game, "left");
    assert.ok(existsSync(path), "a ray click on Save writes the file");
    assert.equal((await tool()).panel.status, "Saved");
    assert.equal((await tool()).saved, true);
    const pulses = await haptics();
    assert.ok(pulses.sequence[0] > before, "the free (left) controller pulses");
    assert.ok(Math.abs(pulses.last[0]!.amplitude - 0.9) < 1e-6, "the save's click");
    const saved = readFileSync(path, "utf8");

    // Stage again, then set the free controller down (untracked) and poke
    // Discard with that hand's tracked index tip: from 3 cm in front to
    // 1.5 cm through the button.
    await aimAway(game, "left");
    await click(game, "left");
    assert.equal((await tool()).staged, true);
    await game.input.set("left_hand.tracked", 0);
    const { toLocal } = await workbench(game);
    const panel = (await tool()).panel.panel!;
    const discard = xyz(panel.buttons.find((b) => b.event === "Discard")!.center);
    const normal = xyz(panel.normal);
    const leftHand = handInMetres({ left: true });
    const tipAt = (depth: number) => {
      const tip = add(discard, scale(normal, depth / unit));
      return leftHand.map((p) => toLocal(add(tip, scale(sub(p, leftHand[10]!), 1 / unit))));
    };
    for (const depth of [0.03, 0.01, -0.005]) {
      await game.input.set("left_hand.joints", tipAt(depth));
      await game.step({ frames: 1 });
    }
    assert.equal((await tool()).staged, true, "touching is not yet a press");
    await game.input.set("left_hand.joints", tipAt(-0.015));
    await game.step({ frames: 1 });
    assert.equal((await tool()).staged, false, "a centimetre through presses Discard");
    assert.equal(readFileSync(path, "utf8"), saved, "discarding writes nothing");
  },
);

test(
  "grip pose tool panel: Pose switches to the other hand's grip on the same item",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const { mug, unit, hand } = await openWorkbench(game);
    const bench = async () => (await game.scene.objects({ entityId: mug.id })).objects[0]!.position;
    const at = await bench();
    const rightBefore = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!.grip;
    await aimAtButton(game, "left", "SwitchHand", unit);
    await click(game, "left");
    const { tool, hand: posing } = await workbench(game);
    assert.equal(posing, "left", "the left hand is posed now");
    assert.ok(distance(await bench(), at) < 1e-3, "the mug stays where it is on the bench");
    const shipped = JSON.parse(
      readFileSync(join(findRepoRoot(process.cwd())!, "assets/vr-grips.json"), "utf8"),
    ) as { entries: { model: string; hand: string; grip: ResolvedGrip }[] };
    // Starting from the left hand's shipped grip for the mug.
    const leftGrip = (await game.info()).player.hand_grips.find((g) => g.hand === "left")!.grip!;
    const shippedLeft = shipped.entries.find((e) => e.model === "mug" && e.hand === "left")!.grip;
    assertClose(leftGrip.rotation, shippedLeft.rotation, 1e-5, "rotation");
    assert.equal(tool.panel.status, "Pose, then snapshot");
    // The right controller is free now: its trigger stages the tracked left
    // hand and Save saves it.
    await game.input.setMany({
      "right_hand.joints": null,
      "right_hand.position": hand,
      "left_hand.joints": trackedHand([hand[0] - 0.4, hand[1], hand[2]], unit, { left: true }),
    });
    await aimAway(game, "right");
    await game.step({ frames: 30 });
    await click(game, "right");
    assert.equal((await workbench(game)).tool.staged, true);
    await saveOnPanel(game, "right", unit);
    const saved = JSON.parse(readFileSync(join(data.root, "vr-grip-overrides.json"), "utf8")) as {
      entries: { model: string; hand: string }[];
    };
    assert.deepEqual(saved.entries.map((e) => [e.model, e.hand]), [["mug", "left"]]);
    // Leaving, the right hand still holds the mug on its own grip.
    await game.input.setMany({ "left_hand.joints": null });
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 5 });
    const grips = (await game.info()).player.hand_grips;
    const right = grips.find((g) => g.hand === "right")!;
    assert.equal(right.entity_id, mug.id);
    assert.deepEqual(right.grip, rightBefore, "the right hand's grip is unchanged");
    assert.ok(!grips.some((g) => g.hand === "left"), "the left hand holds nothing");
  },
);

test(
  "grip pose tool: quitting from the pause menu saves a staged capture",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    // Held where the player starts: the pause panel helper assumes an
    // unmoved pawn.
    await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: ["--vr"] });
    const path = join(data.root, "vr-grip-overrides.json");
    await game.step({ frames: 10 });
    await game.input.set("right_hand.squeeze", 1);
    await game.player.spawnItem("Wrench", { hand: "right" });
    await game.step({ frames: 5 });
    const unit = await metresPerWorldUnit(game);
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 2 });
    const hand = (await game.input.state()).right_hand.position;
    await game.input.setMany({
      "right_hand.squeeze": 0,
      "right_hand.position": [hand[0] + 1, hand[1] - 0.5, hand[2]],
      "right_hand.joints": trackedHand(hand, unit),
    });
    await game.step({ frames: 30 });
    await aimAway(game, "left");
    await click(game, "left");
    assert.equal((await workbench(game)).tool.staged, true);
    assert.ok(!existsSync(path));

    await game.input.set("right_hand.joints", null);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).paused, true);
    await vrClickCanvasPoint(game, pauseEntry(4));
    await game.step({ frames: 10 });
    assert.equal((await game.info()).mission, "main_menu", "quit");
    const saved = JSON.parse(readFileSync(path, "utf8")) as { entries: { model: string; hand: string }[] };
    assert.deepEqual(saved.entries.map((e) => [e.model, e.hand]), [["wrench_h", "right"]]);
  },
);

test(
  "grip pose tool: the bench item is drawn though its frozen mission spot is culled",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using _data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    await game.input.set("right_hand.squeeze", 1);
    const wrench = await game.player.spawnItem("Wrench", { hand: "right" });
    await game.step({ frames: 5 });
    const held = (await game.info()).player.hand_grips.find((g) => g.hand === "right");
    assert.equal(held?.entity_id, wrench.entity_id, "the right hand holds the wrench");
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 2 });
    const benchDraws = async () =>
      (await game.scene.objects({ entityId: wrench.entity_id })).objects.filter((o) => o.source === "grip_workbench");
    assert.ok((await benchDraws()).length > 0, "on the bench");

    // The player moves far away: the wrench, frozen where it was held, is
    // in a cell the mission no longer sees. The bench still shows it.
    const log = (await game.entities.list({ filter: "Audio Log", limit: 80 })).entities
      .find((e) => e.template_id === 1608)!;
    const [x, y, z] = (await game.entities.detail(log.id)).position;
    await game.player.teleport({ x, y: y + 0.5, z });
    await game.step({ frames: 5 });
    const pos = await game.player.position();
    assert.ok(Math.hypot(pos.x - x, pos.z - z) < 3, "teleported");
    assert.ok((await benchDraws()).length > 0, "still on the bench");
  },
);

test(
  "grip pose tool panel: the beam always shows, a press just off the panel never snapshots, a squeeze turns only the view",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const path = join(data.root, "vr-grip-overrides.json");
    const { unit, hand } = await openWorkbench(game);
    const tool = async () => (await workbench(game)).tool;
    const beam = async () =>
      (await game.scene.objects()).objects.filter((o) => o.source === "grip_workbench_pointer").length;

    // The free controller shows its beam within reach of the panel, and
    // aimed at nothing at all.
    await aimAtButton(game, "left", "Exit", unit, { standOff: 0.12 });
    await game.step({ frames: 1 });
    assert.ok((await tool()).panel.hover, "aimed at Exit from 12 cm");
    assert.equal(await beam(), 3, "a beam (halo and core) and its hit dot, 12 cm from the panel");
    await aimAway(game, "left");
    await game.step({ frames: 1 });
    assert.equal(await beam(), 2, "aimed away: the beam still shows, with no dot");

    // Set down (untracked), the free controller draws no beam and its
    // trigger snapshots nothing - nor when it is picked up still pressed.
    await game.input.setMany({ "left_hand.tracked": 0, "left_hand.trigger": 1 });
    await game.step({ frames: 2 });
    assert.equal(await beam(), 0, "no beam out of hand");
    await game.input.set("left_hand.tracked", 1);
    await game.step({ frames: 2 });
    await game.input.set("left_hand.trigger", 0);
    await game.step({ frames: 2 });
    assert.equal((await tool()).staged, false, "no snapshot from a controller out of hand");

    // Quest multimodal: the hand holding the free controller is tracked too.
    // Its controller still aims, and its trigger clicks rather than snapshots.
    await game.input.set("left_hand.joints", trackedHand([hand[0] - 0.4, hand[1], hand[2]], unit, { left: true }));
    await aimAtButton(game, "left", "ResetView", unit);
    const haptics = async () => (await game.info()).player.hand_feedback!.haptics!;
    const clicks = (await haptics()).sequence[0];
    await click(game, "left");
    const clicked = await haptics();
    assert.ok(clicked.sequence[0] > clicks && Math.abs(clicked.last[0]!.amplitude - 0.6) < 1e-6,
      "the ray clicked Reset view");
    assert.equal((await tool()).staged, false, "and did not snapshot");
    // Nor does that hand, closed round its controller, pinch a snapshot.
    await game.input.set("left_hand.joints",
      trackedHand([hand[0] - 0.4, hand[1], hand[2]], unit, { left: true, pinch: true }));
    await game.step({ frames: 3 });
    assert.equal((await tool()).staged, false, "a pinch with the controller in hand is no snapshot");
    await game.input.set("left_hand.joints", null);

    // A press aimed 2 cm past the panel's edge beside Exit is a missed
    // button: no snapshot. 15 cm past it, it snapshots.
    const panel = (await tool()).panel.panel!;
    const { v, s } = panel.rotation;
    const right = quatRotate([v.x, v.y, v.z, s], [1, 0, 0]);
    const exit = xyz(panel.buttons.find((b) => b.event === "Exit")!.center);
    const edgeMetres = (panel.size.x / 2 - dot(sub(exit, xyz(panel.center)), right)) * unit;
    await aimAtButton(game, "left", "Exit", unit, { miss: scale(right, edgeMetres + 0.02) });
    await game.step({ frames: 1 });
    assert.equal((await tool()).panel.hover, null, "off the panel");
    await click(game, "left");
    assert.equal((await tool()).staged, false, "2 cm off the edge does not snapshot");
    await aimAtButton(game, "left", "Exit", unit, { miss: scale(right, edgeMetres + 0.15) });
    await click(game, "left");
    assert.equal((await tool()).staged, true, "15 cm off the edge snapshots");
    await aimAtButton(game, "left", "Save", unit);
    await click(game, "left");
    const unturned = JSON.parse(readFileSync(path, "utf8")).entries[0].grip as ResolvedGrip;

    // Untracked, the posing hand shows the saved glove on the bench.
    await game.input.set("right_hand.joints", null);
    await game.step({ frames: 2 });
    const glove = async () =>
      (await game.scene.objects()).objects.filter((o) => o.name === "Right grip glove").at(-1)!.position;
    const before = await glove();
    // Squeeze the free controller and turn it a quarter turn about world up.
    const { player } = await game.info();
    await aimAway(game, "left");
    const start = (await game.input.state()).left_hand.rotation as [number, number, number, number];
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 2 });
    const quarter: [number, number, number, number] = [0, Math.SQRT1_2, 0, Math.SQRT1_2];
    await game.input.set("left_hand.rotation",
      quatMultiply(quatConjugate(player.rotation), quatMultiply(quarter, quatMultiply(player.rotation, start))));
    await game.step({ frames: 2 });
    await game.input.set("left_hand.squeeze", 0);
    await game.step({ frames: 2 });
    const { turn, pivot } = (await tool()).panel.view;
    const turned = [turn.v.x, turn.v.y, turn.v.z, turn.s];
    assert.ok(Math.abs(dot4(turned, quarter)) > 1 - 1e-4, `turned ${JSON.stringify(turn)}`);
    // The glove turns with the item about it, and stays turned.
    const expected = add(xyz(pivot), quatRotate(quarter, sub(before, xyz(pivot))));
    assert.ok(distance(before, expected) > 0.02 / unit, "the glove sits off the pivot");
    assert.ok(distance(await glove(), expected) < 1e-3 / unit, `${await glove()} vs ${expected}`);

    // The real hand holding the turned mug as it held the unturned one
    // saves the same grip: captures stay relative to the item.
    const toWorld = (p: Vec3) => add(player.position, quatRotate(player.rotation, p));
    const toLocal = (w: Vec3) => quatRotate(quatConjugate(player.rotation), sub(w, player.position));
    await game.input.set("right_hand.joints", trackedHand(hand, unit).map((p) =>
      toLocal(add(xyz(pivot), quatRotate(quarter, sub(toWorld(p), xyz(pivot)))))));
    await game.step({ frames: 30 });
    await aimAway(game, "left");
    await click(game, "left");
    assert.equal((await tool()).staged, true);
    await aimAtButton(game, "left", "Save", unit);
    await click(game, "left");
    assertClose(JSON.parse(readFileSync(path, "utf8")).entries[0].grip, unturned, 1e-3);

    // Reset view turns it back.
    await aimAtButton(game, "left", "ResetView", unit);
    await click(game, "left");
    const reset = (await tool()).panel.view.turn;
    assert.ok(Math.abs(reset.s) > 1 - 1e-6, `reset ${JSON.stringify(reset)}`);

    // Resizing after a save is an unsaved edit, and Exit saves it.
    assert.equal((await tool()).panel.status, "Saved");
    await game.input.set("left_hand.thumbstick", [0, 1]);
    await game.step({ frames: 20 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.step({ frames: 1 });
    assert.equal((await tool()).panel.status, "Unsaved pose");
    assert.ok((await tool()).panel.panel!.buttons.some((b) => b.event === "Save"), "Save is enabled");
    await aimAtButton(game, "left", "Exit", unit);
    await click(game, "left");
    const grown = JSON.parse(readFileSync(path, "utf8")).entries[0].grip as ResolvedGrip;
    assert.ok(grown.item_scale > unturned.item_scale * 1.1, `${unturned.item_scale} -> ${grown.item_scale}`);
  },
);

/** Capture and save `item`'s right-hand grip on the bench, from a tracked
 * hand turned 28 degrees round it (its controller set down, the left one
 * snapshotting), then leave the workbench with the controller back in hand. */
async function captureRightGrip(game: GameServer, item: number): Promise<ResolvedGrip> {
  await game.input.set("head.position", [0, 1, 0]);
  await game.step({ frames: 2 });
  const unit = await metresPerWorldUnit(game);
  await game.input.trigger("ToggleGripPoseTool");
  await game.step({ frames: 2 });
  const controller = (await game.input.state()).right_hand;
  const { toLocal } = await workbench(game);
  const bench = (await game.scene.objects({ entityId: item })).objects[0]!.position;
  await game.input.setMany({
    "right_hand.squeeze": 0,
    "right_hand.position": [controller.position[0] + 1, controller.position[1] - 0.5, controller.position[2]],
    "right_hand.joints": handInMetres().map((p) =>
      toLocal(add(bench, scale(add(quatRotate(yaw(28), p), [0.1, -0.05, 0.05]), 1 / unit)))),
  });
  await aimAway(game, "left");
  await game.step({ frames: 30 });
  await click(game, "left");
  assert.equal((await workbench(game)).tool.staged, true, "staged");
  await saveOnPanel(game, "left", unit);
  const grip = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!.grip!;
  await game.input.setMany({
    "right_hand.joints": null, "right_hand.position": controller.position, "right_hand.squeeze": 1,
  });
  await game.input.trigger("ToggleGripPoseTool");
  await game.step({ frames: 5 });
  return grip;
}

/** `hand`'s held item in its controller's frame: the item's origin and, for
 * a gun, the muzzle and the way it fires. */
async function inController(game: GameServer, hand: "left" | "right", item: number) {
  await game.step({ frames: 30 });
  const { player } = await game.info();
  const input = (await game.input.state())[`${hand}_hand`];
  const rotation = quatMultiply(player.rotation, input.rotation as Quat);
  const position = add(player.position, quatRotate(player.rotation, input.position as Vec3));
  const local = (w: Vec3) => quatRotate(quatConjugate(rotation), sub(w, position));
  const detail = await game.entities.detail(item);
  const gun = detail.properties.some((p) => p.name === "WeaponMuzzle") ? muzzleFrameOf(detail) : null;
  return {
    origin: local(detail.position as Vec3),
    rotation: quatMultiply(quatConjugate(rotation), detail.rotation as Quat),
    muzzle: gun && local(gun.position),
    fires: gun && quatRotate(quatConjugate(rotation), gun.forward),
    grip: player.hand_grips.find((g) => g.hand === hand)!,
  };
}

/** Across the hand's mirror plane (controller-local X). */
const mirrorX = ([x, y, z]: Vec3): Vec3 => [-x, y, z];

// Each item's mirror plane, by an item-frame direction lying in it: a gun
// reflects across its barrel plane (z = 0), a melee rig across the hand's (x = 0).
for (const [name, template, inPlane] of [["shotgun", -19, [1, 1, 0]], ["wrench", -928, [0, 1, 1]]] as const) {
  test(
    `grip pose tool: a ${name} grip posed for the right hand holds it mirrored in the left`,
    { skip: !enabled, timeout: 300_000 },
    async () => {
      using _data = isolatedDataRoot([
        "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
      ]);
      await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
      const unit = await metresPerWorldUnit(game);
      const item = await holdDebugMug(game, "right", template);
      assert.equal((await game.info()).player.right_hand_entity_id, item.id);
      const posed = await captureRightGrip(game, item.id);
      const right = await inController(game, "right", item.id);
      assert.deepEqual(right.grip.grip, posed, "the right hand holds it as posed");

      // Let go, and take it in the left hand.
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 5 });
      assert.notEqual((await game.info()).player.right_hand_entity_id, item.id, "dropped");
      await holdDebugMug(game, "left", template);
      const left = await inController(game, "left", item.id);
      assert.equal(left.grip.entity_id, item.id, "the left hand holds it");
      // The left hand holds the mirror image of the right's hold.
      const near = (a: Vec3, b: Vec3, what: string) =>
        assert.ok(distance(a, b) < 0.002 / unit, `${what}: ${a} vs ${b}`);
      near(left.origin, mirrorX(right.origin), "item origin");
      // Turned alike: a direction in the item's mirror plane is the same
      // item direction in either hand's frame.
      const along = (q: Quat) => quatRotate(q, normalize([...inPlane] as Vec3));
      assert.ok(distance(along(left.rotation), mirrorX(along(right.rotation))) < 1e-3,
        `item rotation: ${along(left.rotation)} vs ${mirrorX(along(right.rotation))}`);
      assert.equal(left.grip.source, "mirrored");
      if (right.muzzle) {
        near(left.muzzle!, mirrorX(right.muzzle), "muzzle");
        near(left.fires!, mirrorX(right.fires!), "firing direction");
        // Kept aim: it fires down the left controller as the right one does.
        const off = (Math.acos(Math.min(1, -left.fires![2])) * 180) / Math.PI;
        assert.ok(off < 1, `shots leave ${off} deg off the left controller`);
        // And the free right hand still supports it.
        await attachSupportHand(game, "left");
        const support = (await game.info()).player.hand_grips.find((g) => g.hand === "left")!.support;
        assert.equal(support?.attached, true, "the right hand supports the left-held gun");
      }
    },
  );
}

test(
  "grip pose tool panel: posing the other hand of a handed weapon draws it mirrored and saves its own pose",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const unit = await metresPerWorldUnit(game);
    const gun = await holdShotgun(game);
    await game.input.set("head.position", [0, 1, 0]);
    await game.step({ frames: 2 });
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 5 });
    const benchDraws = async () => (await game.scene.objects({ entityId: gun.id })).objects
      .filter((o) => o.source === "grip_workbench");
    const before = await benchDraws();
    assert.ok(before.length > 0);
    assert.equal((await workbench(game)).tool.panel.switch_disabled, null);
    await aimAtButton(game, "left", "SwitchHand", unit);
    await click(game, "left");
    const { tool, hand } = await workbench(game);
    assert.equal(hand, "left", "the left hand is posed now");
    assert.equal(tool.posing, "left");
    // The bench draws the shotgun as the left hand holds it: reflected, its
    // faces flipped so it is not drawn inside out.
    const after = await benchDraws();
    assert.equal(after.length, before.length);
    assert.ok(before.some((o) => o.backface_culling), "the shotgun culls back faces");
    after.forEach((o, i) => {
      if (before[i]!.backface_culling) {
        assert.notEqual(o.backface_culling, before[i]!.backface_culling, `${o.name} culls the other winding`);
      }
    });

    // Capture the tracked left hand round the mirrored gun (turned 28 degrees
    // the other way); the free right controller snapshots and saves.
    const controller = (await game.input.state()).right_hand;
    const { toLocal } = await workbench(game);
    const bench = (await game.scene.objects({ entityId: gun.id })).objects[0]!.position;
    await game.input.set("left_hand.joints", handInMetres({ left: true }).map((p) =>
      toLocal(add(bench, scale(add(quatRotate(yaw(-28), p), [-0.1, -0.05, 0.05]), 1 / unit)))));
    await aimAway(game, "right");
    await game.step({ frames: 30 });
    await click(game, "right");
    assert.equal((await workbench(game)).tool.staged, true, "the left hand's pose is staged");
    await saveOnPanel(game, "right", unit);
    const saved = JSON.parse(readFileSync(join(data.root, "vr-grip-overrides.json"), "utf8")) as {
      entries: { model: string; hand: string; grip: ResolvedGrip }[];
    };
    assert.deepEqual(saved.entries.map((e) => [e.model, e.hand]), [["sg_h", "left"]]);
    // The capture kept the mirrored grip's aim: in the left frame the
    // barrel still runs along -X, down the controller.
    assert.ok(aimOff(saved.entries[0]!.grip) < 0.1, `left capture aim ${aimOff(saved.entries[0]!.grip)}`);

    // In play the left hand takes its own saved pose.
    await game.input.setMany({
      "left_hand.joints": null, "right_hand.position": controller.position, "right_hand.squeeze": 1,
    });
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    await holdDebugMug(game, "left", -19);
    const left = (await game.info()).player.hand_grips.find((g) => g.hand === "left")!;
    assert.equal(left.entity_id, gun.id);
    assert.equal(left.source, "prepared");
    assertClose(left.grip!.rotation, saved.entries[0]!.grip.rotation, 1e-5, "rotation");
  },
);

/** Pick up the `debug_interactions` rack's shotgun in the right hand. */
async function holdShotgun(game: GameServer): Promise<{ id: number; position: Vec3 }> {
  const gun = await holdDebugMug(game, "right", -19);
  assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
  return gun;
}

type SavedSupport = { palm_anchor: Vec3; rotation_degrees: Vec3; bones?: unknown[] };

test(
  "grip pose tool: support mode poses the shotgun's support hand against the frozen primary grip",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    const path = join(data.root, "vr-grip-overrides.json");
    const rightGrip = async (game: GameServer) =>
      (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
    let saved: SavedSupport;
    {
      await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
      const gun = await holdShotgun(game);
      const unit = await metresPerWorldUnit(game);
      const shippedSupport = (await rightGrip(game)).support!;
      assert.equal(shippedSupport.finger_pose, "curls", "the shipped support pose");
      await game.input.set("head.position", [0, 1, 0]);
      await game.step({ frames: 2 });
      await game.input.trigger("ToggleGripPoseTool");
      await game.step({ frames: 2 });
      const tool = async () => (await workbench(game)).tool;
      assert.equal((await tool()).mode, "primary");
      const primaryGrip = (await rightGrip(game)).grip;

      // The free (left) controller switches to Support: the left hand is posed now.
      await aimAtButton(game, "left", "SupportMode", unit);
      await click(game, "left");
      assert.equal((await tool()).mode, "support");
      assert.equal((await tool()).posing, "left");
      assert.ok(!(await tool()).panel.panel!.buttons.some((b) => b.event === "SwitchHand"));
      const names = new Set((await game.scene.objects()).objects.map((o) => o.name));
      assert.ok(names.has("Right primary glove"), "the primary glove stays at its grip");
      assert.ok(names.has("Left grip glove"), "the saved support pose is drawn");

      // The tracked left hand along the barrel; the right controller snapshots and A saves.
      const { toLocal } = await workbench(game);
      const bench = (await game.scene.objects({ entityId: gun.id })).objects[0]!.position;
      const leftHand = handInMetres({ left: true });
      await game.input.set("left_hand.joints",
        leftHand.map((p) => toLocal(add(bench, scale(add(p, [-0.25, -0.05, 0]), 1 / unit)))));
      await aimAway(game, "right");
      await game.step({ frames: 30 });
      await click(game, "right");
      assert.equal((await tool()).staged, true, "the support pose is staged");
      assert.equal((await tool()).panel.support_disabled, "save or discard first");
      assert.deepEqual((await rightGrip(game)).grip, primaryGrip, "the primary grip is untouched");
      assert.equal((await tool()).panel.test_hold_disabled, "primary grip only");
      await saveOnPanel(game, "right", unit);
      assert.equal((await tool()).saved, true);
      const file = JSON.parse(readFileSync(path, "utf8")) as { entries: unknown[]; support: Record<string, SavedSupport> };
      assert.deepEqual(file.entries, [], "no primary grip was saved");
      assert.deepEqual(Object.keys(file.support), ["sg_h"]);
      saved = file.support.sg_h!;
      assert.ok(Array.isArray(saved.bones), "the captured fingers are bones");

      // Leave, then support the gun two-handed: it uses the saved pose.
      await game.input.set("left_hand.joints", null);
      await game.input.trigger("ToggleGripPoseTool");
      await game.step({ frames: 5 });
      assert.ok(!(await game.info()).player.hand_grips.some((g) => g.pose_tool));
      await attachSupportHand(game, "right");
      const support = (await rightGrip(game)).support!;
      assert.equal(support.finger_pose, "bones");
      assertClose(support.palm_anchor, saved.palm_anchor, 1e-5, "palm_anchor");
      assertClose(support.rotation_degrees, saved.rotation_degrees, 1e-3, "rotation_degrees");
    }
    // A restart loads it from the overrides file.
    {
      await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
      await holdShotgun(game);
      const support = (await rightGrip(game)).support!;
      assert.equal(support.finger_pose, "bones");
      assertClose(support.palm_anchor, saved.palm_anchor, 1e-5, "palm_anchor");
      assertClose(support.rotation_degrees, saved.rotation_degrees, 1e-3, "rotation_degrees");
    }
  },
);

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const dot4 = (a: number[], b: number[]) => a.reduce((sum, x, i) => sum + x * b[i]!, 0);

/** Numbers within `tolerance`, everything else equal, recursively. */
function assertClose(actual: unknown, expected: unknown, tolerance: number, path = "grip"): void {
  if (typeof expected === "number") {
    assert.ok(typeof actual === "number" && Math.abs(actual - expected) <= tolerance, `${path}: ${actual} vs ${expected}`);
  } else if (expected !== null && typeof expected === "object") {
    assert.deepEqual(Object.keys(actual as object).sort(), Object.keys(expected).sort(), path);
    for (const [key, value] of Object.entries(expected)) {
      assertClose((actual as Record<string, unknown>)[key], value, tolerance, `${path}.${key}`);
    }
  } else {
    assert.deepEqual(actual, expected, path);
  }
}

/** Capture and save the mug's right-hand grip at `worldScale`, the tracked
 * hand held the same, in metres, against the mug on the bench. */
async function captureAt(worldScale: number): Promise<{ saved: ResolvedGrip; tipError: number[] }> {
  using data = isolatedDataRoot([
    "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
  ]);
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.devParams.set("world_scale", worldScale);
  await game.step({ frames: 2 });
  const unit = await metresPerWorldUnit(game);
  const mug = await holdDebugMug(game);
  // A level controller: the mug's world rotation is then the pawn's and its grip's.
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 2 });
  await game.input.trigger("ToggleGripPoseTool");
  await game.step({ frames: 2 });
  const hand = (await game.input.state()).right_hand.position;
  await game.input.setMany({ "right_hand.squeeze": 0, "right_hand.position": [hand[0] + 1, hand[1] - 0.5, hand[2]] });
  await game.step({ frames: 2 });
  const { player } = await game.info();
  const grip = player.hand_grips.find((g) => g.hand === "right")!.grip!;
  const mugRotation = quatMultiply(player.rotation, [grip.rotation.v.x, grip.rotation.v.y, grip.rotation.v.z, grip.rotation.s]);
  const bench = (await game.scene.objects({ entityId: mug.id })).objects[0]!.position;
  // The hand 6 cm under and 4 cm behind the mug, in the mug's frame.
  const joints = handInMetres().map((p) => {
    const world = add(bench, quatRotate(mugRotation, scale(add(p, [0, -0.06, 0.04]), 1 / unit)));
    return quatRotate(quatConjugate(player.rotation), sub(world, player.position));
  });
  await game.input.set("right_hand.joints", joints);
  await game.step({ frames: 30 });
  const tipError = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!.pose_tool!.tip_error_mm!;
  await game.input.set("left_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("left_hand.trigger", 0);
  await game.step({ frames: 2 });
  await saveOnPanel(game, "left", unit);
  const saved = JSON.parse(readFileSync(join(data.root, "vr-grip-overrides.json"), "utf8")) as {
    entries: { grip: ResolvedGrip }[];
  };
  assert.equal(saved.entries.length, 1, `saved at world scale ${worldScale}`);
  return { saved: saved.entries[0]!.grip, tipError };
}

test(
  "grip pose tool: the same hand on the same mug saves the same grip at any world scale",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    const full = await captureAt(1.0);
    const reduced = await captureAt(0.85);
    // Saved in authored units, so it replays alike at either scale.
    assertClose(reduced.saved, full.saved, 1e-3);
    const shipped = JSON.parse(
      readFileSync(join(findRepoRoot(process.cwd())!, "assets/vr-grips.json"), "utf8"),
    ) as { entries: { model: string; hand: string; grip: ResolvedGrip }[] };
    const mug = shipped.entries.find((e) => e.model === "mug" && e.hand === "right")!;
    assert.ok(Math.abs(reduced.saved.item_scale - mug.grip.item_scale) < 1e-5, "the mug keeps its authored size");
    // The glove is drawn at its physical size: its fingertips miss the
    // tracked ones by the same real distance.
    reduced.tipError.forEach((mm, i) =>
      assert.ok(Math.abs(mm - full.tipError[i]!) < 0.5, `tip ${i}: ${mm} vs ${full.tipError[i]} mm`));
  },
);

const quat = (q: ResolvedGrip["rotation"]): Quat => [q.v.x, q.v.y, q.v.z, q.s];
const yaw = (degrees: number): Quat => {
  const half = (degrees * Math.PI) / 360;
  return [0, Math.sin(half), 0, Math.cos(half)];
};
/** Degrees between where a shotgun grip fires (`sg_h`'s barrel is model -X)
 * and the controller's aim (-Z), both in the hand's frame. */
const aimOff = (grip: ResolvedGrip): number => {
  const barrel = quatRotate(quat(grip.rotation), [-1, 0, 0]);
  return (Math.acos(Math.max(-1, Math.min(1, -barrel[2]))) * 180) / Math.PI;
};

test(
  "grip pose tool: a gun capture keeps its aim; test hold shows it and Align aim fires it down the controller",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    const path = join(data.root, "vr-grip-overrides.json");
    // A shotgun grip saved aiming 20 degrees right of the controller (as a
    // capture from a mimed wrist did): the shipped grip turned about the hand.
    const shipped = JSON.parse(
      readFileSync(join(findRepoRoot(process.cwd())!, "assets/vr-weapon-grips.json"), "utf8"),
    ) as { solver_revision: number; entries: { model: string; hand: string; grip: ResolvedGrip }[] };
    const entry = shipped.entries.find((e) => e.model === "sg_h" && e.hand === "right")!;
    assert.ok(aimOff(entry.grip) < 0.1, "the shipped grip fires down the controller");
    const turned = quatMultiply(yaw(-20), quat(entry.grip.rotation));
    const offset = quatRotate(yaw(-20), xyz(entry.grip.offset));
    writeFileSync(path, JSON.stringify({
      version: 1,
      solver_revision: shipped.solver_revision,
      entries: [{
        ...entry,
        grip: {
          ...entry.grip,
          rotation: { v: { x: turned[0], y: turned[1], z: turned[2] }, s: turned[3] },
          offset: { x: offset[0], y: offset[1], z: offset[2] },
        },
      }],
    }));

    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    const rightGrip = async () => (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
    const gun = await holdShotgun(game);
    const skewed = (await rightGrip()).grip!;
    assert.ok(Math.abs(aimOff(skewed) - 20) < 0.1, `the override loads: ${aimOff(skewed)}`);
    await game.input.set("head.position", [0, 1, 0]);
    await game.step({ frames: 2 });
    const unit = await metresPerWorldUnit(game);
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 2 });
    const tool = async () => (await rightGrip()).pose_tool!;
    assert.equal((await tool()).panel.test_hold_disabled, "snapshot first");
    assert.equal((await tool()).panel.align_disabled, "test hold first");

    // The tracked right hand round the grip, turned 28 degrees: the
    // controller set down, the left trigger snapshots.
    const controller = (await game.input.state()).right_hand;
    const { toLocal } = await workbench(game);
    const bench = (await game.scene.objects({ entityId: gun.id })).objects[0]!.position;
    const handWorld = handInMetres().map((p) =>
      toLocal(add(bench, scale(add(quatRotate(yaw(28), p), [0.1, -0.05, 0.05]), 1 / unit))));
    await game.input.setMany({
      "right_hand.squeeze": 0,
      "right_hand.position": [controller.position[0] + 1, controller.position[1] - 0.5, controller.position[2]],
      "right_hand.joints": handWorld,
    });
    await aimAway(game, "left");
    await game.step({ frames: 30 });
    await click(game, "left");
    const captured = (await rightGrip()).grip!;
    assert.equal((await tool()).staged, true, `staged (${(await tool()).capture_failure})`);
    assert.ok("bones" in captured, "the capture's fingers apply");
    assert.ok(Math.abs(aimOff(captured) - 20) < 0.05,
      `the capture keeps the previous grip's aim: ${aimOff(captured)} deg`);
    assert.ok(distance(xyz(captured.offset), xyz(skewed.offset)) > 1e-3, "and takes the hand's position");

    // X test-holds it: the right controller holds the gun as play would.
    await game.input.setMany({ "right_hand.joints": null, "right_hand.position": controller.position });
    await game.input.trigger("LeftHandLowerButton");
    await game.step({ frames: 2 });
    let posed = await tool();
    assert.deepEqual(posed.test_hold, { tracked: true });
    assert.ok(Math.abs(posed.aim_error_deg! - 20) < 0.05, `aim error ${posed.aim_error_deg}`);
    assert.equal(posed.panel.status, `Aim off by ${posed.aim_error_deg!.toFixed(1)} deg`);
    const aimLines = (await game.scene.objects()).objects.filter((o) => o.source === "grip_workbench_aim");
    assert.equal(aimLines.length, 4, "two lines, halo and core each");
    // Put down (untracked), it says so and draws no lines.
    await game.input.set("right_hand.tracked", 0);
    await game.step({ frames: 2 });
    posed = await tool();
    assert.deepEqual(posed.test_hold, { tracked: false });
    assert.equal(posed.aim_error_deg, null);
    assert.equal(posed.panel.status, "Pick up the Right controller");
    await game.input.set("right_hand.tracked", 1);
    await game.step({ frames: 2 });

    // Align aim fires it down the controller.
    await aimAtButton(game, "left", "AlignAim", unit);
    await game.step({ frames: 1 });
    await click(game, "left");
    posed = await tool();
    assert.ok(posed.aim_error_deg! < 0.05, `aligned: ${posed.aim_error_deg}`);
    assert.ok(aimOff((await rightGrip()).grip!) < 0.05);
    assert.equal(posed.staged, true);
    await saveOnPanel(game, "left", unit);
    assert.equal((await tool()).saved, true);
    const saved = JSON.parse(readFileSync(path, "utf8")) as { entries: { model: string; hand: string; grip: ResolvedGrip }[] };
    assert.deepEqual(saved.entries.map((e) => [e.model, e.hand]), [["sg_h", "right"]]);
    assert.ok(aimOff(saved.entries[0]!.grip) < 0.05, "saved aligned");

    // Leave, drop it and pick it up again: in play it fires down the controller.
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 5 });
    assert.equal((await rightGrip()).pose_tool, null);
    // A fresh squeeze, then letting go, drops it.
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.notEqual((await game.info()).player.right_hand_entity_id, gun.id, "dropped");
    await holdShotgun(game);
    const regrabbed = (await rightGrip()).grip!;
    assert.ok(aimOff(regrabbed) < 0.05, `re-grabbed aim error ${aimOff(regrabbed)} deg`);
    // The shot frame gameplay fires along runs down the controller.
    await game.step({ frames: 30 });
    const { player } = await game.info();
    const aim = quatRotate(quatMultiply(player.rotation, (await game.input.state()).right_hand.rotation), [0, 0, -1]);
    const fires = muzzleFrameOf(await game.entities.detail(gun.id)).forward;
    const shotOff = (Math.acos(Math.min(1, dot(normalize(fires), normalize(aim)))) * 180) / Math.PI;
    assert.ok(shotOff < 1, `shots leave ${shotOff} deg off the controller`);
  },
);

/** `handInMetres` with the index finger hooked round a trigger. */
function triggerHandInMetres(): Vec3[] {
  const joints = handInMetres();
  joints[8] = [-0.025, -0.035, -0.11];
  joints[9] = [-0.025, -0.055, -0.095];
  joints[10] = [-0.025, -0.06, -0.075];
  return joints;
}

type Bones = ResolvedGrip["rotation"][];
/** Bone poses equal as rotations (q and -q alike). */
function assertSameBones(actual: Bones | null | undefined, expected: Bones, label: string): void {
  assert.equal(actual?.length, expected.length, label);
  actual!.forEach((q, i) => {
    const d = Math.abs(dot4(quat(q), quat(expected[i]!)));
    assert.ok(d > 1 - 1e-5, `${label}: bone ${i} off (dot ${d})`);
  });
}
const fingers = (grip: ResolvedGrip) => grip as { bones?: Bones; trigger_bones?: Bones };

test(
  "grip pose tool: the trigger slot snapshots the pulled fingers, which play blends to with the trigger",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    const path = join(data.root, "vr-grip-overrides.json");
    let saved: ResolvedGrip;
    {
      await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
      const rightGrip = async () => (await game.info()).player.hand_grips.find((g) => g.hand === "right")!;
      const gun = await holdShotgun(game);
      await game.input.set("head.position", [0, 1, 0]);
      await game.step({ frames: 2 });
      const unit = await metresPerWorldUnit(game);
      await game.input.trigger("ToggleGripPoseTool");
      await game.step({ frames: 2 });
      const tool = async () => (await rightGrip()).pose_tool!;
      assert.equal((await tool()).slot, "rest");
      assert.equal((await tool()).panel.slot_disabled, null);

      // Rest: the tracked right hand round the grip, the controller set down.
      const controller = (await game.input.state()).right_hand;
      const { toLocal } = await workbench(game);
      const bench = (await game.scene.objects({ entityId: gun.id })).objects[0]!.position;
      const onGun = (hand: Vec3[]) => hand.map((p) => toLocal(add(bench, scale(add(p, [0.1, -0.05, 0.05]), 1 / unit))));
      await game.input.setMany({
        "right_hand.squeeze": 0,
        "right_hand.position": [controller.position[0] + 1, controller.position[1] - 0.5, controller.position[2]],
        "right_hand.joints": onGun(handInMetres()),
      });
      await aimAway(game, "left");
      await game.step({ frames: 30 });
      await click(game, "left");
      assert.equal((await tool()).staged, true, `rest staged (${(await tool()).capture_failure})`);
      const rest = (await rightGrip()).grip!;
      assert.ok(fingers(rest).bones, "the rest capture is bones");
      assert.equal((await tool()).has_trigger_pose, false, "no pulled pose yet");

      // Trigger slot: the index hooked; only the fingers change.
      await aimAtButton(game, "left", "Slot", unit);
      await game.step({ frames: 1 });
      await click(game, "left");
      await aimAway(game, "left");
      assert.equal((await tool()).slot, "trigger");
      await game.input.set("right_hand.joints", onGun(triggerHandInMetres()));
      await game.step({ frames: 30 });
      await click(game, "left");
      const posed = (await rightGrip()).grip!;
      assert.equal((await tool()).has_trigger_pose, true, `trigger staged (${(await tool()).capture_failure})`);
      assertClose(posed.offset, rest.offset, 1e-5, "offset");
      assertClose(posed.rotation, rest.rotation, 1e-6, "rotation");
      assert.equal(posed.item_scale, rest.item_scale);
      assertSameBones(fingers(posed).bones, fingers(rest).bones!, "rest fingers kept");
      const pulled = fingers(posed).trigger_bones!;
      const moved = pulled.some((q, i) => Math.abs(dot4(quat(q), quat(fingers(rest).bones![i]!))) < 0.99);
      assert.ok(moved, "the pulled fingers differ from the rest");

      // Save writes both; leaving, dropping and re-grabbing loads them.
      await saveOnPanel(game, "left", unit);
      saved = (JSON.parse(readFileSync(path, "utf8")) as { entries: { model: string; grip: ResolvedGrip }[] })
        .entries.find((e) => e.model === "sg_h")!.grip;
      assert.equal(fingers(saved).bones?.length, 24);
      assert.equal(fingers(saved).trigger_bones?.length, 24);
      await game.input.setMany({ "right_hand.joints": null, "right_hand.position": controller.position });
      await game.input.trigger("ToggleGripPoseTool");
      await game.step({ frames: 5 });
      assert.equal((await rightGrip()).pose_tool, null);
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 3 });
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 5 });
      assert.notEqual((await game.info()).player.right_hand_entity_id, gun.id, "dropped");
      await holdShotgun(game);

      // The glove's fingers follow the trigger from the rest pose to the pulled one.
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 2 });
      let held = await rightGrip();
      assert.equal(held.visual_trigger, 0);
      assertSameBones(held.finger_bones, fingers(saved).bones!, "released: the rest pose");
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 2 });
      held = await rightGrip();
      assert.equal(held.visual_trigger, 1);
      assertSameBones(held.finger_bones, fingers(saved).trigger_bones!, "pulled: the trigger pose");
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 2 });
    }

    // A fresh run loads both poses from the overrides.
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await holdShotgun(game);
    const loaded = (await game.info()).player.hand_grips.find((g) => g.hand === "right")!.grip!;
    assertSameBones(fingers(loaded).bones, fingers(saved).bones!, "restart: rest");
    assertSameBones(fingers(loaded).trigger_bones, fingers(saved).trigger_bones!, "restart: trigger");
  },
);
