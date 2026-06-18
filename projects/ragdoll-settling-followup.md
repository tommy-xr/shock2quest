# Ragdoll Settling & Hip Connection — Follow-up Plan

Hand-off doc for continuing the ragdoll work. Goal: a believable corpse that
**settles** and whose limbs (notably the **hips**) stay connected, while remaining
**interactive** (you can poke/push the body — that's part of the fun).

See also `projects/ragdoll.md` (the main ragdoll doc + earlier 2026-06-15/06-18
investigation logs). This doc is the current frontier as of 2026-06-18.

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

## Recommended next steps (ranked)

### 1. Use Rapier auto-sleep for "stops twitching, still pokeable" (do this first)
The user wants the corpse to stop twitching **without** losing interactivity. Rapier's
**island sleeping** is exactly that: a body whose linear/angular velocity stays below a
threshold for `time_to_sleep` seconds **sleeps** (drops out of simulation), but
**auto-wakes on contact or applied force** — so you can still walk into / shoot / push
the corpse and it springs back to life. We do **not** disable sleeping anywhere (we only
read `is_sleeping` for debug).

- Check whether the ragdoll bodies actually sleep once quiescent:
  `GET /v1/physics/bodies/:id` → `is_sleeping`. If the residual ~40 s twitch keeps them
  awake, the fix is to help them cross the sleep threshold sooner: a bit more damping,
  and/or lowering `IntegrationParameters::{normalized_linear_/angular_}? sleep thresholds`
  / `time_to_sleep` for the ragdoll. (Confirm the exact field names in rapier 0.31.)
- This is the interaction-friendly alternative to a hard freeze/kinematic-pin — **prefer
  it.** A hard freeze would make the corpse un-pokeable; auto-sleep keeps the fun.

### 2. Close the hip gap — per-joint stiffer hips
Make just the **hip** (and probably shoulder) joints less compliant than the rest — a
higher `SpringCoefficients::natural_frequency` (or a per-bone stiffness profile keyed off
the joint id, like the existing per-bone `JointLimit` cone profile). The heavy core (step
done in #301) now absorbs more joint stiffness without re-exploding, so stiffer hips are
likely tolerable. Verify with the **joint-anchor viz** + `/v1/physics/joints` that the
hip separation closes **without** re-introducing energy (watch `max_linear_speed`).

### 3. Multibody (reduced-coordinate) joints, done properly (bigger, principled fix)
Reduced-coordinate joints **cannot separate** (no hip gap) and inject **no** residual
energy — the right tool for an articulated tree. To make them stable here:
- **Forward-kinematic the body spawn poses from the root**: build the bodies root-first,
  positioning each child from `parent_pose * joint_local_transform` (not from the
  independently-captured death-pose transforms), so the multibody's rest config matches
  the spawn state and nothing snaps.
- Insert joints **parent-before-child** (topological / depth order — the Dark bone list is
  not sorted).
- **Drop the `softness`** (multibody is rigid; a spring oscillates).
- Mind the tiny-hub inertia (combine with the heavy-core change, or give the hub real size).
- API: `PhysicsWorld::create_multibody_joint(parent, child, GenericJoint)` →
  `multibody_joint_set.insert(parent, child, joint, true)` (was prototyped then reverted;
  re-add it). Note `/v1/physics/joints` + the joint-anchor viz currently iterate the
  **impulse** joint set — extend them to the multibody set if you switch.

### Avoid
- **Hard freeze / kinematic pin** of the settled corpse — kills interactivity (the user
  explicitly wants to be able to poke it). Use auto-sleep (step 1) instead.

## Tooling & methodology (important gotchas)

- `cargo dbgr --mission debug_ragdoll --debug-physics --port N` (no extra `--`). The
  joint-anchor viz draws cyan/magenta anchor crosses + a yellow gap line per joint.
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
