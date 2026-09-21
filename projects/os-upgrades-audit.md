# O/S upgrade audit

Audited 2026-09-20. Scope: panel parity, description containment, and implementation feasibility for every disabled upgrade. Gameplay unlocks/balance changes are follow-ups, not part of the layout fix.

## Reference and asset provenance

The classic descriptions are in `sshock2.kpf`, `data/res/strings/TRAITS.STR`. Original engine references are `shock/shktrait.cpp`, `shktrcst.h`, `shkplgun.cpp`, `shkmelee.cpp`, `shkscomp.cpp`, `shkhrm.cpp`, `shkscrpt.cpp`, `shkloot.cpp`, and `shkmap.cpp` in the local Dark Engine source checkout. The shipped descriptions are an important cross-check: this source snapshot contains obsolete/commented-out traits and even uses the lethal multiplier in its Sharpshooter path.

The test installation mounts `mods/scp.kpf` (Community Patch), whose `strings/traits.str` overrides the classic descriptions. This is not evidence that those expanded effects exist in our engine. Distinguish classic retail mechanics from patch additions when implementing traits.

## UI findings

Confirmed before the fix:

- Character-count wrapping does not measure the displayed font; long lines visibly escape the description well.
- Five 12px lines starting at y=214 extend to y=274 despite a stated bottom of 264. Further text is silently discarded.
- Refusal/used-machine feedback is prepended to hover help, consuming its already limited space. Availability warnings can disappear after long descriptions.
- Installed icons have no hover descriptions, unlike the original.

The focused fix uses the existing `PanelText` MFD font/metrics and shared paragraph layout, confines descriptions to `(15,214)-(174,264)`, puts availability/used feedback below that well, and lets current hover help replace stale feedback. Installed icons now expose their descriptions. Both presentations consume the same resolved components.

Remaining parity differences (not changed in this increment):

- Purchased traits now disappear from the purchase matrix and no longer expose a hover target there. The original also hides the entire matrix after a purchase; ours retains the remaining choices and refuses another purchase.
- Original draws one empty slot after the installed traits; ours draws all four empty placeholders.
- Original matrix spacing is 35×34 pixels (`TRAIT_W`/`TRAIT_H`); ours uses 34.5×33.5 pitches. Check art/hit geometry before changing this.
- A medsci2 VR capture shows the machine model/hologram overlapping the top of its panel. This is a placement issue separate from text wrapping.
- Pack-Rat intentionally advertises the port's extra VR holster, beyond classic retail.

The sixteen-choice ordering, four stored slots, free single-use acquisition, stable machine identity, save/load, and cross-deck accumulation already have implementation and SDK coverage.

## The ten disabled upgrades

Sizes below are relative engineering scope, not time estimates. Every unlock needs positive/negative tests, save/load checks, and an end-to-end acquisition/effect test. Keep `live_effect_note` disabled until the effect is verified.

