# Item-targeted psi powers and the Recycler

Scope: ElectroPsi (#1283), Fabricate (#1284), Alchemy (#1275), and the
portable Recycler in flat and VR. Work branch: `feat/item-targeted-psi`.

## Authored behavior checked September 21, 2026

The old issue sketches contain hypotheses. The installed gamesys, English
`psihelp.str`, engine property declarations and installed allobjs script binary
resolve these as follows:

- ElectroPsi: `Data1 * effective PSI` charge units (20 per PSI). Clamp to the
  Maintenance-dependent maximum. Energy weapons use their clip as 100%; powered
  items use `Energy`. Preserve condition, settings and item ownership.
- Fabricate: item `Fabricate` is the added quantity; `FabCost` is its nanite
  cost. `Data1 + Data2 * effective PSI` is the percentage success chance, not
  a value ceiling or quantity. Failure spends psi but no nanites. Success adds
  to the existing stack, preserving its identity. Shipped eligible ammo and
  hypos are stackable. Missing/zero Fabricate refuses.
- Alchemy: consume `min(StackCount, StackInc)`, defaulting absent counts and
  increments to one. Award `floor(quantity * Alchemy * Data1 *
  (0.8 + 0.2 * effective PSI))` nanites. Missing/nonpositive Alchemy refuses.
- Recycler: award `Recycle * StackCount` (one for an unstacked item), consume
  the entire target and keep the reusable Recycler. Missing/nonpositive Recycle
  refuses. Recycler eligibility and Alchemy eligibility are different properties.

Reference: [Telliamed script reference](https://thiefmissions.com/telliamed/allscripts.html).
Local property definitions: `~/code/darkengine/src/shock/shkpsipr.cpp` and
`shkprop.cpp`. Script arithmetic cross-checked in installed
`~/ss2-25th/allobjs-windows-x86_64.dll`: Alchemy handler RVA 0x3e7f0,
Fabricate 0x3ee80, ElectroPsi 0x3e530, Recycler tool use 0x21130.
These are observations of the installed version, not portable binary addresses.

## Interaction

VR: the amp targets the item in the opposite hand using its ordinary trigger
charge/release timing. The Recycler instead accepts a deliberate release of
an item brought close to the hand holding it. Pulling its trigger only explains
that gesture; incidental contact never consumes an item.

Flat: drag an item onto the Recycler, or activate the amp/use the Recycler
from inventory and choose a target on
the existing inventory canvas. Opening does not spend currency. The opening
press is disarmed. Tab cancels; changing the selected power invalidates the
pending request. Refused targets keep their state and cost nothing.

Operations resolve against live state in the mission effect handler. Each
result batch runs before the next queued item operation, including payment and
consumption, preventing stale same-frame purchases or double recycling.

## Verification

- `cargo test -p dark --lib`: 214 passed, including parsing all five properties.
- `cargo test -p shock2vr --lib`: 2,035 passed, three ignored. Item policy tests
  cover authored amounts, chance boundaries, caps, partial stacks, missing
  properties, insufficient resources, self/unowned/empty targets and overflow.
- `tools/shock2-sdk/test/item-targeted-psi.e2e.test.ts`: real inventory clicks
  and trigger/grip input in flat and both VR hand configurations. Covers all
  four tools, implant and weapon recharging, ordinary drops versus feeding,
  currency, target identity, cancellation and consumed physics cleanup.
- The flat scenario transitions to `earth.mis`, saves, loads and checks the
  recharged implant, reusable Recycler and resulting nanite balance.
- SDK unit suite: 63 passed. Android release APK builds successfully.
- Flat/VR GIF and PNG evidence: [captures](https://gist.github.com/tommy-xr/e068e62886d9fc3b2172a70df3eee6e3).
  These are debug-runtime captures, not Quest compositor captures. The attached
  Quest's existing session was preserved; physical feeding comfort remains a
  wearer check.
