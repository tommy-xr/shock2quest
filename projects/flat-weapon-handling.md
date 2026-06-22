# Flat-runtime weapon handling & feel

Status: planning (2026-06-20). Follow-on to the flatscreen runtime (slices 1-6,
#295-#305). The flatscreen runtime is now the **default desktop experience**, so
the first-person weapon loop is the most-exercised gameplay code and worth
getting right. This doc collects the aiming/viewmodel polish work and the
"modern FPS feel" wishlist into one place.

See also: `projects/flatscreen-and-vr-architecture.md` (the intent/outcome seam
that all of this must respect), `projects/melee-combat.md`,
`projects/player-damage.md`.

## Status / progress (2026-06-21)

Landed on branch `feat/flat-aim-viewmodel` (PR: flat first-person weapons — aim +
viewmodel):

Done:

- **Camera-origin aim** — flat shots spawn at the camera and travel straight
  along the crosshair, for both hitscan bullets and slow physics projectiles
  (e.g. the grenade). `RuntimePropFlatAim` carries the fire ray; the flat
  controller sets it on the wielded weapon and `weapon_script::create_projectile`
  consumes it (the "fire intent" seam above). VR/AI weapons are unchanged.
- **First-person viewmodel from `PropPlayerGun.hand_model`** — render each
  weapon's native FP mesh, dropping the inconsistent VR `HAND_MODEL_POSITIONING`
  table. Fixes the per-weapon rotation (shotgun / EMP / assault / psi-amp were
  sideways or facing the player). NB: `PropPlayerGun.heading` is NOT the model
  rotation — applying it over-rotates each gun by exactly its heading — so it is
  deliberately unused.
- **Eye-height alignment** — the shot origin + viewmodel use the shared
  `shock2vr::PLAYER_EYE_HEIGHT` (4.0 SS2 units), matching every runtime's render
  camera, so shots land on the crosshair and the debug-runtime camera matches
  desktop (was rendering ~1 world unit low).
- **Tooling**: debug-runtime controllable input over HTTP (`head.look` + hand
  trigger; the render camera now honors the input head rotation), a `CycleWeapon`
  action (`B` / HTTP) over the 11-weapon roster, and the `debug_weapons` scene (a
  wall straight ahead) for headless aim verification.

Verified: the pistol hit-spang and the grenade land dead-center on the crosshair
(`debug_weapons`); all 11 player weapons render their FP mesh oriented correctly.

Remaining (follow-ups):

- **TODO(fable model): per-weapon viewmodel framing from `PropPlayerGun.model_offset`.**
  Currently the flat viewmodel uses a single shared `VIEWMODEL_OFFSET` for every
  weapon (playable: all 11 frame bottom-right with the crosshair clear).
  Investigated wiring per-weapon `model_offset` and it does NOT reduce to one
  scalar — revisit with a fresh approach (a Fable-model pass). Findings:
  - Axis mapping that works: Dark view space `(x=forward, y=left, z=up)` ->
    look space `(+x right, +y up, -z forward)` = `vec3(-mo.y, mo.z, mo.x)`.
  - **FOV mismatch is the blocker.** SS2 authors the *pistol* more cornered
    (`model_offset` ~38° down-right) than the *big guns* (EMP/AR ~18°). Our
    renderer's FOV clips the cornered pistol, while we'd want the opposite for
    playability. So:
    - a uniform scale on the whole offset keeps each weapon's authored angle ->
      big guns frame great (bottom-right, reticle clear) but the pistol clips
      off the corner at any scale;
    - a forward-only gain (`mo.x * ~2`) pulls the pistol in nicely but centers
      the big guns over the crosshair ("breaks visibility");
    - `/SCALE_FACTOR` puts every gun ~0.4 world units from the eye (fills view).
  - Likely correct fix: render the viewmodel pass with its own **wider FOV /
    separate projection** (standard FPS technique), then faithful `model_offset`
    frames all weapons as authored. Verify per weapon in `debug_weapons`.
  - Ref: `darkengine` `PlayerGunDescGetModelOffset` / `m_posOffset`. The
    experiment code lived on the abandoned `feat/flat-viewmodel-offset` branch
    (see git reflog) if useful.
- **Muzzle flash is world-pinned** — created at the weapon's fire-time transform
  and not re-parented, so it doesn't track the weapon. Attach it to the muzzle
  vhot / render it in viewmodel space.
- **Melee weapons** — Wrench / Electro Shock / Crystal Shard / PsiSword.
  - Done: render their `PropLimbModel` FP mesh in the flat viewmodel (they have
    no `PropPlayerGun`), added to the CycleWeapon roster, a flat swing
    (no-projectile branch in `WeaponScript`: short raycast along the crosshair
    ray + `MELEE_DAMAGE`, damage verified on a live enemy), a closer melee
    viewmodel offset, the **idle pose**, and the **swing animation**. VR melee
    (physical-collision `MeleeWeapon`) is unchanged.
  - Remaining: derive damage from the weapon's `Melee Typ`; swing sound;
    camSynch (below). (Player melee `-928` etc. use `WeaponScript`, not the
    `wrench`->`MeleeWeapon` script, which belongs to the Maintenance Tool -2949.)

### First-person weapon animation (from the darkEngine reference)

The FP weapon is a **skeletoned actor (ActorType 1 = `PlayerLimb`)** driven by
the motion system, not a static prop. The original engine layers two motions:

1. **`camSynch`** (virtual, every frame) — bolts the arm's root joint to the
   camera: `armPos = camPos + camRot*posOffset`, `armRoot = angOffset ∘ camRot`.
   This **cancels the clips' root motion**; the relative joints carry the gesture.
2. **a gesture clip** — the idle (`+plyrmelee:0` = `ph212203`, the ready stance)
   or a swing (`+plyrmelee:2 +plyrmeleeswing` = `leftswing`/`rightswing`/
   `highswing`; shipped game uses left). Press=windup, release=swing.

Other reference facts: orientation/placement come from a **separate**
`sMPlayerLimbOffsets` property ("Arm Pos/Ang Offset"), NOT `PropPlayerGun`; no
FP-specific FOV/scale; melee damage opens a collision window on a swing keyframe
(`MF_TRIGGER1`) and lands on physical overlap.

What we implemented (this PR):
- Pose the FP mesh via an `AnimationPlayer` (was unskinned -> looked mid-swing).
- **Idle**: the static frame-0 of `ph212203` (head-up ready stance). Static
  because the looping clip carries root motion we'd otherwise need camSynch to
  cancel.
- **Swing**: `Effect::FlatMeleeSwing` plays `leftswing` once on attack, then
  returns to the static idle.

**TODO(camSynch / root override)** — the remaining orientation issue. Symptom:
the idle/swing arm hangs somewhat low and the arm *stub* (open cut end) is
visible, because the clip's root motion isn't cancelled (the original engine's
camSynch re-anchors the arm root to the camera every frame; we don't).

Investigation (how our animation system applies the root — `dark/src/`):
- A posed FP model has **two** root-motion sources, both in
  `ss2_skeleton::animate(skeleton, anim_info, additional_joint_transforms)`:
  1. the **root joint's** per-frame animation transform (the joint entry for the
     root bone in `animation_transforms`), and
  2. a **separate per-frame `root_transform`** (`AnimationClip.root_transforms[frame]`),
     passed into `calc_and_cache_global_transform(..., root_transform)` as the
     base of the whole hierarchy.
- `AnimationPlayer.additional_joint_transforms` **completely override** a joint's
  animation transform (`animate`: `animation_transforms.insert(joint, transform)`,
  commented "Have joint transforms completely override animation transforms").
  So overriding the root *joint* there cancels source (1) - but NOT (2).

Plan:
1. Find the FP skeleton's root joint id (the bone with no parent / id 0).
2. Set `additional_joint_transforms[root_joint] = identity` (or the desired arm
   root orientation) on the melee `AnimationPlayer` to cancel source (1). The
   `AnimationPlayer` builders already thread `additional_joint_transforms`; add a
   small constructor/setter for it.
3. Cancel/replace source (2): either feed an identity `root_transform` for the FP
   weapon path, or add a flag to `animate`/`get_transforms` to ignore
   `root_transforms`. (Cleanest: a "no root motion" mode on the player or a
   variant of `to_animated_scene_objects` for view models.)
4. Then re-anchor the arm to our viewmodel transform (the `SetPositionRotation`
   already positions the model; with root motion cancelled it should sit
   correctly) and verify: idle can loop animated without drift, arm stub
   off-screen, swing stays anchored.

Then also: derive the swing direction/length from hold time (medium vs. long),
and tie damage to the swing keyframe (`MF_TRIGGER1`) rather than a fixed raycast.
- **Crouch-accurate aim** — `PLAYER_EYE_HEIGHT` is the standing value; desktop
  crouch (1.5) lowers the camera but the flat controller's shot origin is fixed,
  so crouched shots land slightly high. Pass the actual eye height into the
  controller to fix.
- **Latent**: the Hybrid Shotgun (-4073) panics on creation (unimplemented
  `trashedshotgun` script) — excluded from the cycle roster; the
  panic-on-unknown-script is a separate crash risk worth its own issue.

The "feel" wishlist below (recoil, dynamic crosshair, reload animations, look
inertia, ammo) remains open.

## Architectural constraint (read first)

From the flatscreen architecture doc: **anything that affects simulation outcome
lives in the core and is expressed as a resolved intent**, not baked into a
runtime. Aim direction, recoil, accuracy/spread, and ammo consumption are all
simulation outcomes. The flat front-end's job is to *produce the intent* ("fire
a shot from point P toward crosshair convergence point C, with spread S"); the
VR front-end produces the same intent from hand pose. If we instead special-case
ballistics inside `flat_player_controller`, the same tuning will have to be
re-done for VR. Draw the line at intent, not device.

Concretely, this argues for a small **fire intent** that the controller emits and
the weapon script consumes, rather than the weapon script deriving everything
from its own transform (which is what produces the current aim divergence).

## Current state (grounding)

- `shock2vr/src/flat_player_controller.rs`
  - Wields one weapon as a viewmodel at `VIEWMODEL_OFFSET = (2.0, -2.5, -5.0)`
    (right/down/forward in look space) — `:28`.
  - Already casts a **camera-forward ray** each frame for the crosshair frob
    target (`:103-116`). This is the ray we want to aim shots along.
  - Fires by emitting `OutMessage{TriggerPull/Release}` to the weapon entity on
    the trigger edge — same effects the VR hand emits.
- `shock2vr/src/scripts/weapon_script.rs`
  - On `TriggerPull`, spawns the projectile and muzzle flash from the **weapon's
    own transform + vhot** (`create_projectile`, `create_muzzle_flash`).
  - Muzzle flash is `Effect::CreateEntity { position: vhot_offset,
    root_transform: transform.0 (at fire time) }` — a free entity pinned to the
    world pose captured at the trigger pull (`:155`). It does not re-parent to or
    track the weapon.
  - `// TODO: Handle setting or ammo type? This just picks the very first
    projectile` (`:54`) — ammo selection is unimplemented; the first
    `Link::Projectile` always wins.
- `shock2vr/src/input/dispatcher.rs`
  - `SpawnDebugItem` spawns a single hardcoded template (`-17`, a pistol; `:9`),
    bound to Space. No weapon cycling / number keys yet.

## Work items

### 1. Fix the aim direction (highest priority — correctness)

Symptom: shots leave the offset barrel along the weapon-model axis, so they do
not hit where the crosshair points. The viewmodel is intentionally offset
right/down for framing, which makes the divergence obvious at range.

Approach:
- Define a **fire intent** carrying the firing ray: origin at (or near) the
  weapon muzzle vhot, **direction toward the crosshair convergence point** — i.e.
  the camera-forward raycast hit point (reuse the ray the controller already
  computes at `flat_player_controller.rs:103`), falling back to a far point along
  camera-forward when the ray hits nothing.
- The weapon script spawns the projectile with that direction instead of the
  weapon's local barrel axis. (VR continues to supply muzzle-forward as its
  direction — same intent, different producer.)
