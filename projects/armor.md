# Armor audit and equipment

The October 2026 audit found that `ArmorScript`/`WormSkin` only toggled a
hazard-protection marker, `PoweredArmor` was an unimplemented script, and
`L$Armor Eff` and `P$ReqStatsD` were unparsed. Combat and Worm Skin's PSI
bonus/cost were absent. The paperdoll chest had no equipment readout/control.

## Authored behavior

| Armor (template) | STR required | Combat reduction | Radiation/toxin | Other effect |
| --- | ---: | ---: | ---: | --- |
| Light (-79) | 2 | 20% | 0% | — |
| Medium (-80) | 4 | 30% | 0% | — |
| Heavy (-81) | 6 | 40% | 0% | — |
| Powered / Reflec (-82) | 3 | 50% | 0% | 75% reduction for Energy Stim; one charge per 5 s |
| Vacc Suit (-83) | 0 | 0% | 75% | Slows new exposure, does not cure existing contamination |
| Worm Skin (-84) | 2 | 20% | 30% | Research required; +2 PSI; one PSI/HP per 30 s |

Combat protection comes from the suit's `Armor Effect` links to metaproperty
receptrons, not from blanket scaling of final damage. The covered stimuli are
Standard Impact, Energy Stim, Incendiary, Anti-Human, Armor Piercing Impact,
High Explosive, WeaponBash, Cold, and Droid Fusion. Powered armor authors **two**
Energy Stim factors of 0.5. Untyped damage, such as falling or Worm Skin's own
cost, must not gain an invented armor reduction.

Evidence: mounted `shock2.gam` templates -78 through -84 and effect templates
-3057/-3485/-3486/-3488/-3489; original `shkpldmg.cpp`'s
`ShockEquipArmor`/`ShockUnequipArmor`; `shkplayr.cpp`'s `CheckRequirements` and
`GetStats`; and [Telliamed's script reference](https://thiefmissions.com/telliamed/allscripts.html).
Classic `allobjs.osm` WormSkin handlers at 0x1000b920 and 0x1000bb10 write
`ArmrStatsDesc`'s PSI field to 2/0 and schedule `PsiDrain` with float 30.0. Each
tick decrements positive PSI by one, otherwise adjusts player HP by -1. The
ported behavior is based on those original handlers, not a separate reverse
engineering of the remaster DLL.

## Implementation and use

Inventory USE equips/removes the exact carried entity: VR controller trigger,
flat double-click, or held-item trigger. The shared paperdoll ARMOR chest
shows the suit and its charge/defense/PSI readout. Clicking it removes the suit.
Only one suit can be worn; replacing it does not stack benefits. Armor keeps
its backpack footprint. Stowing held armor in the backpack preserves its
equipped state and drain timer; dropping it into the world removes its equipped
marker. Research and primary-stat requirements reject equip attempts with
feedback.

`armor::active_receptrons` supplies the same authored filters to contact and
radius stimuli. Hazard protection and effective PSI derive from the active
carried suit, so depleted power armor grants no protection. The existing
BaseImplant script implementation also runs powered armor's authored drain
and recharge behavior; recharge stations restore capacity using the same
Maintenance-dependent rule as implants. The unrelated portable Battery script
is still unimplemented.

The equipped marker travels with saved/carried entities. Script timer phase
survives save/load; effective stats derive afresh instead of persisting bonuses
into base training. The psi amp reads those effective stats when selecting a
projectile tier and scaling power durations, so Worm Skin's bonus affects casts
as well as the character sheet. Worm Skin's cost is committed centrally as an
effect, so scripts remain pure.

## Verification

The new VR UI scenario fails on the audit baseline because power armor never
appears in the chest slot. Flat and VR inventory USE and chest removal pass on
the implementation. Other scenarios cover all six suits against live hybrid
melee, requirement and research refusal, one-slot replacement, hazard
protection, Worm Skin's bonus/cost across save/load, powered depletion, and
recharge after a deck transition, real Energy Stim/contact and Cold/radius
damage, and VR backpack stowing for ordinary, powered, and Worm Skin armor.
Unit tests exercise owned/equipped/powered filters and the double Energy Stim
factor. Matching flat/VR PNGs and looping
GIFs were captured with real pointer input in the headless debug runtime.
These verify rendering and input routing, not physical headset comfort.

Focused validation: all 17 armor scenarios pass (the power/save fixture was
rerun after moving it from a generated scene to `medsci1.mis`), all three hazard
scenarios pass, and all 23 mission load checks pass. The five Rust tests matching
`armor` pass, `cargo check` passes for gameplay/desktop/debug runtimes, and the
fast SDK suite passes 82 tests. The broader Rust run passes all 264 `dark` tests
and 2446 gameplay tests, with one existing unrelated flat-HUD message-position
assertion failing and three tests ignored.

The full SDK suite also ran with concurrency 4: 1203 passed, 88 failed, one was
cancelled, 12 were skipped, and two were TODOs. This is **not a clean regression
pass**. Nine failing cases were reproduced against the unchanged baseline,
including the old VR trigger-drag expectation and EMP projectile visibility;
the remaining failures were not all diagnosed in this armor change. The full
run overlapped the final stowing fix and test refinements: its original long
Worm Skin depletion case timed out, while the final save-based boundary test
passed. The 17 final armor scenarios are covered by the focused runs above.
