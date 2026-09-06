# SystemReShock interactions — one-page reference

Research snapshot: 2026-09-06, plugin `v2` commit
`3c69f183872f2838b0c38cca38bc9816c00f1f2d`. Based on documentation and source
inspection, not a headset playthrough or frame-by-frame video review. This is a
System Shock **Remake** reference; SS2's inventory and credential rules differ.

## What the player does

| Interaction | Documented behavior |
| --- | --- |
| Pick up / manipulate | Grip highlighted items; grip and move levers. Pointing fingers operate consumables and forearm hardware. |
| Backpack | Release a held pickup over either shoulder to store it, subject to inventory space. |
| Weapon holster | Right grip over the shoulder holsters; grip there and bring the hand forward to draw. |
| Access card | Hold left grip at the waist card, then swipe a yellow door scanner. Unlocking still requires the previously collected credential. |
| Body UI | Hands carry UI; right forearm carries hardware controls. Touch the inner left wrist with the right controller and grip to toggle the MFD. |
| Selection / fallback | Hold/release right thumb for the item selector. MFD uses a laser; world interaction also has a laser/button fallback. |

These controls are specified in the [plugin README](https://github.com/gwizdek/SystemReShock-UEVR-Plugin/blob/3c69f183872f2838b0c38cca38bc9816c00f1f2d/README.md#gestures).

## What makes the implementation interesting

**Hands have explicit interaction states.** The exposed hand component includes
an interaction pose, blend weight, pose transform, weapon grip pose, active
interaction source, and snap state. It also exposes nearest-grab queries,
finger overlap handling, and backpack reach checks. This supports a design
built around contextual targets and poses. It does **not** establish a universal
mesh-to-grasp algorithm: these are generated Blueprint interfaces, and the
actual Blueprint graph logic is not shown in the C++ wrappers.
[Hand component interface](https://github.com/gwizdek/SystemReShock-UEVR-Plugin/blob/3c69f183872f2838b0c38cca38bc9816c00f1f2d/SystemShockVR/SDK/_BP_HandInteractionComponent_classes.hpp).

**Two-handed melee is a gameplay state.** The C++ damage path checks
`IsTwoHandingWeapon()` and selects a power-swing montage; otherwise it selects a
fast attack. That verifies explicit two-hand handling, but does not reveal the
grip solver or prove every item fits perfectly.
[Melee implementation](https://github.com/gwizdek/SystemReShock-UEVR-Plugin/blob/3c69f183872f2838b0c38cca38bc9816c00f1f2d/SystemShockVR/plugin.cpp#L1976).

**Feedback and physical placement make actions discoverable.** My interpretation:
a stable place to reach, a contextual hand pose, and feedback before committing
help make the interactions feel coherent. The glove-light behavior and excellent
arbitrary-item fit reported from the video remain user observations here; the
exact light colors, meanings, and fitting method were not established by this
inspection. An ammo pouch is our proposed extension, not a verified plugin feature.

**Transfer to shock2quest:** consistent gloves; explicit grip anchors; waist and
shoulder affordances; one shared interaction result driving pose, highlight,
lights, haptics, and input. Preserve SS2 semantics underneath those gestures.
See [the proposed implementation path](astra-vr-hands-round-2.md).