- Decide the convergence model: true "barrel -> crosshair point" convergence
  (parallax-correct, shots cross the reticle at the hit distance) vs.
  "camera-origin ray" (shots always exactly on the reticle, barrel is cosmetic).
  Camera-origin is simplest and what most flat FPS do; revisit if it looks wrong
  for the viewmodel.

**Debug raycast visualization (do this first — it's the measurement tool):**
- Add a debug flag / `InputAction` that draws the fire ray: a line from the
  origin (weapon muzzle / camera) through the crosshair to the hit point, plus a
  marker at the hit. This makes "where is it actually pointing vs. the crosshair"
  directly visible and is reusable for tuning every later item.
- Drive it headlessly via the debug runtime (screenshot + `/v1/physics/raycast`)
  so aim regressions are catchable without an interactive session.

### 2. `debug_weapons` scene + weapon cycling (the test rig)

We need to exercise many weapons quickly without hunting them in a mission.

- New debug scene `debug_weapons` (in `shock2vr/src/scenes/`, registered like the
  others) that spawns the player in a simple room with the full weapon set
  available.
- Flat-runtime weapon cycling on number keys **1-9 and 0**: add
  `InputAction::SelectWeapon(n)` (or ten discrete actions, matching the existing
  `all()`/`as_str()` pattern in `input/actions.rs`), map to wielding the n-th
  weapon in the dispatcher, bind digits in `DesktopInputMapper`. Per the input
  doc this is then automatically HTTP/SDK-triggerable.
- Seed a canonical weapon template list (the SS2 player weapons). Reuse for the
  `SpawnDebugItem` family so debug spawns aren't a single hardcoded pistol.

### 3. Ammo usage / ammo switching

Currently `weapon_script.rs:54` always fires the first projectile link and never
decrements anything.

- Track ammo on the player (or weapon) state. SS2 models ammo as items/links;
  resolve the equipped projectile from an **ammo-type selection** rather than
  "first link wins."
- Consume a round per shot; block fire (and play dry-fire) at zero.
- Ammo switch input (cycle ammo types for the held weapon — many SS2 guns take
  multiple ammo types, e.g. standard vs. AP).
- Surface current ammo in the flat HUD (we already have the `UiCanvas` HUD layer
  from #300 — add an ammo readout next to health/psi).
- Reload: see weapon animations below; reload is where ammo, animation, and input
  meet.

### 4. Muzzle flash should follow the weapon

Bug: the muzzle flash is spawned as a free entity at the weapon's fire-time world
pose (`weapon_script.rs:155`), so during its (brief) lifetime it stays put while
the weapon keeps moving (sway/recoil/look). Visible as the flash detaching from
the barrel.

Options (cheapest first):
- Parent/attach the flash to the weapon's muzzle vhot for its lifetime so it
  tracks the weapon transform, instead of snapshotting `transform.0` once.
- Or render the flash as a short-lived viewmodel-space effect (like the weapon
  viewmodel itself, which is re-placed every frame in `render_per_eye`).
- Same concern applies to any short-lived attached effect (shell ejection, smoke)
  added later.

## Feel wishlist (modern-FPS-of-the-era polish)

These are what made late-90s/early-2000s shooters feel good; SS2's stats give us
real inputs to drive them.

- **Dynamic crosshair / accuracy.** Spread widens when moving/jumping, tightens
  when still/crouched. Drive from SS2 stats already in the data: agility,
  strength, and per-weapon accuracy stats. The crosshair *shows* the current
  spread (reticle bloom). Spread is applied as a randomized cone on the fire
  intent (item 1), so it stays a sim outcome, not a render trick.
- **Recoil — spring/physical model.** Prefer a spring-damper kick (per-shot
  impulse to a viewmodel + aim offset that decays back) over a scripted curve, so
  sustained fire walks the aim and settles naturally. Ties into the dynamic
  crosshair (recoil feeds spread). Recoil is an aim *outcome*, so it belongs in
  the intent/core layer (VR two-handed bracing reduces it — same knob).
- **Weapon animations (reload / attack / swing / idle).** Several weapons ship
  with these animations; we currently don't play them. Hook the viewmodel to play
  the weapon's reload/fire/idle clips. Reload animation gates the ammo refill
  (item 3); swing animations feed melee (`projects/melee-combat.md`).
- **Look inertia / weapon sway.** GoldenEye/Perfect Dark/TimeSplitters-style lag:
  the viewmodel trails the camera on fast turns and eases back, plus a subtle idle
  bob. Pure viewmodel-space transform on top of `VIEWMODEL_OFFSET`; does not
  affect the fire intent (cosmetic), but should be considered alongside recoil so
  they compose rather than fight.

## Proposed sequencing

1. **Debug fire-ray visualization** (item 1's tool) — measurement first.
2. **Fix aim direction** via a fire intent aimed at the crosshair convergence
   point — the core correctness fix; unblocks honest testing of everything else.
3. **`debug_weapons` scene + 1-9/0 cycling** — the rig to validate aim/feel
   across the whole arsenal.
4. **Muzzle flash follows weapon** — small, self-contained visual fix.
5. **Ammo usage + HUD readout + ammo switch** — gameplay completeness.
6. **Feel pass**: recoil spring -> dynamic crosshair -> look inertia ->
   weapon animations (reload ties ammo + animation together).

Items 1-4 are bounded and headlessly verifiable via the debug runtime; the feel
pass (6) is interactive-tuning-heavy and should be gated so it doesn't absorb all
the energy (the flatscreen doc's warning: polish flat *enough to be a faithful
instrument*, reserve real polish for the VR product).
