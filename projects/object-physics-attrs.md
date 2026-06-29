# Object Physics Attributes → Rapier

Threading Dark's authored per-object physics attributes (`P$PhysAttr`) into the
Rapier simulation. Background and full gap analysis: `research/physics-system-differences.md`
(Delta #2 is the core of this work).

## Problem

shock2quest parses `PropPhysAttr` (`mass`, `density`, `elasticity`, `friction`,
`cog`, `rotation_axes`, `rest_axes`, …) but feeds Rapier almost none of it. Every
dynamic collider was hardcoded to `restitution = 0.7`, `friction = 0.5` (Rapier
default), `density = 0.1` — so bounce, surface friction, and mass were uniform
and wrong, regardless of what each object authored.

Only `gravity_scale` and `PhysInitialVelocity` were previously plumbed.

## Scope of the *dynamic-body* change (important)

Per Delta #1, only a small set of objects currently become **dynamic** Rapier
bodies (creatures, frob-`MOVE` grab items, and `!immobile && SPHERE` objects).
Everything else is a **kinematic** cuboid (`add_kinematic`), which these
attributes do not touch. So this work only affects that dynamic subset; letting
more objects (mobile OBB props) be dynamic is a separate follow-up (Delta #1).

## Slice 1 — elasticity + friction (this PR)

`DynamicPhysicsOptions` now carries `restitution` and `friction`, populated from
`PropPhysAttr` in `entity_creator.rs` and applied in `physics::add_dynamic`.

- **elasticity → restitution.** Calibrated, not 1:1. Dark's shipped `elasticity`
  is essentially always `1.0`; a Rapier restitution of `1.0` is a perfect,
  never-settling bounce. We scale by `ELASTICITY_TO_RESTITUTION = 0.7` so the
  default-authored object reproduces the previously-hardcoded `0.7`. Net effect
  on shipped data: **none** (it's a no-op until objects with non-1.0 elasticity
  appear), but per-object variation now drives the sim correctly.
- **friction → friction.** Authored friction (`0.0`–`0.4` in the data) replaces
  the flat Rapier default of `0.5`. This is the one field that changes real-game
  behavior in this slice: objects are slightly slidier (most author `0.0`). Low
  magnitude and covered by the unit test, but flagged in `todo.md` to eyeball
  alongside the density playtest.
- Non-finite authored `elasticity`/`friction` fall back to the defaults (Dark
  data can carry non-finite physics values — cf. `sanitize_collider_size`).
- Entities **without** `PropPhysAttr` are unchanged — `DynamicPhysicsOptions::default()`
  reproduces the old `{restitution 0.7, friction 0.5, density 0.1}`.

### Verification

- `shock2vr/src/physics/mod.rs` unit tests (deterministic, headless, fixed 1/60
  step): `higher_restitution_bounces_higher`, `higher_friction_slides_less`.
  Both were confirmed **red** with the wiring reverted (both bodies reported
  identical apex / slide distance) and green with it in place.
- `tools/shock2-sdk/test/missions.e2e.test.ts` — all 23 missions still load.

## Deferred

### Needs human (in-VR / flat) verification — batch these together

- **density → mass.** Mapping `density` (shipped default `1.0`) onto the
  collider replaces the hardcoded `0.1`, i.e. roughly **10× the mass** for
  dynamic objects. That changes how grabbed/thrown items feel and how hard the
  player can push them — there is no automated test for "feel", so it needs a
  human in the loop. Also requires clamping the `density = 1000000.0`
  "immovable" sentinel seen in the data so it can't destabilize the solver if
  such an object ever becomes dynamic. Tracked in `todo.md` under the
  human-verification batch.

### Other follow-ups (no human gate, but out of scope here)

- **center-of-mass (`cog`)** via `set_center_of_mass`.
- **`rotation_axes` / `rest_axes`** → `set_enabled_rotations` (today rotations
  are only ever fully locked, on creatures).
- **Delta #1 — mobility-based dynamic bodies.** Decide dynamic-vs-kinematic from
  a mobility flag (`PropImmobile` / Dark's translatable bit) instead of shape,
  so mobile OBB props can tumble. This is the change that makes the attribute
  work above *visible* on the majority of props.
- **friction calibration.** Direct mapping is a reasonable first port;
  cross-checking Dark's friction model may warrant a scale factor.
