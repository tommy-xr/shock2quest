# Item-targeted psi powers

Scope: ElectroPsi (#1283), Fabricate (#1284), and Alchemy (#1275).

## Integration review — October 6, 2026

Rebased onto current main. The original Recycler implementation and its flat/VR
input overrides are superseded by `item_tools`: keep that shared tool handler,
preview, and trigger/release gestures. This PR now only adds psi targeting.
Recycler regression coverage belongs to the existing item-tools scenarios.

ElectroPsi uses `AdjustEquipmentEnergy` so recharging an equipped implant
restores its bonuses. The picker accepts implant socket targets without
unequipping them; ordinary inventory and equipment gestures remain available
after cancellation. Powered armor uses the same energy transition.

## Authored behavior

- ElectroPsi: `Data1 * effective PSI` charge units (20 per PSI), capped by
  Maintenance. Energy weapons express charge as a percentage of clip capacity;
  other powered items use `Energy`. Preserve condition and ownership.
- Fabricate: add the item's `Fabricate` quantity for `FabCost` nanites.
  Success chance is `Data1 + Data2 * effective PSI`. Failure spends psi only;
  success adds to the existing stack. Missing or zero quantity refuses.
- Alchemy: consume `min(StackCount, StackInc)` (absent values default to one).
  Award `floor(quantity * Alchemy * Data1 * (0.8 + 0.2 * effective PSI))` nanites.
  Missing or nonpositive value refuses.

Original arithmetic research: installed gamesys and English `psihelp.str`,
`~/code/darkengine/src/shock/shkpsipr.cpp` and `shkprop.cpp`, and installed
allobjs script observations recorded September 21:
`~/ss2-25th/allobjs-windows-x86_64.dll`, Alchemy handler RVA `0x3e7f0`,
Fabricate `0x3ee80`, and ElectroPsi `0x3e530`. These addresses identify the
installed version, not portable binary offsets. See also the
[Telliamed script reference](https://thiefmissions.com/telliamed/allscripts.html).
The issue sketches are hypotheses; they do not override the authored values.

## Interaction and verification

VR casts at the item held opposite the amp, in either hand. Flat opens the
shared inventory canvas and waits for a target click. Opening costs nothing;
Tab cancels, changing powers invalidates the request, and refusals cost nothing.
Each transaction resolves and applies before the next queued operation.

Focused scenarios cover charge caps, equipped-implant recharge, powered armor,
authored duplication/transmutation, invalid targets, cancellation, and save/load.
Current validation results are recorded in the PR description.
