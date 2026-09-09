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
  muzzle fallback fix. Reuse its bounds plumbing selectively: its claim that
  `GunFlash.vhot` is a file index is incorrect. Dark's `gunflash.cpp` calls
  `VHotGetLoc`, whose evaluated table is keyed by `v->id` in
  `libsrc/md/render.c::md_eval_vhot_subobj`. `VHotGetRaw` is a distinct,
  file-indexed helper. Preserve authored IDs and current scale-normalized shot
  frames when adapting this reference.
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

- [x] Fix shooter filtering for fast rays and physical projectiles. Near-chest
  shots must not hit the firing player's capsule or held items accidentally.
  Preserve enemy hits, point-blank world obstruction and explosive self-damage.
  Baseline #1050 already supplies the ray/physical filters. The next stacked
  slice, `fix/projectile-owner-save`, fixes their save/load lifetime: a live
  player-fired laser bolt was transparent to the player before saving and solid
  after loading. Ownership now round-trips separately from launch provenance,
  including entity remapping and held/world partitions. Enemy shots are not
  inferred to be player-owned. It leaves blast damage and muzzle placement
  unchanged; source-filter unit tests and the close-body VR psi scenario cover
  the existing firing paths alongside the new real save/load regression.
- [ ] Audit muzzle/vhot lookup and missing-vhot fallbacks for all held models.
  Ensure no fallback or clearance moves a shot through a wall.
  The `fix/weapon-vhot-identities` layer preserves raw IDs (including sparse IDs
  and values above 8) and file order, and resolves GunFlash attachments by ID.
  It explicitly retains the current lowest-ID projectile muzzle choice for
  existing weapon assets. Missing-muzzle geometry and obstruction are the next
  separate layer; no unverified barrel-tip fallback is introduced here.
  For sparse IDs, direct lookup intentionally avoids Dark's inconsistent
  count-bound guard before its ID-keyed evaluated table. Inspected normal held
  weapon models use dense IDs, so no visible change is claimed for those
  assets. The existing VR muzzle regression now accounts for calibrated item
  scale; its prior unscaled laser expectation failed on the parent as well.
- [ ] Reproduce flash/casing regressions with matched world/held models. Inspect
  GunFlash flags, attachment point identity, orientation, visibility and lifetime.
  Muzzle flashes may follow the gun; ejected shells must detach correctly.
- [ ] Resolve dry-fire sound end to end: event name, schema tags, selected clip
  and playback. Distinguish empty, broken, reloading and insufficient-skill states.
- [ ] Adapt #1142/#1153 to current main. Verify fitted gun-only colliders,
  wall stops, material impact sounds, rotation into walls and lifecycle cleanup.

### Muzzle geometry implementation

The next stacked layer selects authored muzzle ID 0 first. Known weapon models
without that point use the center of their frontmost gun-only polygon cap;
arm materials are excluded through the existing glove geometry importer.
Explicit barrel-frame profiles distinguish world -Z guns from held -X guns.
Unknown models retain their prior lowest-ID/origin behavior. Model swaps and
current-build loads regenerate the fallback, and left-hand reflection applies
to both the authored point and fallback. Projectile frames remain normalized,
so grip scaling changes muzzle placement without changing speed.

Spawn clearance begins at the tracked firing palm (the camera in flatscreen),
not the calibrated model origin. It sweeps an enclosing sphere for authored
projectile radii/offsets and a tiny sphere for point projectiles, excluding the
player and held objects while retaining anonymous level geometry. Spawn
translation also applies to projectile trails. This prevents forward muzzle
placement across a wall; a controller already pushed into the wall still needs
the later physical-held collision work.

Matched EMP footage demonstrates the former inside-gun origin and corrected
barrel-tip origin. Independent archive polygon decoding supplies regression
coordinates for both hands. This layer does not implement GunFlash launch/
random-bank flags or physical recoil; those remain separate increments.

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

## Refused-shot sound parity

