import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { findRepoRoot, GameServer, setHandWorldPose } from "../src/index.js";
import type { ResolvedGrip, Vec3 } from "../src/types.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";
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
    // The workbench is a scene of its own: none of the mission is drawn (its
    // world, labels, HUD), only the mug, the hands, a floor, a backdrop and
    // the control panel.
    const benchScene = (await game.scene.objects()).objects;
    const strays = benchScene
      .filter((o) => !["grip_workbench", "player_hands", "grip_workbench_panel", "grip_workbench_pointer"].includes(o.source ?? ""))
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
    // The bench plus three views: 4 copies of every joint.
    const VIEWS = 4;
    const JOINTS = 26;

    // Without hand tracking (controller-only, no multimodal) a capture says so.
    await pullTrigger();
    grip = await rightGrip();
    assert.equal(grip.pose_tool?.capture_failure, "no_joints");
    assert.equal(grip.pose_tool?.tip_error_mm, null, "no tracked hand, no tip error");
    // Untracked, the saved grip's glove holds the mug. By default only its
    // spheres touching or through the mug show; with nothing tracked there
    // are no raw joint markers.
    assert.ok((await named("Right grip glove")) > 0, "the saved grip's glove");
    assert.equal(await named("contact sphere free"), 0);
    assert.equal(await named("tracked joint"), 0);

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
    // hand's way. Its raw joints show markers on the wrist and fingertips.
    assert.equal(await named("Right grip glove"), 0, "the saved glove steps aside");
    assert.ok((await named("Right glove")) > 0, "for the live one");
    assert.equal(await named("contact sphere free"), 0);
    assert.equal(await named("tracked joint"), 6 * VIEWS);
    // How far each glove fingertip lands from the tracked one: this flat,
    // built hand's thumb to middle are within the glove's reach, so on them.
    const live = (await rightGrip()).pose_tool;
    const tipError = live?.tip_error_mm;
    assert.equal(tipError?.length, 5);
    assert.ok(tipError!.every((mm) => Number.isFinite(mm) && mm >= 0), `${tipError}`);
    assert.ok(tipError!.slice(0, 3).every((mm) => mm < 3), `thumb to middle on the hand: ${tipError}`);
    // About the glove's size.
    assert.ok(Math.abs(live!.hand_size_ratio! - 1) < 0.1, `${live?.hand_size_ratio}`);
    // The free (left) controller's thumbstick click steps to every sphere and
    // marker, then none, then back to contacts only.
    const clickStick = async () => {
      await game.input.set("left_hand.thumbstick_click", 1);
      await game.step({ frames: 1 });
      await game.input.set("left_hand.thumbstick_click", 0);
      await game.step({ frames: 1 });
      return (await rightGrip()).pose_tool?.contact_view;
    };
    assert.equal(await clickStick(), "full");
    assert.equal(await named("contact sphere"), JOINTS * VIEWS, "every live glove joint");
    assert.equal(await named("tracked joint"), JOINTS * VIEWS);
    assert.equal(await clickStick(), "off");
    assert.equal(await named("contact sphere"), 0);
    assert.equal(await named("tracked joint"), 0);
    assert.equal(await clickStick(), "contacts");
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
    // A (the right controller's lower button) saves it for the left hand.
    await game.input.trigger("RightHandLowerButton");
    await game.step({ frames: 2 });
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
    const { mug, unit } = await openWorkbench(game);
    const tool = async () => (await workbench(game)).tool;
    const haptics = async () => (await game.info()).player.hand_feedback!.haptics!;

    // The panel is drawn while the workbench is open, and names the free
    // (left) controller's own buttons.
    const panelDraws = (await game.scene.objects()).objects.filter((o) => o.source === "grip_workbench_panel");
    assert.ok(panelDraws.length > 0, "the panel is drawn");
    assert.equal((await tool()).panel.status, "Pose, then snapshot");
    // Nothing staged: Save and Discard are disabled.
    assert.deepEqual((await tool()).panel.panel!.buttons.map((b) => b.event), ["SwitchHand", "Contacts", "ResetView", "Exit"]);
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
    await game.input.setMany({ "left_hand.joints": null, "left_hand.tracked": 1 });
    await game.step({ frames: 1 });

    // The tracked hand pushed into the mug buzzes the free controller, and
    // stops when contacts are hidden.
    const mugDraw = (await game.scene.objects({ entityId: mug.id })).objects[0]!;
    const into = handInMetres().map((p) => toLocal(add(mugDraw.position, scale(sub(p, [0, 0, -0.09]), 1 / unit))));
    await game.input.set("right_hand.joints", into);
    await game.step({ frames: 2 });
    const buzzing = await haptics();
    const penetrating = (await game.scene.objects()).objects.filter((o) => o.name === "contact sphere penetrating").length;
    assert.ok(penetrating > 0, "joints go through the mug");
    assert.ok(buzzing.last[0]!.amplitude > 0 && buzzing.last[0]!.amplitude <= 0.35 + 1e-6, `${JSON.stringify(buzzing.last[0])}`);
    await aimAtButton(game, "left", "Contacts", unit);
    await click(game, "left");
    await click(game, "left");
    assert.equal((await tool()).contact_view, "off", "the panel steps contacts: full, off");
    const quiet = (await haptics()).sequence[0];
    await game.step({ frames: 5 });
    assert.equal((await haptics()).sequence[0], quiet, "no buzz with contacts off");
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
    // hand and A saves it.
    await game.input.setMany({
      "right_hand.joints": null,
      "right_hand.position": hand,
      "left_hand.joints": trackedHand([hand[0] - 0.4, hand[1], hand[2]], unit, { left: true }),
    });
    await aimAway(game, "right");
    await game.step({ frames: 30 });
    await click(game, "right");
    assert.equal((await workbench(game)).tool.staged, true);
    await game.input.trigger("RightHandLowerButton");
    await game.step({ frames: 2 });
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
    await aimAtButton(game, "left", "Contacts", unit);
    await click(game, "left");
    assert.equal((await tool()).contact_view, "full", "the ray clicked Contacts");
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

test(
  "grip pose tool panel: a handed weapon cannot switch the bench to the other hand",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    using _data = isolatedDataRoot([
      "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
    ]);
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const pistol = (await game.entities.list()).entities.find((e) => e.template_id === -17)!;
    await aimVrHandAt(game, pistol.position, 0.2, 1, 0, { hand: "right" });
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, pistol.id);
    await game.input.set("head.position", [0, 1, 0]);
    await game.input.trigger("ToggleGripPoseTool");
    await game.step({ frames: 5 });
    const { tool, hand } = await workbench(game);
    assert.equal(hand, "right");
    // Its left-hand model is the mirror image: a grip for that hand would be
    // posed against the wrong mesh.
    assert.equal(tool.panel.switch_disabled, "hold it in that hand");
    assert.ok(!tool.panel.panel!.buttons.some((b) => b.event === "SwitchHand"));
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
  await game.input.trigger("LeftHandLowerButton");
  await game.step({ frames: 2 });
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
