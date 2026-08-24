# Footstep Sounds

Assessment of what it takes to give the player and creatures footstep audio,
plus the design for the player half (the creature half is implemented).

## 1. The data supports this fully

The shipped sound schema authors a complete footstep vocabulary. Resolve it
with `cargo dq sound`:

```
$ cargo dq sound +event:footstep +creaturetype:player +material:metal
Result: ftmet3 (volume -1000 millibels)
$ cargo dq sound +event:footstep +creaturetype:oncegrunt +material:fleshtarget +material2:metal
Result: ft_ogm1 (volume -1200 millibels)
$ cargo dq sound +event:footstep +creaturetype:monkey
Result: ft_monk1 (volume -1200 millibels)
```

The tree is keyed `event=footstep` first, then `creaturetype`, then per-type
refinements. Two distinct shapes:

**The player** branches on the surface underfoot and on how the foot arrived:

| key | value | samples |
| --- | --- | --- |
| `material` | `plasticrete` | `ftpoly1..4` |
| `material` | `metal` / `metalbig` / `metaldebris` / `metaltarget` | `ftmet1..4` |
| `material` | `glass` | `fttil1..4` |
| `material` | `glassbits` | `ftsnow1..4` |
| `material` | `fabric` | `ftcar1..4` |
| `material` | `flesh` / `fleshtarget` | `ftfle1..4` |
| `material` | `earth` | `ftear1..4` |
| `medialevel` | `foot` | `ftwat1..4` (wading) |
| `medialevel` | `body` | `swimtop1..3` |
| `medialevel` | `head` | `stroke1..3` (submerged swim stroke) |

plus a `landing=true` sub-key under every material giving the heavier
one-shot landing thud (`ftmetj`, `ftpolyj`, `fttilj`, `ftearj`, `ftsnowj`,
`ftflej`). A player query with no `material` and no `medialevel` resolves to
**nothing** — the player's branch requires one of them.

**Creatures** mostly resolve on `creaturetype` alone to a four-sample set:

| `creaturetype` | samples |
| --- | --- |
| `oncegrunt` (hybrid) | `ft_og1..4` (soft floors) / `ft_ogm1..4` (metal) |
| `monkey` | `ft_monk1..4` |
| `droid` | `ft_dro1..4` |
| `protobot` | `ft_pro1..4` |
| `rumbler` | `ft_rumb1..4` |
| `midwife` | `ft_mw1..4` |
| `spider` | `ft_spid1..4` |
| `assassin` (ninja) | `ft_ninj1..4` |

Hybrids are the only type that branches further: `material` = the creature's
*own* material (its feet — `fleshtarget`), `material2` = the surface underfoot.
No footsteps are authored for swarms, apparitions, SHODAN, servbots or the
overlord; those resolve to nothing and are silent, which is correct.

There is also an `event=climbstep` tag in the vocabulary, but nothing in the
tree resolves under it for the player — ladder-step audio would need new
sourcing and is out of scope.

## 2. Creatures — implemented

Cost: about an hour. Two small pieces:

- `script_util::play_footstep_sound(world, entity_id)` builds the schema query
  (`event=footstep` + class tags + `material`/`material2`) and emits
  `Effect::PlayEnvironmentalSound` at the creature's transform, reusing the
  existing `play_environmental_sound` helper.
- `AnimatedMonsterAI::handle_message` handles `AnimationFlagTriggered` with
  `LEFT_FOOT_STEP | RIGHT_FOOT_STEP`, beside the existing `FIRE` and
  `MELEE_CONTACT_START` arms.

Everything else already existed: the motion format's per-frame flags are parsed
into `AnimationClip::motion_flags`, `AnimationPlayer::update` returns the flags
crossed this frame, and `mission_core` already dispatches them as
`AnimationFlagTriggered`.

**Why no rate limiting is needed.** The shipped locomotion clips are
*per-half-step*: `ogpwlklt` and `ogpwlkrt` are separate clips, each carrying
exactly one foot-plant flag. A walk cycle is therefore exactly two footsteps,
paced by the animation, at whatever speed the creature is actually moving. No
accumulator, no cooldown, no speed threshold.

**Why it is not gated on "is this a locomotion clip".** Scanning the 589 clips
referenced by the shipped motion schemas, 219 carry foot-plant flags — and they
include idle clips (`ogpidle2`, `ogsidle2`), turns (`ogpturn`), staggers
(`ogprecwd`) and death collapses (`ogpdie2`, `humdie2`). Those are authored, not
accidental: a hybrid shifting its weight while standing *does* plant a foot. So
the handler fires on the flag wherever it appears, including from a dying
creature, and deliberately does not consult `is_dead` the way the `FIRE` and
melee arms must.

**Terrain material.** The port has no per-texture material lookup for world
geometry (`script_util::DEFAULT_IMPACT_MATERIAL`), so `material2` is the default
bulkhead metal. For hybrids that resolves `ft_ogm*` rather than `ft_og*`, which
is right for most of the ship and wrong on Hydroponics carpet/soil. See §4.

## 3. The player — plan, not implemented

