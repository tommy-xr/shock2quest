# Radiation and toxins

Implemented on `feat/radiation-toxins`, based on local main `ae2eaf5a`.
Audit date: 2026-09-12. Evidence is the local Dark Engine source, shipped
`Data/shock2.gam` and missions, and x86 disassembly of `Data/allobjs.osm`.
The OSM is a stripped September 1999 PE32 DLL; no separate symbol file was
found. Binary-derived formulas are documented below so they can be compared
with other releases; remaster-specific script differences remain unverified.

## Gameplay evidence and implementation

`references/darkengine/src/shock/shkhazpr.h` declares separate ambient radiation,
accumulated radiation, absorption/recovery and toxin properties. The existing
`ActiveRadiation` save state now carries toxin severity and its timer too.

| Behavior | Authored/retail value | Evidence |
| --- | --- | --- |
| Radiation absorption | every 0.1 s, player default 0.05 toward ambient ceiling | OSM 0x100149f0/0x1001683c; RadRoom -1280 |
| Radiation damage | every 6 s, truncate(level × END multiplier × metabolism) | OSM 0x10014a33/0x10016e52, arithmetic 0x1001690d–0x100169c8 |
| Radiation recovery | subtract authored RadRecover after damage when ambient is zero | OSM 0x10016c9c–0x10016dbe; player property is disk-truncated `P$RadRecove`, value 3 |
| Toxin damage | every 10 s, truncate(max(1, severity × END multiplier × metabolism)) | OSM 0x10014a76/0x100172c5; minimum 0x10016fd9–0x10016ff0 |
| Toxin recovery | no natural decay | no decay write in toxin timer |
| Endurance multipliers, END 1–8 | 1, .94, .85, .73, .58, .4, .2, .01 | STATPARAM; `shkparam.h` sStatParams; `shkscrpt.cpp` GetHazardResistance |
| Strong Metabolism | radiation ×.75, toxin ×.5 | OSM trait checks; trait 1 |
| Rad Patch | -6 radiation, -7.2 with Pharmo-Friendly | OSM RadPatch; trait 2 |
| Detox Patch | -2 toxin, -2.4 with Pharmo-Friendly | OSM 0x1002366c and multiplier branch |
| Vacc Suit / Worm Skin | 75% / 30% radiation and toxin protection | `P$Armor` toxic/radiation/combat floats, templates -83/-84 |
| WormHeart | suppress toxin damage while equipped; leave severity intact | OSM 0x10016efb, implant type 12 |

Direct `radiate` and `toxin` reactions raise severity to the maximum of current
and protected incoming intensity, rather than adding every hit. See
`shkreact.cpp` reaction handlers. Contact and blast reactions preserve authored
amplification and the existing radiate multiplier resolver. A review nuance:
original `shkreact.cpp::radiate_func` does not use its `increm` parameter, while
that pre-existing resolver does. Non-unit authored parameters need a separate
compatibility check; this change reuses the existing behavior. Persistent radius emitters remain ambient
sources and room entry/exit drives ambient exposure through the existing
`internal_room_trigger` sensor forwarding, not physical colliders on room markers.
Overlaps choose the highest ceiling, then the highest absorption rate on ties.
This is a deterministic policy for this port's overlapping room sensors; it does
not claim to reproduce retail's room-boundary dispatch order.

The previous partial radiation implementation used level ×.25 for damage. The
OSM arithmetic uses the full level before Endurance/trait scaling; this change
corrects that approximation. Radiation below one clears during recovery.
Simulation clocks use f64 and process absorption/damage chronologically, so
large updates agree with 60 Hz stepping. Death resets both hazards; save/load
and deck transitions preserve severity and timer phase. Ambient room ownership
is rebuilt in the destination and is not carried as stale runtime entity IDs.

### Protection and cures

Room accumulation and direct reactions intentionally differ. Armor scales the
room absorption rate, not its final ambient ceiling. Rad Shield's authored value
5 divides room absorption; the direct reaction treats player armor as a percentage
(`shkreact.cpp`). Do not unify these formulas without new retail evidence.
Toxin Shield authors 100% protection. Both powers use the existing sustained-psi
activation/duration system (10 + 5 × PSI seconds) and active durations now survive
save/load and deck transitions. Named templates are -1113 Toxin Shield and -1114
Rad Shield. Protection prevents new exposure, not existing damage.

Frob equips one hazard armor and one WormHeart implant, using carried entity
identity; equipped markers persist through saves/transitions. Research-required
items cannot equip until their object state permits use. This is a minimal
integration with existing inventory use, not a complete armor/implant slot UI.
Strong Metabolism is now purchasable through the existing live-trait gate.

Patches consume their exact source item only when they reduce contamination.
`TrapRadCleanse` and `TrapToxinCleanse` clear player status on TurnOn.
`FlushRadiation` / `EraseRadiation` clear environmental RadLevel and ambient ownership, preserving
player contamination. Clearing all non-player RadLevel properties is intentional:
`shkscrpt.cpp` localClearRadiation does precisely this, rather than following
links from the invoking object. `EraseRadiation` calls the legacy RadiationHack
service; FlushRadiation calls ClearRadiation (OSM 0x100392b0/0x10039350).
`EngineRemoveRad` (0x10039620) additionally slays descendants of its authored
ConsumeType, sets EngineRadClear to raw 1, unlocks EngineCoreDoor and shows
its optional localized UseMsg. Its Engineering runtime test verifies that
subsequent room entry does not add radiation and existing player contamination
survives. Existing persistent radius-source scripts are separate from room
RadLevel ownership.

