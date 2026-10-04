# Weapon modification implementation plan

Build one complete weapon upgrade flow first, then add independent capabilities and broaden weapon coverage. The first playable milestone is a pistol that can be modified through the normal interface, receives the selected benefit and automatic bonuses, and preserves those changes through save/load. The state and shared-stat layers are implemented. The chooser layer now connects the pistol to paid and French-Epstein installation, including the alternate-fire purchase and lock. Other families retain their legacy modification flow during staging; attachment effects and broader weapon coverage follow.

The existing retail implementation is in `shock2vr/src/weapon_modification.rs`; its binary-audit provenance is recorded in [the upgrade audit](os-upgrades-audit.md). This plan separates confirmed rules, remaining decisions, and independently reviewable implementation steps.

## Confirmed rules

- Four total modifications per weapon. Ordinary Modify stops at two; French–Epstein devices enable tiers three and four.
- Each modification grants one choice plus an additive 8% of base damage: +8%, +16%, +24%, +32%.
- Each modification removes 5% of base wear, additively: 5%, 10%, 15%, 20%. Multiply the result by the Low Maintenance factor; tier four with rank II uses 40% of baseline wear.
- The new bonuses replace the retail modification packages.
- Choices include flashlight, laser, pistol/AR15 silencer, Low Maintenance I and II, alternate fire, and doubled clip capacity.
- Low Maintenance progresses from 25% to 50% less wear over two choices; rank II replaces rank I's reduction.
- Alternate fire starts locked until purchased through modification.
- Physical attachment meshes are deferred. Reuse hand spotlights for flashlights. Laser presentation includes a surface dot and a cylindrical beam with a smoky glow.

## Three design areas

### Progression and weapon rules

Define one compatibility table per weapon family: available choices, maximum ranks, prerequisites, baseline damage/wear/capacity, and the benefit for a weapon without conventional damage or ammunition. Maintain a distinction between total modification tier and the installed capabilities. Derive final values from the original weapon data plus choices, rather than repeatedly multiplying already-modified values.

Decisions needed before the affected implementation:

| Decision | Suggested default | Needed before |
|---|---|---|
| Devices at tiers zero and one | Implemented: one device purchases the next tier without Modify skill or nanite cost; normal Modify still stops after any two total upgrades | Complete for pistol |
| Installation permanence | Implemented: permanent choices; no refunds or respec | Complete for pistol |
| Paid cost, challenge, and failure | Implemented: existing skill requirements, nanite costs, Tinker discount, challenge, and failure consequences | Complete for pistol |
| Energy capacity | Decide explicitly whether double capacity applies to charge stores as well as ammunition magazines | Energy weapon coverage |
| Weapon coverage | Ranged weapons only, including energy and biological capacity; melee and psi amp cannot be modified | Confirmed |

The coverage requirement is all ranged weapon families; melee weapons and the psi amp are excluded. Starting with the pistol is implementation order, not a decision to exclude other weapons. No universal placeholder upgrade should be introduced without deciding its actual gameplay benefit.

### Interaction and installation

Use the same sequence for flat and VR: inspect the specific weapon, open Modify, choose an eligible upgrade, preview its result, then apply it. Show total tier, installed choices, before/after stats, cost or device use, and reasons for unavailable options. Paid Modify runs the existing challenge after selection; device use opens the same chooser and bypasses the challenge.

Define cancellation, invalid targets, broken weapons, hands changing ownership, and the target being dropped or changed while a chooser is open. Revalidate at completion using the weapon identity and expected previous tier. A stale or duplicate completion must not install twice or charge twice. Consume the device only in the successful installation operation. Paid attempts retain their existing nanite spending and failure rules.

Keep flashlight and laser controls per weapon in its existing interface initially. Their enabled preferences survive stowing, but their effects stop while stored or holstered. Keep the same resolved layout and eligibility in flat and VR, with explicit targeting of the weapon in the chosen hand.