The player has no creature animation, so there is no foot flag to consume. A
footstep has to be derived from locomotion. Rough size: **a day** for a good
version, half a day for a crude one. Nothing here is deep, but there are more
decisions than the creature half and the verification is fussier.

### What exists

- `PhysicsWorld::step_player_movement` (`shock2vr/src/physics/mod.rs`) runs the
  walk pass and the gravity pass; `PlayerHandle` already carries the transient
  per-frame state a footstep accumulator belongs beside — `is_grounded`,
  `is_crouched`, `jump_velocity`, `support` (moving terrain underfoot),
  `slope_displacement`. That struct's own comment notes this category of state
  is purely derived and deliberately not saved, which is exactly right for a
  step accumulator.
- One shared movement call site for flat and VR: `MissionCore::update` feeds
  `input_context` thumbsticks into `PhysicsWorld::update_with_facing_and_jump`,
  which returns the player's new position. There is no VR-specific movement
  branch to duplicate into.
- `script_util::play_environmental_sound` already turns a tag set into a
  positional `Effect::PlayEnvironmentalSound`.

### What must be built

1. **A public `is_grounded()` on `PlayerHandle`.** Currently private.
2. **A distance accumulator**, not a timer. Accumulate the *horizontal* component
   of the per-frame position delta the physics update already returns; when it
   crosses a stride length, emit a footstep and subtract the stride. Distance-
   based automatically gives slower steps when walking slowly and faster ones
   when running, with no speed tiers, and it cannot fire while standing still.
   Crouch shortens the stride (and should drop the volume, or the sneaking
   player is louder than the walking one).
3. **A player footstep emit path.** The player is an entity but has no
   `PropClassTag`, so `get_environmental_sound_query` returns `None` for it —
   the query has to be built directly as `event=footstep`, `creaturetype=player`,
   `material=<surface>`, rather than going through the class-tag helper.
4. **A landing event.** `landing=true` under each material is authored and is the
   single most noticeable half of this feature in VR — the thud when a fall ends.
   Emit it on the grounded false→true edge when the fall had meaningful vertical
   speed, and reset the stride accumulator so a landing is not immediately
   followed by a half-stride step.

### Where I would put it

A small `player_footsteps` module owning a struct with two fields (`distance:
f32`, `was_grounded: bool`), updated once per frame from `MissionCore::update`
right after the movement call, returning an `Option<Effect>`. That keeps it a
pure function of (position delta, grounded, crouched) and unit-testable with no
physics world: feed it a synthetic walk and assert the step count, feed it a
stationary player and assert silence, feed it a grounded edge and assert the
landing variant. It does not belong in `physics/` (which should not know about
audio) and it is not a script (the player has no script).

### Traps

- **Airborne.** Gate on `is_grounded`. Without it a jump plays a step at the top
  of the arc, because horizontal distance keeps accruing.
- **Elevators and moving platforms.** `PlayerHandle.support` exists precisely
  because the player can be carried. A player standing still on a moving lift
  accrues world-space distance and would step continuously. The accumulator must
  use displacement *relative to the support*, or be suppressed when `support` is
  present and the input stick is centred.
- **Teleport locomotion** (`teleport` experimental feature) moves the player via
  `Effect::SetPlayerPosition`, bypassing `move_player` entirely. A world-position
  delta accumulator would emit a burst of footsteps for a 20-foot hop. The
  accumulator must be driven by the movement call's own return value, and reset
  on any `SetPlayerPosition` (which also covers level load and quickload).
- **Room-scale walking does not move the capsule.** Headset translation is only
  used for camera placement — it never reaches `move_player`. So physically
  stepping in the playspace will produce no footstep. That is a *correct* first
  version (the sound would be wrong anyway — the deck is not moving under you),
  but worth knowing it is a deliberate gap rather than a bug report waiting to
  happen.
- **Frame-rate independence.** Accumulate distance, never `dt`-scaled counters
  tied to a fixed assumption of 60 Hz. Quest and desktop run different rates.
- **Surface material.** See §4 — every player footstep is currently `ftmet*`.
- **Water.** The `medialevel` branch (`foot`/`body`/`head` → wading, swimming,
  stroking) is authored and would be a genuinely nice touch, but the port has no
  water volume/immersion state to key it on. Out of scope; note it and move on.
- **Volume.** Schema volumes are authored per sample (`-1000` to `-1500`
  millibels for the player), so do not add a second attenuation on top — the
  first-person listener is at zero distance and a naively-positioned footstep at
  full gain is startling in VR.

## 4. The one shared gap: world surface material

Both halves are currently stuck on `material=metal` because the port has no path
from a world-geometry hit to a material. `RayCastResult` carries only
`hit_point`, `hit_normal` and `maybe_entity_id` (`None` for terrain) — no
texture or surface id. Fixing it is its own piece of work: plumb the hit
collider back to its texture, then map texture name → material tag. That would
upgrade footsteps (both halves), bullet impacts and melee impacts at once, so it
is worth doing as a separate change rather than folded into either half here.
Until then the ship sounds like metal, which is mostly true.
