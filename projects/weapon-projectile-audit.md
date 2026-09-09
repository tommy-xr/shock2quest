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
- Shotgun pellets carry unparsed `P$Projectil`. Dark `shkproj.cpp:495–514` gets pellet count/spread from the projectile archetype, not BaseGunDesc.spray (shotgun setting spray is zero). Do not implement shotgun spread from the misleading gun field.
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

1. Implement proximity-grenade arming, detection, detonation and cleanup,
   including the contact-mode payload and save/load of deployed mines.
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
