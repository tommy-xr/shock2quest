# Weapon correctness and VR feel

Status: agreed direction, 2026-09-08. Implementation and runtime verification are
tracked below; a source inspection is not a completed projectile audit.

## First implementation slice

Implemented on `feat/weapon-feedback-workstream`: authored `BaseWeaponDesc`
weapon-skill parsing, a shared trigger-time skill check, and a three-second
refusal bound to the attempted weapon. An existing burst stops if the weapon
becomes ineligible. Repeated pulls while the same notice is active do not extend
its lifetime or stack messages. Flat and VR read the same active notice and
shared message layout; meeting the requirement clears it immediately. VR
anchors the text above the firing hand, including when the weapon supplies its
own hand mesh, and for a trigger-driven weapon with no ammo readout. The amp
retains its separate psi eligibility path. VR melee contact eligibility remains
part of the broader audit; this slice gates `WeaponScript` trigger pulls.

The first text is `Requires Standard Weapons 6 - You have 4` (ASCII separator
for the shipped bitmap font). This slice adds no denial sound, stat/research
gate, grip change, recoil, or projectile behavior change beyond skill gating.
Those remain separate increments. Headset readability is not yet verified.

Earth's `PlayerFactory` marker authors Standard 1 / Energy 2. Dark's
`sim/plyrloop.cpp::create_player_obj` clones that marker's properties onto the
player, and `shkplayr.cpp::GetWeaponSkills` reads them. Mission construction now
copies that weapon-skill allowance onto the nonserialized runtime player;
`player_skill_level` takes the maximum of it and the persistent trained skill.
This maximum is the port's adaptation to its split runtime/career state, not
an original-engine formula. The allowance is reconstructed on same-mission
load and rederived on transition, so tutorial Energy 2 never becomes a permanent
career reward. Character-sheet/trainer displays still show persistent training.

The SDK scenario `weapon-skill-feedback.e2e.test.ts` exercises a real mission
with an under-skilled rifle, checks unchanged ammo and message expiry while
holding the trigger, and raises the skill to the authored threshold to verify
firing. It covers flat and both VR hands. The unit rejection assertion was run
red before connecting the gate and green afterward.

Verification: 14 focused runtime cases pass, including Earth firing/reload,
save/load and removal of its allowance on transition; flat and left/right VR
refusals; and the affected projectile-effect scenarios. Rust library tests pass
(166 dark, 1600 shock2vr, 2 ignored), as do warning-denied runtime/package checks
and 63 fast SDK tests. All 23 mission-load smoke cases passed during this slice.
The full SDK run was stopped after failures and is not a complete green run;
the unrelated Watts reader assertion is already tracked by #1415. Complete the
full suite and headset review before landing.

