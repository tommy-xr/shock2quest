# Authored weapon audit

Generated from `cargo dq weapon-audit` on the active 25AE gamesys. This is an inventory of expected routing and costs, not a claim of damage/effects parity.

| Weapon (template) | Setting | Ammo/projectile (template) | Cost per shot | Burst | Cooldown ms |
|---|---:|---|---:|---:|---:|
| Pistol (-17) | 0 | Standard Bullet (-362) | 1 | 1 | 500 |
| Pistol (-17) | 0 | AP Bullet (-492) | 1 | 1 | 500 |
| Pistol (-17) | 0 | HE Bullet (-33) | 1 | 1 | 500 |
| Pistol (-17) | 1 | Standard Bullet (-362) | 1 | 3 | 700 |
| Pistol (-17) | 1 | AP Bullet (-492) | 1 | 3 | 700 |
| Pistol (-17) | 1 | HE Bullet (-33) | 1 | 3 | 700 |
| Assault Rifle (-18) | 0 | Assault Standard Bullet (-2253) | 1 | 1 | 250 |
| Assault Rifle (-18) | 0 | Assault AP Bullet (-2254) | 1 | 1 | 250 |
| Assault Rifle (-18) | 0 | Assault HE Bullet (-2252) | 1 | 1 | 250 |
| Assault Rifle (-18) | 1 | Assault Standard Bullet (-2253) | 1 | -1 | 250 |
| Assault Rifle (-18) | 1 | Assault AP Bullet (-2254) | 1 | -1 | 250 |
| Assault Rifle (-18) | 1 | Assault HE Bullet (-2252) | 1 | -1 | 250 |
| Shotgun (-19) | 0 | Rifled Slug (-516) | 1 | 1 | 1000 |
| Shotgun (-19) | 0 | Pellet Projectile (-524) | 1 | 1 | 1000 |
| Shotgun (-19) | 1 | Double Slug (-3422) | 3 | 1 | 1000 |
| Shotgun (-19) | 1 | Double Pellet (-3423) | 3 | 1 | 1000 |
| Gren Launcher (-21) | 0 | Normal Grenade Proj (-3443) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 0 | Prox Grenade Proj (-1347) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 0 | Incendiary Grenade Proj (-1348) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 0 | EMP Grenade Proj (-1349) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 0 | Disruption Grenade Proj (-1350) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 1 | Bouncy Normal Grenade Proj (-1346) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 1 | Bouncy Prox Grenade (-3444) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 1 | Bouncy Incend Grenade Proj (-3427) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 1 | Bouncy EMP Grenade Proj (-3428) | 1 | 1 | 1000 |
| Gren Launcher (-21) | 1 | Bouncy Disruption Grenade Proj (-3429) | 1 | 1 | 1000 |
| Laser Pistol (-22) | 0 | Laser Shot (-2474) | 3 | 1 | 350 |
| Laser Pistol (-22) | 1 | Big Laser Shot (-2255) | 20 | 1 | 3000 |
| EMP Rifle (-23) | 0 | EMP Shot (-235) | 2 | 1 | 400 |
| EMP Rifle (-23) | 1 | Big EMP Shot (-3421) | 20 | 1 | 400 |
| Stasis Field Generator (-25) | 0 | Stasis Shot (-1352) | 4 | 1 | 200 |
| Stasis Field Generator (-25) | 1 | Big Stasis Shot (-3868) | 8 | 1 | 200 |
| Fusion Cannon (-26) | 0 | Fusion Shot (-232) | 2 | 1 | 1000 |
| Fusion Cannon (-26) | 1 | Big Fusion Shot (-3424) | 2 | 1 | 2000 |
| Worm Launcher (-27) | 0 | AH Annelid Rocket (-1356) | 4 | 1 | 200 |
| Worm Launcher (-27) | 1 | AA Annelid Rocket (-3502) | 4 | 1 | 200 |
| Viral Prolif (-29) | 0 | AH Viral Shot (-1357) | 2 | 1 | 200 |
| Viral Prolif (-29) | 1 | AA Viral Shot (-2695) | 2 | 1 | 200 |

Separate paths: Wrench (-928), Crystal Shard (-28), Electro Shock (-24), PsiSword (-2291), and Psi Amp (-247). Hybrid_Shotgun (-4073) is an NPC gun. These must not be mistaken for missing player projectile links.

## Confirmed source discrepancies

- Fast rays use fixed 6 damage; physical collisions use fixed 1 damage. Authored pistol contact intensity is 4, assault rifle 10, slug 8/16, laser 2/12; target receptrons and ammo type must determine resulting damage.
- Shotgun pellet count/spread comes from `P$Projectil`, now parsed and applied in the pellet follow-up below. Dark `shkproj.cpp:495–514` reads the projectile archetype, not BaseGunDesc.spray (shotgun setting spray is zero).
- Stasis is an authored non-damage contact stimulus, requiring separate behavior; fixed collision damage is not an acceptable substitute.
- Annelid homing remains unparsed; exotic secondary spawns need effect/lifecycle verification.

