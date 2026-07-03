# VR Gloves: SteamVR Hand Poses on the Glove GLB

Status: **working** — programmatic hand poses (open, point, fist, and blends)
apply correctly to the SteamVR glove model. Verified by unit tests
(`shock2vr/src/scenes/hand_pose.rs`) and visually in the `debug_gloves` scene.

## Assets

- `assets/vr_glove_model.glb` — right-hand glove, adapted from Valve's
  [SteamVR Unity plugin](https://github.com/ValveSoftware/steamvr_unity_plugin)
  (FBX2glTF export). 26 skin joints + 5 non-skinned aux nodes.
- `assets/vr_glove_color.jpg` — external color texture.
- Pose data in `shock2vr/src/scenes/hand_pose.rs`, transcribed from the
  plugin's `SteamVR_Skeleton_Pose` assets (right hand): `ReferencePose_OpenHand`
  (identical to `ReferencePose_BindPose`), `fallback_point`, `fallback_fist`.

## The three bugs that broke previous attempts (PRs #214–#224, Nov 2025)

Symptom back then: "skinning data causes a zoom" / exploded fingers. Three
independent problems stacked:

1. **Double scale.** FBX2glTF baked a ×100 unit conversion twice: as a scale on
   the `Root` joint *and* on the `renderMesh0` node. The importer baked the
   mesh-node transform into the skinned vertices, and the joint skinning
   matrices (global × inverse-bind) carry the Root's ×100 as well — the IBMs
   are authored without it — so real skinning rendered the glove ~100× too big.
   The unposed glove only ever looked right because it was drawn *without*
   skinning matrices. Fix: per the glTF spec, skinned meshes ignore the mesh
   node's transform (`dark/src/importers/glb_model_importer.rs`), and
   `GlbModel::new` now bakes bind-pose skinning into its scene objects so
   unposed clones render correctly too.

2. **Whole-transform pose application.** Poses were written as
   translation×rotation local transforms, replacing the bind translations
   (bone lengths) and the Root's scale. Fix:
   `GlbAnimationState::set_joint_rotation` writes *rotation only*, preserving
   bind translation and scale — bones can't stretch by construction.

3. **Wrong coordinate conventions.** The pose data is authored in Unity
   (left-handed) on Valve's hand rig; the GLB is right-handed and FBX2glTF
   re-parameterized every joint's local frame by a constant rotation (bones
   point down −X in the GLB, +X in the pose data; each joint frame differs by
   up to ~44°). A single global mirror is not enough.

## The retargeting solution (`HandPoseRetarget`)

Each GLB joint's frame differs from the pose data's frame by a constant basis
change `C_j`. Since the GLB's bind pose and the plugin's reference bind pose
are the same physical hand pose, the `C_j` are recovered recursively from the
two bind poses:

```
C_j = mirror(reference_bind_rotation_j)⁻¹ · C_parent(j) · glb_bind_rotation_j
```

where `mirror` is the handedness conversion (translation `x → −x`; quaternion
`(x,y,z,w) → (x,−y,−z,w)`), seeded at the wrist with a constant rotation
(~2.8°) solved once by a Kabsch fit aligning the five metacarpal bind offsets
of the two rigs. Poses then map on per joint as:

```
glb_local_rotation_j = C_parent(j)⁻¹ · mirror(pose_rotation_j) · C_j
```

Only joints 2..=25 (fingers) are posed. Joint 0 (`Root`) keeps the exporter
scale; joint 1 (wrist) keeps bind — its pose transform is the hand-to-
controller offset, which the scene/hand attachment supplies instead.

By construction the reference (open/bind) pose reproduces the GLB bind exactly
— that's the calibration invariant the unit tests assert, along with bone
lengths being preserved under every pose and the fist actually curling.

`Pose::blend(a, b, t)` slerps per-bone for analog states (grip squeeze,
trigger pull).

## Verifying

```bash
cargo test -p shock2vr hand_pose         # invariants, headless
cargo dbgr --mission debug_gloves --port 8080
# line-up left→right: bind, open, point, fist
curl -X POST http://127.0.0.1:8080/v1/step -d '{"frames": 30}'
curl -X POST http://127.0.0.1:8080/v1/screenshot -d '{"filename": "gloves.png"}'
```

## VR hands (`shock2vr/src/hand_glove.rs`)

In VR mode, `VirtualHand::render` draws the posed glove at each hand's
transform instead of the old green debug cube. The pose is driven per frame
from the controller's analog inputs via `Pose::blend_per_finger`: the index
finger follows the trigger, the other fingers (and thumb) follow the squeeze.
The left hand mirrors the right-hand model with a negative-X scale (the same
`flip_x` trick held weapons use). The hand frame's forward is **-Z** (the
raycast/aim direction — see `VirtualHand::update`), while the glove model's
fingers point along +Z, so the grip alignment yaws the model 180° to line the
fingers up with where the hand points. This was validated against the raycast
hit markers in-game (fingers must point at the hand's own hit cube).

Desktop `--vr` controls: hold **E** (right hand) or **Q** (left hand) to
possess a hand — the mouse then drives it (move = aim, LMB = trigger,
RMB = squeeze). Bare clicks do nothing to the hands in VR mode.

Test headlessly with `cargo dbgr --vr`: the debug runtime's
`POST /v1/control/input` accepts `{left,right}_hand.position [x,y,z]`
(pawn-local), `.rotation [x,y,z,w]`, `.trigger`, and `.squeeze` channels, so
hand placement and finger state are fully scriptable for screenshots.

## Follow-ups

- On-headset tuning of `grip_rotation()` / wrist offset once tested in a real
  HMD (alignment was tuned visually through the flat debug capture path).
- Hide or relax the glove pose when a weapon is wielded, if the fist-around-
  weapon look needs it.
- More poses if needed (`fallback_relaxed`, pinch) — transcribe like the fist.