### Effect presentation

- Flashlight: reuse hand spotlights, gated by the installed upgrade and weapon toggle; add the matching equipped-weapon behavior in flat mode. Avoid duplicate lights from the development override.
- Silencer: adjust AI gunshot noise, firing audio, and muzzle flash together. Preserve impact noise and casing ejection. No new mesh, muzzle displacement, or collision shape is needed.
- Laser: resolve the nominal shot target before spread, including the camera/muzzle distinction between flat and VR. The first obstruction from the physical emitter determines beam length and dot position. In flat mode the sight converges toward the crosshair, but muzzle-adjacent cover can block the laser even when the camera-origin shot clears it; installing a sight does not change the existing firing policy or accuracy. No hit means a bounded fading beam with no floating dot.
- Smoky beam: add a thin bright core and a soft cylindrical halo with subtle animated noise. Start with a procedural material and bounded geometry; no scene-wide fog system is required. Preserve depth testing and check stereo appearance and transparency cost.

Physical attachment models, authored sockets, and hands-on assembly gestures remain later work. They should consume the established upgrade state and effect origins.

## Proposed pull request sequence

Each row is one logical change, with small commits inside it where useful. If integrating a row requires unrelated refactoring, split that preparation out rather than widening the feature PR.

| PR | Result | Main verification |
|---|---|---|
| 1. Weapon upgrade state and rules | Serializable per-weapon choices and toggle preferences; pure eligibility and stat evaluation; total tier derives from choices | Rank prerequisites, duplicates, four-tier cap, exact additive damage and wear math, save round-trip |
| 2. Shared effective weapon stats | Firing, condition loss, reload, charge capacity, and readouts obtain values from the same evaluated state | All consumers agree; existing behavior remains intact before the new installation path is enabled; no double application of retail and new bonuses |
| 3. Paid choice flow | A complete pistol flow with Low Maintenance and extended capacity, using the existing challenge and the new automatic bonuses | Cancel/failure/success, stale completion, paid two-tier cap, ammo conservation, flat/VR interaction and save/load |
| 4. Alternate fire purchase and lock | The chooser offers the capability and every mode-setting path enforces it | UI, hand buttons, direct effects, restored mode, ammo compatibility, and actual projectile behavior; primary fire remains available |
| 5. French–Epstein devices | The existing device can purchase a selected eligible upgrade, including tiers three and four | One successful install consumes one device; cancellation/rejection consumes none; repeated completion and fifth-upgrade attempts fail safely |
| 6. Flashlight behavior | Installed flashlights reuse hand lights and work in flat mode | Two hands with different upgrades/toggles, stowing, hand changes, save/load, light budget and device performance |
| 7. Laser aiming and surface dot | An installed laser marks the first visible obstruction using the authoritative aim | Near walls, moving targets, misses, left/right hands, flat/VR aiming, no hidden accuracy change |
| 8. Cylindrical smoky laser beam | The laser gains its required translucent beam and fog-like halo | Beam/dot agreement, occlusion, no persistent trail on fast movement, stereo appearance, Quest frame cost |
| 9. Silencer behavior | Pistol and AR15 gain reduced noise, quieter audio, and reduced flash | AI hearing changes independently of audio; impacts and casing effects still work; both weapon families verified |
| 10. Weapon coverage and campaign balance | Complete the compatibility matrix and useful benefits for remaining families; review device availability | Representative weapon from every supported family, same-build save/load and level transitions, device supply and build tradeoffs |

PR 10 is an umbrella milestone: split it by weapon family and keep loot/balance edits separate. It is not one large PR. Likewise, if weapon-stat consumers are too broad for PR 2, split damage/wear from capacity/reload integration while preserving the same evaluated-state API.

PR 3 must replace the old modification packages for weapons enrolled in the new flow. During staging, any retained legacy behavior for other families must be explicit; both systems must never apply to the same weapon. Retire that temporary routing once coverage is complete. This is a development transition, not a save migration: pre-existing saves do not require compatibility.

