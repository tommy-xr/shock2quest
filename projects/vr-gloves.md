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

Test headlessly with `cargo dbgr --vr --mission earth.mis`: the debug runtime's
`POST /v1/control/input` accepts `{left,right}_hand.position [x,y,z]`
(pawn-local), `.rotation [x,y,z,w]`, `.trigger`, and `.squeeze` channels, so
hand placement and finger state are fully scriptable for screenshots.

## Weapon grip (#352 follow-up)

While a hand is holding an entity (`HandState::Grabbing`), the glove switches
from input-driven blending to a grip pose: fingers wrapped on the handle,
thumb locked, and the index resting on the trigger, curling with
`trigger_value` (constants in `hand_glove.rs`, tuned visually). Held-model
placement offsets are hand-local (`VRHandModelPerHandAdjustments::with_offset`,
mirrored by `flip_x`) and were tuned per weapon against screenshots: pistol,
assault rifle, shotgun, and wrench have fitted grips; the remaining weapons
use the generic held-weapon placement. Note the model long axes differ per
weapon (pistol/AR along model Y, wrench along model X), so offset axes are
per-model — probe with exaggerated single-axis offsets when tuning a new one.

## Bare hands + sleeved forearm (#950, first pass)

The glove *mechanics* are unchanged; only its look is. Three parts:

- **Skin instead of glove.** `hand_glove.rs` skins the same GLB with
  `assets/vr_hand_skin.png` rather than `vr_glove_color.jpg`. The hand has to be
  textured in the *glove's* UV atlas, so the skin map is that atlas recoloured:

  ```
  detail = luminance / gaussian_blur(luminance, size/32)   # local relief only
  texel  = skin_tone * clamp(detail, 0.86, 1.14) ** 0.6    # softened by 1.5 px
  ```

  which keeps the glove's baked creases, wrinkles and seam shadows - the thing
  that stops a hand reading as plastic - while dividing out its albedo, so black
  leather and white straps don't land as light and dark patches of skin.
  `skin_tone` is the mean skin colour of the game's own first-person hand
  texture `res/obj/txt16/HRPistArm.gif` — `(185, 139, 124)` — **divided by 1.5**:
  both material shaders composite `texel * 0.5` (ambient) `+ texel * emissivity`
  and the hands render at emissivity 1.0, so an undivided tone clips at white
  and washes out. `load_hand_skin` is the single loader, used by `debug_gloves`
  too, so the harness cannot drift onto a different skin from the production
  hands.

  Two approaches were tried and rejected first: the game's own hand texture
  (`HRPistArm.gif`) through the glove's UVs renders **magenta** - the glove's
  atlas lands on that texture's transparent background, its layouts being
  unrelated - and a flat skin tint reads as rough, featureless plastic.
- **A sleeved forearm** (`shock2vr/src/hand_forearm.rs`). A capped tube (radius
  0.5, z = 0..1, 16 segments) running from 2 cm *inside* the wrist to 8.5 cm
  toward the elbow, 6.4 cm across, rigidly following the hand pose — no elbow, no
  IK. It wears the game's own suit cuff: `FISTCOMP.PCX`, the texture the `*_h`
  first-person hand models wear, whose top ~42% is the ribbed sleeve and whose
  remainder is bare skin. The tube's UVs sample only that band, running it
  cuff-edge-at-the-wrist to deeper-sleeve-at-the-elbow, and the caps sample a
  single texel (a `u` interpolated across a fan wedge would draw the ribbing as
  concentric rings). Resolution goes through `dark::util::resolve_texture_name`,
  so a 25AE or mod install's upgraded encoding of that same texture wins
  automatically and a classic install finds the original in `obj.crf` — no new
  art is committed for the sleeve.
  Two numbers are load-bearing and unit-tested: the tube starts inside the hand
  so a turning wrist never opens a gap, and it stops before the forearm HUD
  panel's **near edge** — the panel lies *along* the arm, centred on its axis and
  26 cm wide, so its near edge is at `FOREARM_OFFSET.z - HUD_PANEL_WIDTH / 2`
  = 9.1 cm, not at its 19 cm centre. (Guarding against the centre passed while
  the tube swallowed a third of the panel; the test now derives the bound from
  `hud::virtual_arms`' own constants.)
- **A weapon replaces the hand.** `virtual_hand::shows_hand_visual` hides the
  hand *and* its sleeve while the hand holds a wieldable weapon (`PropPlayerGun`
  / `PropLimbModel`), because the weapon's own model is drawn at the same
  transform - previously the glove rendered *inside* the gun. An empty hand, or
  one holding anything without a first-person weapon model, still shows. Hiding
  the sleeve too is the owner's scoped rule ("the weapon model renders as-is and
  replaces the hand visual"), not an oversight.

Deferred (see #950): pose switching, point-on-hover, procedural auto-grab,
elbow IK, and the 25AE authored hand models.

## Follow-ups

- On-headset tuning of `grip_rotation()` / wrist offset once tested in a real
  HMD (alignment was tuned visually through the flat debug capture path).
- More poses if needed (`fallback_relaxed`, pinch) — transcribe like the fist.
- The sleeve is unlit and its shading rides with the wrist, so rolling the
  forearm rotates the ribbing's highlight rather than leaving it with the light.
- The glove mesh keeps its strap and cuff-flap geometry under the bare skin;
  removing them needs mesh surgery, not a texture swap.
