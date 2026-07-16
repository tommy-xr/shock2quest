# Ragdoll Settling & Hip Connection — Follow-up Plan

Hand-off doc for continuing the ragdoll work. Goal: a believable corpse that
**settles** and whose limbs (notably the **hips**) stay connected, while remaining
**interactive** (you can poke/push the body — that's part of the fun).

See also `projects/ragdoll.md` (the main ragdoll doc + earlier 2026-06-15/06-18
investigation logs). Current frontier: the **2026-07-16 re-evaluation** below —
the multibody rig now settles and is the graduation candidate.

## Where we are now (done / merged / in PR #301)

- **rapier 0.19 → 0.31** (on `main`): solver rework + per-joint softness. The ragdoll
  uses **impulse joints** (`GenericJoint`, `LOCKED_SPHERICAL_AXES` + cone limits +
  compliant `SpringCoefficients { natural_frequency: 60, damping_ratio: 2.0 }`).
- **Joint dedupe** (on `main`): the Dark skeleton lists some joints twice (a torso's
  main joint is also a fixed point on the parent torso, see `ss2_skeleton::create`);
  we now build exactly one joint per child body.
- **Collider sanitize** (on `main`): clamps degenerate (0 / ∞) collider sizes — fixed
  an earth.mis crash on the 0.31 BVH broad-phase.
- **Deterministic fixed-60Hz `/v1/step`** (on `main`).
- **`/v1/physics/joints`** endpoint (on `main`): per-joint anchor separation + applied
  impulse, **labeled by bone**.
- **PR #301** (`fix/ragdoll-hip-joint`):
  - **Joint-anchor visualization** under `--debug-physics`: cyan cross = parent anchor,
    magenta cross = child anchor, yellow line = the gap. Overlapping crosses + no line
    = satisfied joint; a visible gap = separation.
  - **Heavier "core" mass** (`CORE_BODY_MASS = 4.0`): the cuboid core bodies (pelvis
    hub, abdomen, torso) get 4× the limb mass. **This made the rig settle** — see below.

## The two remaining problems

1. **One hip joint still separates ~9 cm under load** (the other hip is tight ~2.5 cm;
   it's asymmetric / depends on the final pose). Impulse joints inherently allow the
   locked-translation constraint to be violated under the thigh's weight.
2. **Slow settle (~40 s of sim time)** — with the heavier core the rig *decays* to rest
   (max speed → ~0.07) instead of jittering forever, but the heavy core has momentum so
   it takes a while; it twitches for a bit before going still.

## Investigation log — what we found and what we ruled out

**Root cause of "never settles":** with `/v1/physics/joints` we saw the impulse joints
holding a **permanent unsatisfiable residual at rest**. The skeleton is a hub-and-spoke
tree whose pelvis "hub" (bone 8) is a ~4 cm, near-zero-inertia box carrying the two
**heavy thigh pendulums** (bones 6/7). The tiny hub gets yanked around; the constraint
solver keeps correcting and pumps energy → the whole rig jitters at 3–7 m/s forever.
The hip gap is the same root cause (the heavy thigh stretches the compliant joint).

**Tried and RULED OUT (don't repeat):**
- Compliant impulse joints (60 Hz / 2.0): reduced the violence vs the near-rigid default
  but the rig still jittered 3–7 m/s and the hips gapped ~10 cm.
- **Multibody (reduced-coordinate) joints as a drop-in:** *oscillate* with `softness`
  (a spring on a reduced-coord joint), and *explode* (max speed → 4e11) without it. Cause:
  our bodies are placed **independently at the death pose**, but a multibody joint snaps
  the child to the reduced-coordinate rest config relative to the parent at spawn — the
  mismatch injects enormous energy. **Not a drop-in** (see step 3 below for the fix).
- Solver iterations (16), higher damping (up to 1.0/4.0), self-collision on/off, bumping
  the hub `MIN_HALF_EXTENT`: each helped marginally at best; none settled it.
- **Heavier core mass: WORKED.** 4× the limb mass on the core cuboids makes the rig decay
  to rest. 8× was noisier (more momentum); 4× was the sweet spot. This is the change in
  PR #301.

## Investigation update — 2026-06-18 (hip "disconnect" deep-dive)

Focused on the **thigh joint visually disconnecting from the hip**. Findings, all
verified against `debug_ragdoll` / `debug_hitbox` over the debug runtime:

- **Rotation extraction is NOT the bug (ruled out, hard).** Dumped the 3×3 of every
  joint world-matrix the ragdoll consumes (`add_ragdoll`): **all perfectly
  orthonormal** — column norms 1.000, off-diagonal dot-products 0.000, det 1.000,
  including the thighs. So `get_rotation_from_matrix` (raw 3×3 → quat, no
  orthonormalization) corrupts nothing, and red==green in `debug_hitbox`. The old
  `debug_hitbox` doc comment ("red diverges at the thighs due to scale/shear") was a
  wrong guess and has been corrected in code.

- **The joints are positioned ~29 cm from where the limbs articulate (quantified).**
  Added a joint overlay to `debug_hitbox` (see Tooling). For the hips:
  `add_ragdoll` anchors **both** hip joints at the **same** point — the bone-8
  (pelvis) origin — because `frame1`'s translation is `(0,0,0)`. That origin sits
  ~11 cm above, ~13 cm behind, and ~18 cm lateral of each real hip socket — measured
  anchor→socket distance **0.287 m / 0.289 m** for L/R. So each thigh pivots about a
  point up at the sacrum, and its top swings off the pelvis as it rotates → the
  pose-dependent disconnect.

- **Anchoring at the correct hip socket (child origin) is geometrically right but
  destabilizes (ruled out as a standalone fix).** A/B'd via a `RAGDOLL_ANCHOR=child`
  env toggle: offsetting the anchor from the heavy pelvis hub torques it and *adds*
  energy — left hip went 12→16 cm and the rig got jumpier. The parent-origin anchor
  is the *stable* choice precisely because it doesn't torque the hub.

- **Stiffening the soft translation lock closes the gap but diverges on contact
  (confirms the doc's warning, even with the heavy core).** Swept
  `natural_frequency` via `RAGDOLL_FREQ`. At 60 (current) the rig is **stable** —
  `min_y` steady, `max_lin` decays 0.20→0.065 — but the left hip holds **9 cm** at
  rest (right ~2.4 cm; the asymmetry is systematic, the loaded hip hitting its 60°
  cone limit and the soft translation letting it squirt out). At 500/1e6 the hip gap
  closes to ~2 cm transiently, then **`max_ang` climbs to 27+ once it hits the
  floor**. So you cannot get correct-pivot + closed-gap + stability out of impulse
  joints on this hub-and-spoke skeleton.

**Conclusion:** the visible disconnect is a *real positioning issue* (anchor far
from the articulation point) compounded by the *soft translation lock* (needed for
hub stability) — and the two cannot be reconciled within impulse joints. The
principled fix is **multibody joints** (next steps, now the chosen direction).

## Investigation update — 2026-06-18 (multibody conversion, behind a flag)

Implemented multibody joints and made them **opt-in via experimental flags**, with
the impulse rig kept as the stable default:

- `--experimental ragdoll` — spawn a ragdoll on creature death (`SlayEntity`) instead
  of just removing the entity. Without it, death is unchanged. (Before this there was
  *no* ragdoll-on-death path at all — ragdolls only existed in the `debug_ragdoll`
  scene.)
- `--experimental ragdoll_multibody` — the ragdoll uses reduced-coordinate multibody
  joints (anchored at the child/hip articulation point, uniform mass, no softness);
  otherwise it uses the impulse rig (parent-origin anchor, heavy core, soft
  translation). Routing verified: default → 19 impulse joints; multibody flag → 0
  impulse joints, bodies stay connected (~1.6 m extent).

**What the multibody fixes:** translation is structurally not a DOF, so limbs
**cannot separate** — the hip gap is gone, and we can finally anchor at the true
articulation point (child origin) without the hub-torque instability impulse joints
had. Forward-kinematics reproduces the captured death pose at spawn (frames are
death-pose-aligned), so there's **no snap** — the failure mode of the earlier naive
drop-in (which exploded because bodies spawned independently of the reduced-coord
rest config).

**Multibody stability findings (verified, env-swept then baked in):**
- **Drop the heavy core — biggest win.** The 4× `CORE_BODY_MASS` is an *impulse* hub
  crutch; a proper articulated solver is *destabilized* by the mass ratio. Uniform
  mass took `max_ang` from ~44 → ~3. The multibody branch now uses `TARGET_BODY_MASS`
  for the core.
- **Joint-friction motors inject energy → explode** (`motor_velocity(0, factor)`,
  Acceleration-based, drove `max_ang` to 9e4 and through the floor). Do **not** use
  motors to damp; ruled out.
- **Soft contacts** (`IntegrationParameters::contact_softness` natural_frequency
  30→10) remove the spawn/floor-**impact** spike — but this is a *global* physics knob,
  so it was reverted (not safe to change world-wide for an experimental rig).
- **More solver iterations** (4→16) help marginally then *hurt* with uniform mass.
- **Self-collision off** doesn't help the churn.
- **Body-creation dedup bug found + fixed:** the Dark skeleton lists joint 18 twice,
  and body creation (unlike joint creation) wasn't deduped → a second, never-jointed
  **orphan body** for joint 18 fell away as an invisible stray (and inflated the body
  bbox to 6 m). Now one body per joint id.

**The remaining blocker (multibody):** even at the best config the corpse does **not
fully settle on the floor** — the core comes to rest but **extremities jitter in a
contact limit-cycle** (one-body `max_ang`~8, `lin`~5; bulk drift only ~0.08 m/s, so
it's localized limb buzz, not bulk sliding). Body linear/angular damping doesn't bleed
the multibody's *constrained* DOF, and the obvious fix (joint-friction motors) injects
energy. This is why multibody stays **experimental / opt-in**, not the default.

**Net trade-off (concrete):**
- *Impulse (default):* settles cleanly (`lin`→0.065) but the hip **gaps ~9 cm**.
- *Multibody (flag):* **no gap, can't separate**, but **extremities won't settle**
  (contact jitter) and the in-game death path is gated behind `--experimental ragdoll`.

## Re-evaluation — 2026-07-16 (multibody blocker no longer reproduces)

Re-ran both rigs in `debug_ragdoll` headlessly (45–90 s of fixed-60 Hz sim,
polling `/v1/ragdoll/metrics` once per sim-second, `/v1/physics/joints`,
screenshots). Physics changes landed since the June frontier — most relevantly
#333 (collider friction/restitution driven from `P$PhysAttr`) — and the picture
has changed materially:

- **Impulse rig (default): unchanged, still the documented failure mode.**
  `max_lin` decays to ~0.05, but the loaded hip holds a **9.6 cm** gap (other
  hip ~2.4 cm — same systematic asymmetry), a ~1.6–2.2 rad/s angular twitch
  persists indefinitely, and the settled pose is visually implausible (legs
  folded straight up in the air).
- **Multibody rig (`ragdoll_multibody`): the "extremities won't settle" blocker
  is gone.** Where June measured a one-body limit-cycle of `max_ang`~8 /
  `lin`~5, a 90 s run now tails at `max_lin` ~0.016–0.05 and `max_ang`
  mean **0.43** (t>60), with occasional 1–3 rad/s blips. The settled pose is a
  flat, believable prone corpse — clearly better than the impulse rig's.
  Attribution is circumstantial (no bisect run) but #333's contact
  friction/restitution change is exactly the mechanism the June log implicated
  (floor-contact energy).
- **Residual (small):** one extremity body keeps a ~0.5 rad/s buzz, and
  **0/20 bodies ever sleep** over 90 s, so the rig never goes fully still.
  Island sleeping (the step-2 fallback below) is now the graduation fix for the
  *multibody* rig rather than a consolation for the impulse rig: sleep kills the
  buzz, auto-wake on contact/force keeps the corpse pokeable.

**Verdict: the multibody rig is now better on every axis** (no gap possible,
better pose, lower residual energy). Plan: enable sleeping for ragdoll bodies,
verify (bodies sleep, `max_ang` → 0, wake-on-impulse), then promote multibody to
the default rig. The step-0 ideas below (joint damping, per-collider contact
softness, velocity clamps) were **not needed** and are kept only for reference.

## Recommended next steps (ranked) — as of 2026-06-18, see re-evaluation above

### 0. Crack the multibody floor-contact jitter (the one blocker left)
Make the multibody rig settle so it can graduate from experimental to default. Ideas
not yet tried / not yet working:
- **Stable joint damping that doesn't inject energy** — the multibody's per-DOF
  `damping` vector (set by `MultibodyJoint::default_damping` on append) rather than a
  velocity *motor*; or a very low-gain Force-based motor. Motors at the gains tried
  exploded; needs care.
- **Contact handling for the ragdoll only** — per-collider contact softness/friction
  (not the global `IntegrationParameters`), or temporarily softer contacts while the
  rig is "fresh".
- **Velocity clamp / extra substeps** for the first ~1 s after spawn to absorb the
  transient, then release.
- Watch with `/v1/ragdoll/metrics` (`max_ang`/`max_lin` should decay, not limit-cycle)
  and the `debug_hitbox` joint overlay (anchor stays at the articulation point).

### 1. ⭐ Multibody (reduced-coordinate) joints — IMPLEMENTED (behind `ragdoll_multibody`)
Done in this pass (see `add_ragdoll`'s `use_multibody` branch): anchored at the
child/hip articulation point, dropped `softness`, uniform core mass, death-pose-aligned
frames so forward-kinematics reproduces the spawn pose (no snap), parent-before-child
insertion (insert is order-robust given per-child dedup), body-creation dedup, joints
auto-removed with bodies. `PhysicsWorld::create_multibody_joint` →
`multibody_joint_set.insert`. **Remaining:** the floor-contact jitter (step 0) — until
that's solved this stays experimental, and `/v1/physics/joints` + the `--debug-physics`
anchor viz still iterate only the **impulse** set (extend to the multibody set for
multibody diagnostics).

### 2. Fallback if multibody proves too unstable — accept gap + auto-sleep
Keep the stable soft impulse joints, stop fighting the residual ~9 cm hip gap, and add
Rapier **island sleeping** so the corpse goes still but stays pokeable: a body whose
linear/angular velocity stays below threshold for `time_to_sleep` **sleeps** (drops out
of simulation) but **auto-wakes on contact or applied force** — so you can still walk
into / shoot / push the corpse and it springs back to life. We don't disable sleeping
anywhere (only read `is_sleeping` for debug); check `GET /v1/physics/bodies/:id` →
`is_sleeping`, and help it cross the threshold sooner via a bit more damping and/or
lower `IntegrationParameters` sleep thresholds / `time_to_sleep` (confirm field names in
rapier 0.31). Interaction-friendly alternative to a hard freeze/kinematic-pin (which
would make the corpse un-pokeable). The gap remains visible — pragmatic fallback, not
the fix.

### Avoid
- **Hard freeze / kinematic pin** of the settled corpse — kills interactivity (the user
  explicitly wants to be able to poke it). Use auto-sleep (fallback step 2) instead.

## Tooling & methodology (important gotchas)

- `cargo dbgr --mission debug_ragdoll --debug-physics --port N` (no extra `--`). The
  joint-anchor viz draws cyan/magenta anchor crosses + a yellow gap line per joint.
- `cargo dbgr --mission debug_hitbox --port N` — **physics-free** joint overlay
  (added 2026-06-18). Per parent→child joint, drawn from the live posed skeleton:
  **blue** = bone segment, **yellow** = the impulse-joint anchor `add_ragdoll` uses
  (parent origin), **magenta** = closest/contact points between the two fitted shapes
  (`parry::query::contact`). Cycle poses with the `DebugHitboxCyclePose` input action.
  Use it to see joint placement *vs* where the limbs meet without any solver noise —
  this is how the ~29 cm hip anchor↔socket offset was measured. (Caveat: the debug
  scene camera is fixed and far; the markers read small from the side. Improving the
  framing — move the creature closer / bigger per-joint markers — is a nice follow-up.)
- `GET /v1/physics/joints` — anchor separation + linear/angular impulse, bone-labeled.
- `GET /v1/ragdoll/metrics` — `max_linear_speed` / `max_angular_speed` (settle signal),
  `min_y` (floor contact), `max_drift`, `max_nonadjacent_overlap`.
- **Stepping gotchas (these wasted a lot of time):**
  - Step in **small batches** (`{"frames":60}`) and **poll** `min_y` / metrics until the
    rig has actually landed and quiesced. A single large `{"frames":1500}` call
    *under-steps* — the debug window renders every frame, so the HTTP call times out
    before it finishes, and you measure a still-falling rig.
  - The `debug_ragdoll` spawn happens at a frame offset; **poll until the ragdoll exists**
    (`/v1/ragdoll/metrics` non-empty) before starting your settle loop.
  - Stepping is a deterministic fixed 60 Hz, so `{"frames":N}` == N/60 s of sim time.
- Humanoid bone ids: **8** = pelvis hub, **18** = abdomen, **6/7** = L/R thigh,
  **4/5** = L/R knee, **9** = head, **10/11** = shoulders.

## Key code locations

- `shock2vr/src/creature/rag_doll.rs` → `RagDollManager::add_ragdoll`:
  - body creation + per-shape density (`TARGET_BODY_MASS`, `CORE_BODY_MASS`, damping),
  - joint creation (`GenericJointBuilder`: `local_frame1/2`, cone `limits`, `softness`,
    `contacts_enabled(false)`), `create_impulse_joint`.
- `shock2vr/src/physics/mod.rs`: `create_impulse_joint`, `debug_render` (joint-anchor
  overlay), `debug_list_joints`, `multibody_joint_set` (wired into `step`, ready to use).
- `shock2vr/src/scenes/debug_ragdoll.rs`: the test scene (spawns a pipe hybrid, kills it,
  spawns the ragdoll).
- `shock2vr/src/scenes/debug_hitbox.rs`: physics-free pose/joint inspector —
  `draw_joint_debug` (bone/anchor/contact overlay) + `parry_shape` (build a parry shape +
  world isometry from a fitted `HitBoxShape`, matching `add_ragdoll`'s collider placement).
- `shock2vr/src/util.rs` → `get_rotation_from_matrix`: raw 3×3 → quat. Verified safe here
  (joint matrices are orthonormal), but note it does NOT orthonormalize.