[Flat and both-hand captures](https://gist.githubusercontent.com/tommy-xr/ed550c7bd1ae1809da251e03f6a5793c/raw/presentations.png)
and [notice-expiry GIF](https://gist.githubusercontent.com/tommy-xr/ed550c7bd1ae1809da251e03f6a5793c/raw/right.gif)
show the first presentation; a headless capture establishes placement, not
headset comfort.

## Decisions

- Establish original-game combat parity first, then layer deliberate VR
  augmentations on top. Keep their tuning and rationale explicit.
- **Strength improves VR recoil control.** This is an intentional augmentation,
  not a claim about original System Shock 2. Preserve the original Agility,
  Still Hand and aiming-implant behavior when establishing parity; tune how
  Strength combines with it after that baseline is measurable.
- Weapon skill governs authored inaccuracy. Preserve Sharpshooter's original
  ranged-damage benefit; an accuracy bonus is not part of the accepted baseline.
- Begin failed-fire feedback with a brief message above the firing glove, e.g.
  `Requires Standard Weapons 6 · You have 4`. Do not force a vertical grip.
  Derive the text and firing decision from the same requirement evaluation.
- Adapt the existing physical-held work rather than restart by default. Retain
  the recent calibrated grips, support grips, glove feedback and wrist readouts.
- Recoil must affect the weapon pose used for aim and effects. Never recoil the
  tracked head camera. A successful shot applies an impulse; failed attempts do
  not consume ammo, spawn firing effects, or recoil.

## Source baseline and dependencies

Planning began on local `d693a5a4`; the implementation baseline is current main
`c0ae0e54`. Main now includes glove monitors (#1436) and parsed authored gun kick
and skill inaccuracy (#1406). Recheck earlier findings against this baseline.

- [#1142](https://github.com/tommy-xr/shock2quest/pull/1142): open physical-held
  ranged spike, default-off flag. Kinematic drive, world translation sweep,
  inert gun collision groups. Rotation is not swept. Review actual code and
  reproduce on the current interaction baseline before adopting it.
- [#1153](https://github.com/tommy-xr/shock2quest/pull/1153): open, based on
  #1142. Uses blocking sweep contacts for held-gun impact audio without enabling
  projectile/solver collisions against the held gun.
- [#1080](https://github.com/tommy-xr/shock2quest/pull/1080): open geometry-based
  muzzle fallback fix; inspect before adding another implementation.
- [#1325](https://github.com/tommy-xr/shock2quest/pull/1325): open Sharpshooter
  damage work; coordinate rather than implement the trait twice.
- [#140](https://github.com/tommy-xr/shock2quest/pull/140) is a query-name filter
  fix, not a weapon implementation. The intended reference remains unresolved.
- [Citadel recoil](https://github.com/tommy-xr/citadel-xr/blob/main/games/citadel/Citadel_Entities/Recoil.ml)
  has pitch, yaw and kickback springs and computes the muzzle with recoil.
  Its MachineGun uses reduced kick for two-hand support. Borrow the approach,
  not its model-specific offsets or tuning constants.
- Original-engine reference: `src/shock/shkplgun.cpp`, `CalcKickAngle`,
  `CalcRandAngle`, and the launch-time Sharpshooter stim multiplier. The inspected
  source scales angular kick by Agility and inaccuracy by weapon skill.

## Ordered increments

### 1. Requirement feedback and audit baseline

Source inspection on `c0ae0e54` found no parser for `P$BaseWeapo` (the
`BaseWeaponDesc` property's four weapon-skill integers) and no skill gate in
`WeaponScript::handle_message`. The original `cShockPlayer::CheckRequirements`
in `shkplayr.cpp` checks that property as well as required stats. Therefore the
first slice includes an eligibility prerequisite; the reported silent trigger
has not yet been established as an insufficient-skill rejection. Existing
`script_util::player_skill_level` supplies the character-sheet lookup and
`hud::message_line` supplies reusable status-text layout and expiry patterns.

- [x] Trace original requirement properties and current eligibility checks;
  reproduce a failed trigger with insufficient skill. Do not assume every
  silent trigger is an existing skill gate.
- [x] Return a structured rejection with required/current values and weapon
  identity from shared gameplay code. VR presents it above the firing glove;
  flat presents the same content through shared canvas layout.
- [x] Show feedback on an actual failed attempt, rate-limit repeated pulls,
  and avoid restarting it every frame while the trigger stays held.
- [ ] Verify left/right hands, dual wield, support hand, weapon changes,
  untracked hands, use-mode suppression, and successful firing at the threshold.
- [ ] Build the weapon audit matrix below from authored data and original code.

### 2. Correct firing and physical handling

- [ ] Fix shooter filtering for fast rays and physical projectiles. Near-chest
  shots must not hit the firing player's capsule or held items accidentally.
  Preserve enemy hits, point-blank world obstruction and explosive self-damage.
- [ ] Audit muzzle/vhot lookup and missing-vhot fallbacks for all held models.
  Ensure no fallback or clearance moves a shot through a wall.
- [ ] Reproduce flash/casing regressions with matched world/held models. Inspect
  GunFlash flags, attachment point identity, orientation, visibility and lifetime.
  Muzzle flashes may follow the gun; ejected shells must detach correctly.
- [ ] Resolve dry-fire sound end to end: event name, schema tags, selected clip
  and playback. Distinguish empty, broken, reloading and insufficient-skill states.
- [ ] Adapt #1142/#1153 to current main. Verify fitted gun-only colliders,
  wall stops, material impact sounds, rotation into walls and lifecycle cleanup.

### 3. Recoil, VR augmentation and audit completion

- [ ] Implement authored accuracy/recoil behavior in shared gameplay code,
  using #1406's parsed data and preserving original trait/psi/implant hooks.
- [ ] Add bounded, timestep-stable pitch/yaw/kickback springs to the held drive:
  tracked grip -> recoil offset -> collision-constrained weapon pose -> muzzle.
  Rendering, projectile direction and attached effects read that resolved pose.
- [ ] Define shot ordering explicitly: fire from the current resolved muzzle,
  then apply the new impulse for recovery and subsequent shots. Document any
  intentional departure from original pre-shot kick behavior.
- [ ] Add and tune **Strength-based VR recoil control** after parity. Keep it
  separate from weapon inaccuracy and projectile damage. Start with a monotonic,
  bounded reduction in angular/backward kick; choose constants from headset
  trials rather than commit guessed values as design requirements.
- [ ] Evaluate additional two-hand stabilization using the existing support
  grip. Losing support must not snap or reset accumulated recoil.
- [ ] Tune pistol, assault rifle and a heavy weapon first. Check sustained fire,
  return to rest, hand motion, cover, drop/regrab and different frame rates.
- [ ] Complete remaining projectile/mode fixes by weapon family and rerun the
  matrix. Include flat/VR verification and on-device feel review.

## Weapon/projectile audit matrix

One row per weapon x setting x ammo/projectile variant, including melee and psi
entries explicitly classified as separate firing paths. Enumerate from data;
do not assume the default setting exercises every projectile.

Record:

| Field | Evidence required |
| --- | --- |
| Identity | Stable template, weapon name, model, setting and ammo |
| Eligibility | Required stats/skills/research; actual rejection and feedback |
| Firing | Trigger semantics, cooldown/burst, projectile count and ammo/energy cost |
| Geometry | Muzzle source, barrel axis, spread, spawn position and owner filtering |
| Projectile | Speed/gravity, collision, damage/stim, resistances and special behavior |
| Presentation | Flash, casing/spawns, shot/empty/impact sounds and effect lifetime |
| Lifecycle | Reload, mode/ammo change, breakage, drop/store, current-build save/load |
| Verification | Authored expectation, original-code reference, flat result, VR result, evidence |

Cover ballistic, energy, heavy and exotic weapons, all grenade types and special
effects (EMP, stasis, splash, biological effects), and any secondary spawns.
Use stable template/name discovery, deterministic SDK scenarios and explicit
observed outcomes. Existing fast-projectile fixed damage is an audit target,
not evidence that projectile damage parity is complete.

## Definition of done

Each implementation increment is separately reviewable with focused regression
tests. Gameplay scenarios demonstrate failure before the fix and success after
it. Visual changes include inspected matched before/after PNG and looping GIF
evidence; shared UI renders are checked in flat and VR. Run the repository's
required build and SDK checks before landing. Headless geometry evidence does
not establish headset comfort or recoil feel: record a Quest validation pass
and human feel assessment separately from automated checks.