Only expose choices whose effects work. PR 7 establishes the laser's functional behavior; the requested laser presentation is complete after PR 8. Alternate fire's lock must ship together with its available purchase route in PR 4, never as an earlier standalone restriction.

## Dependencies and milestones

The shared foundation is PR 1 → PR 2 → PR 3. PRs 4 through 8 can then be developed independently against the same eligibility, installation, and effective-stat interfaces. The smoky beam depends on laser aiming and the dot. Weapon-family work can proceed once those interfaces are stable; the final campaign pass follows completed device and capability behavior.

1. **Playable modification:** finish PRs 1–3. A pistol can receive either paid upgrade, display the correct result, and round-trip through save/load.
2. **Complete progression:** finish alternate fire and devices. Exercise a four-choice build such as extended capacity, alternate fire, Low Maintenance I, Low Maintenance II; verify +32% damage and the agreed wear formula.
3. **Complete effect set:** finish silencer, flashlight, and laser including its smoky beam. Exercise a second build such as flashlight, laser, silencer, and extended capacity.
4. **Campaign readiness:** finish the agreed weapon matrix, device-supply audit, and representative campaign/device testing.

The earliest milestone is deliberately playable. It validates the choice and persistence model before visual work depends on it, without requiring every family or effect to be complete.

## Verification and completion criteria

Use focused tests for numerical rules and installation invariants, then debug-runtime scenarios for actual gameplay. A tier-four save must preserve the exact weapon, choices, toggle preferences, ammunition, condition, and mode after load, dropping, hand transfer, inventory storage, and level transitions. Verify device consumption and upgrade installation together so neither can succeed alone.

Every UI or visible effect change needs flat and VR render verification plus the repository's before/after PNG and looping GIF evidence. Quest checks are necessary for tracked-hand behavior, stereo appearance, and performance; debug-runtime captures alone cannot establish those. No such captures or checks are required for this planning document itself.

Campaign balance should compare useful builds, not only maximum damage. Confirm that alternate fire does not become a compulsory purchase on every weapon, Low Maintenance does not eliminate the maintenance economy unintentionally, and the device supply supports the intended number of heavily modified weapons. The standard +32% ceiling is about 5.3% above the common retail +25.4% multiplier; grenade and stasis exceptions require separate balance judgment.

Implementation starts with the persistent state and pure rules. Settle the remaining interaction defaults and sketch the choice panel before connecting installation to gameplay. None of the proposed defaults beyond the confirmed rules are treated as already approved.

## Chooser implementation

The pistol uses one shared 188x300 canvas in flat and VR. Modify opens the chooser; selecting a row previews the automatic bonuses and selected capability. Paid confirmation opens the existing HRM challenge. The chooser reuses the retail Modify bezel and training-screen button texture, with nine-patch borders resolved in shared layout. Installed upgrades are removed from the choices, and Low Maintenance II appears only after I is installed. Selection uses an interior highlight without arrows; the compact list needs no scroll controls. A carried French-Epstein device can be selected with Use device, double-clicked in the inventory strip, or activated with its held-hand trigger to open the chooser directly for the selected pistol. Device confirmation consumes one unit only after target, tier, ownership, and eligibility are revalidated. Closing or backing out consumes nothing.

Low Maintenance I/II, doubled capacity, alternate fire, and flashlight are active choices. Flashlights switch on after installation and have a per-weapon Settings toggle; only equipped weapons emit light. Laser now has a saved Settings toggle and a surface-aligned dot at the first obstruction; the same clipped segment now draws a thin core and animated cylindrical smoky halo. Silencer remains labelled as a later addition until its effects are implemented. Pistol alternate fire starts locked; mode selection and effective firing settings share the purchase check. Upgrade state remains attached to the original weapon across saving and level transitions. Other weapon families remain on their previous behavior until the coverage milestone.