Empty pulls now request authored `Event OutofAmmo`, replacing the unmatched
`dryfire` tag. Broken/destroyed guns request `Event Broken` separately, matching
`cPlayerGun::PullTrigger` in Dark `shkplgun.cpp` (original lines 1263/1269).
The shipped environment schema resolves pistol empty clicks to `out_pist`,
grenade-family clicks to `out_gren`, heavy/shotgun clicks to `out_sg`, and
broken-gun feedback to `gunbrok1`. Existing skill, reload and cooldown gates
retain their priority. A partial magazine below a mode's shot cost also uses
OutofAmmo; no projectile, flash, wear or ammo debit accompanies that refusal.

## GunFlash casing launch

GunFlash flag 1 now creates a detached dynamic casing at its authored ejection
point, preserving the full parsed PhysInitV vector in a normalized barrel frame.
The left VR gun reflects lateral ejection; the right-handed flat viewmodel does
not, even though it occupies the logical left inventory slot. Ordinary flashes
remain attached. Cosmetic casings and impact-spang transients are omitted from
saves so loading cannot relaunch stale effects through the bullet velocity path.

Flat/right/left casing scenarios reproduce the frozen-shell regression on the
parent, then verify upward launch, ejection side and lifetime after the fix.
The flat case saves while a casing is alive and verifies it does not return;
the normal projectile owner-save regression remains green. Matched VR footage
shows the casing rise and fall independently of subsequent hand movement.
Random-bank flag 2 and inherited player movement remain audit followups; this
layer only establishes the distinct authored casing-launch path.

User followup: the EMP footage reveals red splatter. EMP Shot explicitly authors
`swingba2` with `NoRender`, but player projectile creation forces visibility.
Respecting that hidden model and inspecting its legitimate particle riders is
the next presentation fix, alongside the remaining muzzle-flash audit.

## EMP presentation correction

The red splatter had a separate, confirmed resource collision: `blood.pcx` in
`bitmap.crf` is a 32×32 blue glow; `obj.crf/txt16/BLOOD.PCX` is a 128×128 red
splatter. The EMP Blue particle correctly authors the former. Bare texture
lookup selected the latter because object resources precede bitmap resources.
Particle sprites now use a `bitmap/`-qualified key, registered for classic and
25AE base/mod mounts through the existing namespace mechanism. Unqualified
object-texture precedence is unchanged.

Player projectile creation also stopped forcing `RenderType::Normal`. The
initial audit incorrectly treated EMP Shot's hidden `swingba2` model as
permanently hidden; the same change regressed the laser bolt. Both use
`LaserShot`, which was still a no-op. The bitmap namespace correction above
remains valid independently of this mistake.