## Verification boundaries

The SDK matrix exercises actual flat/right-VR pickup, mode selection, ammunition loading, trigger, projectile identity, cost and emitted sounds. The runtime findings below record observed discrepancies. Existing focused cases cover owner filtering, close-wall muzzle clearance, refusal audio, flash/casing rendering and projectile save/load. Damage, pellet count, special effects and all secondary spawns remain explicitly separate checks.


## First runtime pass (2026-09-08 stack baseline)

Final rerun: **70 passing cases and 6 executed TODO failures** across 76
flat/VR cases (35 passes and 3 TODOs per presentation), with no unexpected
failures. The TODOs are the bounce-mode proximity grenade and both worm-launcher
settings in each presentation.

The initial 76-case run found an immediate bounce-mode proximity-grenade
crash (`ProxGrenade` is not implemented). Fast bullets resolve in the firing
frame, so the audit now reads `LastFiredProjectile`, a diagnostic written on
the gun after actual projectile creation; it does not infer failure from a
missing transient entity. Firing-sound checks require the `event=shoot` tag.

The harness explicitly grants all weapon skills at level 6 and loads empty
stasis/annelid weapons before firing. This isolates projectile routing from
eligibility; the separate refusal tests cover insufficient skill. Burst
expenditure is bounded by the authored burst size, but exact burst timing and
shot count remain unverified by this matrix.

Launch routing passing does not verify later impact scripts: proximity contact
mode's spawned `ContactProxGrenade` and `ProxGrenadeTrigger` need lifecycle
coverage too. Neither currently has a runtime implementation. The next fix must
implement behavior, not replace these scripts with no-ops.


Both worm-launcher settings also reach an unimplemented `Homing` script and
crash after firing. Their reload takes longer than the initial fixed wait;
the fixture now waits for the actual reload state to finish before triggering.
The matrix retains these two rows and bounce-mode proximity grenades as
executed TODO tests in each presentation, with explicit failure reasons.
They are not skipped or counted as passing weapon behavior.

### Follow-up order

1. **Implemented in the proximity-grenade stack layer below:** arming,
   detection, detonation and cleanup, including the contact-mode payload and
   save/load of deployed mines.
2. Implement authored annelid homing and verify both target modes and secondary
   effects; remove the worm-launcher TODOs once firing and flight work.
3. Route projectile contact stimuli through target receptrons, replacing fixed
   damage while preserving non-damage effects such as stasis.
4. Parse projectile pellet count/spread and verify shotgun patterns and costs.
5. Verify remaining impact/explosion lifecycles, exact burst cadence, accuracy
   and stat modifiers against Dark Engine behavior before adding spring recoil.

Strength-dependent VR recoil remains the planned augmentation documented in
[weapon-feel-workstream.md](weapon-feel-workstream.md); it does not replace the
original accuracy or damage rules.

### Reproduce

Export the data with `cargo dq weapon-audit > weapon-audit.json`. The CLI also
accepts a mission to include its template overrides. The checked-in fixture
records the 25AE gamesys matrix above; it should be reviewed when assets change.

From `tools/shock2-sdk`, run `npm run build`, then
`SHOCK2_E2E=1 node --test dist/test/weapon-audit.e2e.test.js`.
Set `WEAPON_AUDIT_PRESENTATION=flat` or `vr` to run one presentation.
TODO cases execute and retain their crash output in the test report. No physical
headset verification is claimed by this deterministic debug-runtime matrix.


## Proximity-grenade implementation

The next stack layer implements `ProxGrenade`, `ContactProxGrenade` and
`ProxGrenadeTrigger`. Bounce mode arms at physics sleep and changes to its
last authored model; contact mode arms the deployed `MissSpang` mine. An
actual `Prox Grenade Trigger` owns the authored sensor dimensions and
`Corpse -> HE Explosion`. Live AI collider overlap or damage to the armed
mine detonates that trigger once. Player and scenery overlap do not trip it.
The mine and sensor are removed together, including non-damage removal.

The sensor follows any displacement of its mine before script damage and
overlap checks. Kinematic sensor poses are updated immediately so sensing and
the explosion cannot lag a moving mine by a physics step.

The mine/sensor pair uses saved, remapped `ScriptParams` links. Arming zeros
the mine's persisted initial-velocity property, preventing load reconstruction
from launching it again. Contact mines are gameplay objects even though they
arrive through an impact-spang link, so they are excluded from transient-FX
save suppression.