## Presentation

One 128×66 pixel canvas supplies flat HUD, cyber interface and left wrist:
original radiation art (`radback`, `radmeter`, `radicon`/`radgray`) and toxin pips
(`poisicon`, native 25×32, stride 22). More than five pips show an overflow marker.
The cyber interface always displays status; runtime readouts appear during
exposure or contamination. Active ambient exposure is latched for rendering,
so transient stimulus refresh does not incorrectly display residual radiation.
Flat use mode emits the widget only through the cyber canvas.

VR mounts the same canvas below the existing left glove bio bracelet, at 0.16 m
width, deliberately larger than the 8.5 cm bio bracelet so the original toxin
pips and radiation art can be inspected. That size and the extra hologram depth
are provisional, not a validated comfort choice. It remains available when the hand's glove mesh is hidden by a held item.
Radiation damage uses the existing peripheral damage-feedback layer tinted green,
with the authored `raddmg` sound. This adapts the original full-screen green fade
without adding a separate full-field VR flash. Visual review flags the saturated
green tint and its contrast against green HUD art for the headset tuning pass.
The sound database also contains environmental Geiger schemas (`ms_geigerf`,
`eng_geigers`, etc.) using `geigerF1/F2`, `geigerS1/S2` and `geigerB1/B2`;
this change confirms player damage audio, not every mission ambient sound.

Deterministic flat, wrist and cyber PNG/GIF comparisons have been captured and
inspected. A separate END1 scenario at frame 360 confirms HP 30→12, radiation
18→15, green feedback and `raddmg`. These establish rendering and simulation,
not headset comfort. No Quest was attached: physical readability, occlusion with
both hands occupied and seated/standing placement still need device testing.
Keep the wrist offset provisional; do not promote it to a vr-ui-design rule yet.

## Validation and review

Regression coverage includes persistent/non-stacking toxin, minimum damage,
WormHeart suppression, timestep partitioning, zero absorption/recovery, paused
clocks and serialization phase. Runtime scenarios exercise actual player damage,
Endurance, patch consumption and suit mitigation, plus the existing authored
medsci2 Rad Barrel/Rad Patch scenario. Save/load verifies contamination and
suit protection across entity remapping. A psi-amp cast verifies Toxin Shield
protection and expiry. All 275 SDK test files were exercised across the initial
batch, clean retries and remaining batch. The remaining 201-file batch finished
with 524 passing tests, 24 failures and 10 skips. Every failure was compared
with the unchanged base; the baseline cases are listed below. Core unit tests
finish with 1,702 passed and 2 ignored. Strict core/runtime checks and PR CI pass.

Cross-engine review findings fixed: duplicate implant parser, lost radiate
multiplier, unresearched WormHeart activation, Worm Skin script wiring, missing
live-trait gate, stale active-exposure indicator and duplicate flat use-mode HUD.
Other review concerns (room sensor forwarding, distinct psi formulas and global
environment cleanup) were checked against existing infrastructure and original
source, as documented above.

### Baseline regression failures

The broad SDK run also exercises older features. These assertions reproduce on
unchanged `ae2eaf5a`, as well as this branch:

- `ammo-readout.e2e`: three expected-control lists omit the existing Logs button.
- `creature-loot.e2e`: expects a log reader bound to a discarded pickup entity;
  the reader reports the global panel identity instead.
- `earth-hacking.e2e`: expects a nanite pickup to remain in inventory after
  the existing auto-collection path has converted it to currency.
- `impact-sounds.e2e`: the expected creature collision sound is absent.
- `inventory-strength.e2e`: the VR blocked-cell rectangle differs from the
  test's expected pixel rectangle, identically on both branches.
- `medsci-saved-vr-melee.e2e`: the fresh-control monkey reaches 0 HP rather
  than the expected 1, before its save/transition portion.
- `melee-hitbox.e2e`: the same extra target entry is reported on both branches.
- `psi.e2e`: tries to lower PSI from 6 to 2 through raise-only provisioning.
- `ranged-hitbox.e2e`: its centered shot produces no creature damage.
- `shodan-live-assassin-support.e2e`: cannot find the expected mission object 649.
- `sound-awareness.e2e`: the expected alertness remains Lowest.
- `trap-unlock.e2e`: the locked Engineering button does not emit the expected
  refusal sound.
- `vr-body-calibration.e2e`: exact thigh-position equality fails on tiny drift.
- `vr-cyber-interface-deposit.e2e`: the expected cell rectangle uses an older scale.
- `vr-cyber-interface-pointer.e2e`: its backpack lookup includes the held-item readout.
- `vr-gun-support.e2e`: both pistol-hand support cases fail at the same assertion.
- `vr-gun-weight.e2e`: both AR hands fail to acquire the lowered support socket.
- `vr-held-model.e2e`: the expected full pistol mesh count and left/right melee
  contact-volume equality fail identically on the base.
- `vr-interface-readouts.e2e`: its expected-control list omits Logs.
- `vr-strength-recoil.e2e`: the AR support-grip assertion fails.
- `vr-support-region.e2e`: both wrench support-grip assertions fail.
- `vr-weapon-handedness.e2e`: the forearm readout emits 5 elements, expected 4.
- `weapon-selection.e2e`: spawning template -28 fails during setup.

These are recorded separately from the hazard scenarios; this feature does not
change their expected behavior. All 23 authored mission-load checks pass.