`LaserShot` now reveals its root after 50 ms with `RenderType::FullBright` and
sets attached particle groups to `Normal`. The pending/completed timer is saved.
[Telliamed's reference](https://thiefmissions.com/telliamed/allscripts.html)
documents `Timer(RenderMe)` and its delayed visibility transition. Timing and
values were checked against the installed 25AE `allobjs-windows-x86_64.dll`:
the RenderMe scheduling call at RVA `0x63810` passes `0x32` milliseconds;
the handler at RVA `0x63860` writes root RenderType 2 and incoming
`~ParticleAttachement` source RenderType 0. Flat/VR render checks now verify
both the initially hidden root and the subsequent visible bolt.
FullBright-specific shading is still a renderer gap: this layer restores the
script's render mode transition and visible geometry, not new lighting logic.
Runtime-created particle riders also remain excluded from saves without being
regenerated on load; that existing lifecycle gap is separate from the saved
root visibility and script timer.

Stasis and fusion projectiles were visible in matched flat and VR debug-runtime
captures on the parent of this fix. Their reported disappearance is not yet
reproduced; those captures do not establish headset or every-mission parity.
The shotgun's authored GunFlash relation contains only `SG Eject`, consistent
with the original link-driven `CreateGunFlashes` path; an added muzzle flash
would be an augmentation.

Remaining particle audit includes authored units, animation frames, tint/fade
and additional jet attachments; this correction does not claim that every
projectile's particle behavior now matches the original engine.


## Visible muzzle flashes

The held-model grip correction rotated the one-sided flash cone 180 degrees,
pointing it back into the weapon and culling its surface from the shooter.
Flash orientation now maps its authored -X axis onto the weapon barrel frame;
a unit regression covers held -X and classic world -Z models.

25AE `ND-gunflash.mtl` also authors a sole unlit `SRC_COLOR ONE` pass using
`$TEXTURE`. Ignoring it rendered a black polygon once orientation was fixed.
The existing material-script parser now recognizes that narrow form, with an
unlit additive shader and blend state scoped to the draw. Ordinary multi-pass
weapon overlays retain existing behavior. Additive materials enter the
transparent pass and scale accumulated light by authored opacity. Flat
attachments now apply RenderAlpha as the VR world pass already does, and carry
render-debug identity so SDK assertions inspect actual flash draws.

The user's followup confirms casing motion improved in #1447 but its long
axis remains upright. The casing asset is authored along Y; launch yaw alone
does not lay it sideways. Correcting initial casing pose is the next layer;
authored random bank and spin remain part of the weapon-effects audit.

Flat AR15 verification also exposed a stale four-model wield whitelist: the
renderer used `ar15_h`, but attachment data still came from `ar15_w`. Flat now
adopts every authored first-person mesh through the existing ChangeModel path,
matching its renderer and restoring the correct attachment positions and axes.
The casing-side regression reproduced on the parent as well; this model fix
addresses that underlying mismatch rather than changing its assertion.


## Casing launch pose

The shell's long axis is authored along Y in both classic and 25AE assets.
The launch path's inherited bullet yaw left it upright. A quarter turn around
launch-frame X now lays it along the barrel, independently of the existing
velocity frame. Flat/right/left scenarios assert the actual spawned pose as
well as upward/lateral motion and expiry; real-mission save/load remains
covered. Matched side footage checks the visual pose while it rises and falls.

Render verification caught the rotate Tweq replacing the entire launch pose
with absolute world-time yaw every frame. Its existing 20 degrees/second spin
now advances relative to the current orientation using elapsed time, preserving
pitch/roll and resuming consistently after save/load. Authored rotate config
rates/axes remain a separate parity audit; this does not claim to implement them.


## Data-driven audit baseline

See [weapon-projectile-audit.md](weapon-projectile-audit.md) for all 38 gun,
setting and ammo combinations and the current verification boundaries.
`cargo dq weapon-audit [mission]` exports inherited weapon/projectile metadata,
including setting-filtered link inputs and unparsed-property gaps. The SDK
matrix exercises both flat and right VR; diagnostic last-projectile identity
keeps instantaneous ray shots observable without changing their lifetime.


## Annelid homing

The new stack starts from merged main `69cffd92`. Both Worm Launcher settings
now execute the authored `Homing` script instead of panicking. `P$Homing` supplies
target mask, range, heading filter, per-pulse turn limit and pulse interval;
`P$TargetTyp` supplies target flags. Shipped AH/AA rockets use masks 1/2, range
50 Dark units, a 27.158-degree yaw/pitch window, an 11.25-degree turn limit and
200 ms pulses. Hybrids carry mask 3 and are eligible for both modes.

The source baseline is Dark `shock/shkhome.h` / `shkhome.cpp`, with the one-time
scan and recurring `HomingPulse` checked against the shipped 25AE script binary
and [Telliamed's script reference](https://thiefmissions.com/telliamed/allscripts.html).
Targets must be alive, referenced, in range and angular bounds, with
terrain line of sight. A lost/dead target is not reacquired. Deliberate port
adaptations are scanning from the actual projectile launch pose for VR hands,
using symmetric distance bounds instead of the source's signed-axis comparison,
and preserving actual launch speed (including modifiers) while steering.

Save/load preserves target identity through remapping, pulse remainder and
world-space projectile velocity. The shared velocity snapshot also prevents
ordinary player-fired projectiles from restarting toward world +Z on load.

Both modes have flat/VR firing coverage and visible off-axis steering/impact
captures. Impact kills the fixture hybrid and visible blast particles expire;
the explosion root and two particle child entities still exist after 26 seconds.
That secondary-effect entity cleanup remains an audit gap. These checks do not
establish full damage/stim parity.

The next separate layer addresses Quest's explicit `render_particles: false`
override. It explains the missing particle orbs despite smaller mesh effects
(the user's four-dot EMP observation); headset before/after verification remains
required before calling that presentation gap fixed.


## Quest particle rendering

Since the initial runtime commit, Quest's `GameOptions` explicitly disabled all
particle rendering while the
shared default and debug runtime enabled it. The Quest layer removes that
override and uses the shared default. This affects particle effects throughout
the level, including EMP, stasis and fusion projectile riders; it is not a
replacement orb or a weapon-specific approximation. The user's four-dot EMP
observation is consistent with mesh effects remaining visible while particles
are suppressed; identifying those exact dots still needs a matched device capture.

The attached Quest disconnected before a usable before/after comparison could
be captured. The existing simulated-VR gallery demonstrates the enabled shared
render path, but does not verify the corrected APK on a headset. Keep this layer
in draft until it is installed, projectile orbs are visibly checked and device
captures are embedded. No device performance claim is made.


## Authored projectile contact damage

`fix/projectile-contact-stims` replaces ray damage 6 and physical impact damage
1 with the existing contact-stimulus resolver used by melee. Projectile sources
select the stimulus/intensity; the receiving object's inherited receptrons select
damage, amplification or immunity. Launch modifiers scale source intensity
before response evaluation, matching `shkproj.cpp::ShockMakeProjectile`'s source
scale property, so flat damage responses remain flat. The existing resolver's
reaction ordering and supported-effect set are retained; this is not full
act/react parity.

Hitbox contacts read the parent creature's receptrons, then send the result
through the original hitbox to preserve limb scaling and ragdoll impact data.
Terminal physical impacts now latch once for damage, spang and sound, avoiding
duplicate application when capsule/limb or adjacent faces queue contacts before
destruction. Non-damage and immune contacts emit no Damage message.

Matched single-shot captures and SDK regressions against OG-Pipe show:

| Weapon | Previous HP | Authored HP |
| --- | --- | --- |
| Pistol | 12 -> 7 | 12 -> 9 |
| Laser pistol | 12 -> 11 | 12 -> 10 |
| Stasis | 12 -> 11 | 12 -> 12 |

Standard Bullet authors Standard Impact at 4; the fixture hit crosses a limb
with the existing 0.75 multiplier. Laser Shot authors Energy Stim at 2, received
at x1 by the hybrid. Stasis Shot authors Stasis at 8; its receiver authors
`Freeze` and `add_metaprop`, not damage. **Stasis freezing is still unimplemented:**
this layer removes its erroneous HP loss. Implement those reactions, their
expiry/reapplication behavior and current-build saves in the next increment.

All three new firing tests fail against the parent and pass after the fix;
1,622 gameplay library tests pass (2 ignored), as do warning-denied runtime
checks. Thirteen additional projectile/owner/homing/proximity/ranged scenarios
pass. `ranged-hitbox.e2e.test.ts` still fails to land its centred shot on both the
parent and this layer; the new pistol scenario independently confirms a real hit
retains its limb identity. Do not count that existing failed scenario as green.

[Matched PNG](https://gist.githubusercontent.com/tommy-xr/0f0374285dddac7fab90c2b24983e8d3/raw/comparison.png)
and [capture gallery](https://gist.github.com/tommy-xr/0f0374285dddac7fab90c2b24983e8d3)
provide flat-runtime evidence. Quest particle validation remains with the user.