Source boundaries: the [allobjs script inventory](https://thiefmissions.com/telliamed/allscripts.html)
identifies sleep-based bounce arming, contact activation and AI-triggered
sensors. Local Dark sources `physics/phcore.cpp:4211` and
`physics/phconst.h:73` define terrain reflection using object elasticity times
`kTerrainBounce = 0.1`. This correction is scoped to proximity grenades here;
other objects still use the existing port calibration. Rapier additionally
uses angular damping 1.0 (linear damping 0) on bouncing mines to compensate
for missing rolling resistance. That damping is a port approximation, not an
assertion of exact Dark material or settling parity. The original script
inventory does not provide the complete script implementation.

Validation includes actual firing, deployment, live AI-triggered detonation,
sensor physics and single-HE blast checks in flat and VR; both modes save/load in `earth.mis` without
relaunching or duplicating sensors. Four proximity audit rows now pass and
have no TODO. The original baseline above remains historical: its remaining
known launch failures are the four worm-launcher presentation/mode cases.
General projectile damage, pellet spread, homing and other effect lifecycles
remain the follow-ups listed above.


## Annelid homing followup

Both worm-launcher modes now fire without the unimplemented-script panic.
All four affected flat/VR matrix rows pass and their TODO annotations are
removed; the historical 76-case baseline above is not a new full-matrix rerun.
Authored homing masks, angular limits, pulse timing and target remapping are
covered by focused tests. Both modes visibly steer toward a hybrid (target
mask 3), hit it, and remove the rocket and its flight riders. Visible impact
particles expire, but three secondary effect entities persist after 26 seconds;
cleanup remains open. See [the workstream](weapon-feel-workstream.md#annelid-homing)
for source references and intentional VR adaptations.


## Contact damage followup

Fast rays and physical projectiles now resolve authored contact stimulus damage
through the receiving object's receptrons, replacing fixed 6/1 damage. The shared
source multiplier applies before responses, and hitbox forwarding retains limb
scaling. Immune or non-damage contacts emit no Damage message. Physical terminal
contacts are handled once even when multiple collision messages are queued.

Pistol, laser and stasis actual-shot regressions fail on the parent and pass on
this layer. Stasis no longer removes an erroneous hit point. #1465 implements
native Freeze, duration/reapplication, expiry and save/load; the companion
add_metaprop/FreezeFX behavior remains pending. Full act/react ordering,
remaining non-damage reactions, special-effect entity cleanup and complete
weapon/target damage-matrix verification remain open. See the workstream's
[contact damage section](weapon-feel-workstream.md#authored-projectile-contact-damage)
for observed values, source evidence and the known baseline test failure.


## Shotgun pellet count and spread follow-up

Both pellet archetypes carry the same packed six-byte `P$Projectil` record:
`06 00 00 00 00 04` (`i32 count = 6`, `u16 spread = 1024`). Dark angle units
encode one turn in 65536 units, so each pellet receives independently sampled
heading and pitch offsets in **[-5.625°, +5.625°]**. This is a square angular
distribution, not a uniform circular cone. The global heading/pitch axes match
`darkengine/src/shock/shkproj.cpp:303–314,495–514`; gun roll does not rotate that
distribution. `engfeat/projbase.h` defines the two packed fields.

| Setting | Projectile | Pellets | Spread per axis | Ammo cost | Contact source |
| --- | --- | ---: | ---: | ---: | --- |
| Normal | Pellet Projectile (-524) | 6 | ±5.625° | 1 | High Explosive (-376), intensity 1 each |
| Triple | Double Pellet (-3423) | 6 | ±5.625° | 3 | High Explosive (-376), intensity 2 each |
| Normal | Rifled Slug (-516) | 1 | 0° | 1 | Existing authored source |
| Triple | Double Slug (-3422) | 1 | 0° | 3 | Existing authored source |

The player weapon path expands only the launch effect. Each pellet retains its
owner filtering, shot modifiers and flat eye/VR muzzle origin, and independently
resolves contact damage and impact effects. Ammo, sound, muzzle flash, casing and
wear remain once per shell. The launch descriptor cache uses the existing
property hydrator for inherited values and mission overrides.

`tools/shock2-sdk/test/shotgun-pellets.e2e.test.ts` fires both settings and both
ammo types in flat and VR, verifies six pellet impacts versus one slug impact,
checks bounded two-axis spread, and checks unchanged cost and firing-sound
count. Two close-range cases verify six separate creature hitbox contacts and
lethal aggregate damage against a 12HP hybrid (Human Vulnerability receives
High Explosive at x4 before the existing limb multiplier). Matched captures at
4m and 11.5m show one impact before the fix and six after it. Exact random
patterns vary between shots; deterministic stepping does not seed gameplay RNG.

![Shotgun pellet comparison](https://gist.githubusercontent.com/tommy-xr/47770507a648941ea8a6f2a5736b5e8a/raw/near-0-comparison.gif)

![Near/far, both settings](https://gist.githubusercontent.com/tommy-xr/47770507a648941ea8a6f2a5736b5e8a/raw/all-comparisons.png)

This layer does not implement gun-wide accuracy, Sharpshooter/stat modifiers,
recoil, or change the separate AI projectile launch path. Remaining impact and
explosion lifecycles, burst cadence and accuracy/stat parity precede the planned
Strength-based VR spring recoil augmentation.