| Upgrade | Classic behavior | Existing implementation and proposed work | Scope |
| --- | --- | --- | --- |
| Speedy | +15% movement speed | Shared locomotion in `mission_core.rs` uses `PLAYER_MOVE_SPEED`. Apply an owned-trait multiplier to player locomotion; avoid boosting detached-camera movement, tracked hand climbing, or physical head movement. Verify flat/VR distance over fixed simulation time and across load. | Small |
| Sharpshooter | +15% ranged, non-psi damage | `scripts/weapon_script.rs` already carries projectile stimulus/speed modifiers. Add the trait to the shared player-shot stimulus calculation, retaining weapon skill/fire-mode scaling and excluding enemy shots and psi. Verify projectiles and explosions do not double-apply. Do not copy the old source's apparent lethal-multiplier mistake. | Small–medium |
| Cybernetically Enhanced | Two implants simultaneously | `statboostimplant`/`testimplant` are unimplemented; hazard equipment only supports a special WormHeart path. Needs general implant equip slots, powered effects/drain, duplicate rules, UI, ownership and save/load, then the second-slot unlock. A second inventory slot alone would not implement this upgrade. | Large prerequisite |
| Lethal Weapon | +35% melee damage | Flat attacks in `weapon_script.rs` and VR contact attacks in `melee_weapon.rs` are separate callers. Add one shared player-melee multiplier, compose it with Berserk, and decide rounding centrally. Verify wrench/rapier/psi sword, flat and VR, and exclude thrown props/enemy attacks. | Medium |
| Security Expert | +2 Hack at security computers, requires at least Hack 1 | `ComputerGui.security` identifies the context, but shared `keypad.rs` hack calculations only read base skill. Thread a contextual bonus into the same eligibility/chance/mine calculation used by the board. Do not grant general hacking skill or bypass base training. | Medium |
| Smasher | Charged overhand melee attack | Original switches to a long swing while holding an attack. Flat needs charge timing/animation/damage; VR uses trigger-free physical swings, so needs an intentional gesture or charge interaction with clear feedback and anti-repeat rules. Simply multiplying all melee damage is not equivalent. | Large/design decision |
| Cyber-Assimilation | Destroyed robots yield a repair module that heals 15 HP | `PropGuaranteedLoot` is parsed and the death-loot path already checks trait 12, but selection is still disabled. The actual module (template -436, `Cheeseborger`) uses the unimplemented `cheeseborger` script: the healing half is missing. Implement 15-HP module use, audit robot drops/loot-once/corpse timing, and add a full acquire → robot death → loot → heal regression before enabling. | Medium, partially implemented |
| Power Psi | Burnout causes no HP damage | `psi_amp_script.rs::burnout` centralizes failed cast, point spend, meter flash and HP loss. Gate HP loss on the trait, retain the failed cast/feedback, and verify point-spend semantics separately for classic versus SCP. | Small |
| Tinker | Weapon modification nanite cost halved | Several weapon modification scripts are no-ops; free modification is unimplemented. Implement a real paid Modify transaction/board and resulting gun changes first, then apply the discount consistently to quoted and charged cost with a minimum/rounding policy matching retail. | Large prerequisite |
| Spatially Aware | Entire sublevel map revealed | `scripts/gui/map.rs` draws explored-location decals. Add a trait-based reveal path using appropriate full-map art/location data without destroying the actual exploration history; handle all floors and level transitions. SCP enemy markers are separate work. | Medium |

Suggested sequence: fix text/provenance, then Power Psi and Speedy, Cyber-Assimilation, ranged/melee multipliers, Security Expert, full-map reveal. Implement implants and modification as independent features; design Smasher's VR interaction explicitly.

## Six selectable upgrades also need a parity pass

| Upgrade | Current effect | Audit finding |
| --- | --- | --- |
| Strong Metabolism | Radiation/toxin consumer applies -25% radiation / -50% toxin | Classic and SCP text promise -25% for both. SCP also promises immunity to harmful recreational-substance effects. Follow-up: `projects/radiation-toxins.md` records original September 1999 OSM disassembly proving toxin ×0.5; retain that actual retail behavior and describe it explicitly. |
| Pharmo-Friendly | Healing/hazard item consumers apply a 20% bonus | `PsiKitScript` and `apply_psi_kit_use` do not apply it to psi hypos, despite classic text saying all hypos. SCP additionally promises faster healing. |
| Pack-Rat | Three inventory slots and an extra VR holster | Deliberate port extension; custom description reflects it. |
| Naturally Able | Eight modules, once | Matches classic text. Mounted SCP text promises twenty. |
| Tank | Five maximum and current HP | Matches classic text. Mounted SCP text promises ten. |
| Replicator Expert | 20% price discount | Matches classic text. Mounted SCP additionally promises easier replicator hacking, which is not supplied by the trait. |

Additional SCP promises on disabled traits: Security Expert's robot invisibility during hacked security; Tinker's stronger modifications; Spatially Aware's enemy markers; Power Psi's zero burnout point cost. Do not enable a trait while continuing to advertise unimplemented additions without making the supported behavior explicit.

A coherent follow-up should derive supported descriptions and effect values from a single selected ruleset (or explicitly describe port behavior), instead of combining imported patch promises with hardcoded classic effects. That choice should precede a balance overhaul.

## Validation

- Focused trait unit tests: eight pass, including installed-icon help and description-well containment/stale-feedback regression.
- Render comparison: all 16 mounted descriptions retained in full in both flat and VR (32 checks); both presentations match and emitted text rectangles stay in bounds. Before/after PNGs and looping GIFs are under `/tmp/os-traits-visuals/`.
- Existing `os-traits.e2e.test.ts` SDK scenario passes: unsupported-choice refusal, Tank grant, single-use machine, save/load, deck transition, and Naturally Able at a second machine. Updated the message assertion to compare text across wrapped lines.
- `cargo fmt --all -- --check` and `git diff --check` pass.
- These checks establish headless flat/VR geometry and interaction behavior, not headset readability or exact parity with an independently running retail executable.
