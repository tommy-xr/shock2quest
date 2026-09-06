# VR hands / interactions, round 2 — proposal

Status: direction approved by the user, 2026-09-06; tuning and perk selection
remain open. First implementation slice restores the glove appearance and adds
the [interaction workbench](astra-debug-interactions.md). Automatic fitting,
Explorer editing, watch/lights, support grips, and body slots remain planned.
Companion: [SystemReShock one-page reference](astra-systemreshock-interactions-reference.md).

## Recommendation

Use the rigged `vr_glove_model.glb` with its glove appearance as the persistent
hand. **Automatic fitting is the default, with saved overrides for specific
items that need a better grip** (user-selected direction). Keep the psi amp's
integrated hand/forearm as an explicit presentation exception. Include one thigh
weapon holster by default; Pack-Rat unlocks a second on the opposite leg.

The core fitting set is a coffee mug, magazine, basketball, wrench, pistol, and
shotgun. Together they exercise handles, thin objects, broad spherical contact,
melee, trigger grips, and support grips. Include both a printed magazine and an
ammo magazine/clip to cover the two interpretations. Add the fusion cannon and
worm launcher as oversized-weapon stress cases. Keep card, consumable, and psi
fixtures for the corresponding gameplay transitions.

## What we already have

- [Glove renderer](../shock2vr/src/hand_glove.rs): a rigged SteamVR glove,
  currently recolored as skin and paired with a sleeve. It already supports
  per-finger blending, but held miscellaneous items all get the same fixed grip.
- [Pose library](../shock2vr/src/hand_pose_library.rs): extracted 25AE hands have
  no finger joints, so the library snaps between meshes. Its comments document
  wrist estimation drift between open and curled poses. This library is used
  by the pose debug scene; ordinary VR rendering currently uses the glove path.
- [VirtualHand](../shock2vr/src/virtual_hand.rs): hides the glove when wielding
  a weapon, allowing the weapon's baked hand to replace it.
- [VR configuration](../shock2vr/src/vr_config.rs): model offsets, handedness
  conversions, and runtime melee grip corrections already exist. Extend this
  arrangement into data instead of adding a competing offset system.
- [Model importer](../dark/src/importers/model_importer.rs): already separates
  arm/weapon geometry for measurements. Its warning about a pistol's sleeve
  and hand being separate islands makes “delete all but the largest island”
  unsuitable for removing authored hands.
- [Explorer](../tools/dark_explorer/src/ui.rs) has Files, Archetypes, Archives,
  and a [game-rendered model preview](../tools/dark_explorer/src/model_preview.rs).
  It is a natural home for a VR Grips tab.

## Can an algorithm fit arbitrary items?

Yes, for **contact fitting**. Choosing the intended grasp is a separate problem.
Geometry can suggest where fingers stop; it does not reliably identify a gun
trigger, a safe blade handle, or which face of a card must remain visible.

Commercial tools combine these approaches. HurricaneVR documents multiple
position/orientation-selected grab points, authored poses, per-finger animation,
and automatic posing for generating poses. Auto Hand documents a palm origin,
finger definitions, and fingertip radii for automatic grabbing.
[HurricaneVR posing](https://cloudwalker2020.github.io/HurricaneVR-Docs/manual/hand_posing.html),
[Auto Hand anatomy](https://earnest-robot.gitbook.io/auto-hand-docs/auto-hand/hand).
These are useful design precedents; adopting their Unity components directly
would not supply a Rust/OpenGL implementation.

Proposed solver, rather than a claim about their internal algorithms:

1. Resolve the pose-family hint and any more specific overrides described below.
   Apply an anchor override when available. Otherwise seed a palm location
   from the player's approach and nearby object surface; keep alternative
   orientations limited and prefer minimal movement from the tracked hand.
2. Use the hinted pose family when specified; otherwise choose automatically
   from cylindrical grip, pinch, broad grasp, or trigger grip.
3. Curl each finger along its open-to-grip trajectory. Check several points or
   capsules along the finger against an object-local contact proxy; stop before
   penetration and refine the contact fraction. A fingertip-only check can
   leave knuckles inside the object.
4. Reject badly intersecting palms or unstable fits. Keep a predictable generic
   grasp if no acceptable fit exists. Prototype the solver in the editor, then
   use it automatically at runtime; cache fits at acquisition rather than
   solving continuously every frame. Automatic fitting supplies every value
   not constrained by an override.

### Override hierarchy: hint first, specify only what needs correction

| Level | What the author supplies | What stays automatic |
| --- | --- | --- |
| 0. Automatic | Nothing | Pose family, anchor, and finger contact. |
| 1. Pose-family hint | Cylindrical, pinch, broad grasp, or trigger | Anchor selection and fitting within that family. |
| 2. Grip region / anchor | Allowed handle region or exact palm position/orientation | Finger contact, using the hinted or automatically selected family. |
| 3. Finger corrections | Selected finger curls, thumb opposition, or bone overrides | All unconstrained fingers and placement components. |
| 4. Full authored grip | Exact anchor and complete finger pose | Only explicitly enabled input animation, such as trigger-finger movement. |

These are increasing levels of specificity, not mandatory authoring steps.
Fields compose: a cylindrical hint can stand alone, or combine with a handle
region and a thumb correction. More specific values take precedence only for
the components they constrain; the solver must not overwrite them. Keep
unconstrained components automatic rather than baking every generated value
into the profile when saving a hint.

For example, a card might need only `pose_family: pinch`; a wrench might add a
handle region to `cylindrical`; a pistol might combine `trigger`, an exact grip
anchor, and an index-finger correction. Store pose families as readable named
values in the profile. The Explorer should initially expose this simple family
selector, with anchor and finger controls available when finer correction is
needed. If a constrained fit fails, flag it for review rather than silently
ignoring the hint or corrections.

Start with five independent curl values and a thumb opposition control.
Open/point/fist blends alone have limited grasp coverage; add reusable pinch and
handle poses, with optional bone overrides for important exceptions. Use simple
grip proxies around handles: a whole-object box or convex hull can erase the
very recess a hand needs to fit. Full physics-driven fingers are a later option,
not a prerequisite for this contact solver.

## Calibration and the VR Grips editor

Separate controller-to-palm calibration from object-to-grip alignment. A
controller correction should fix every item; an item correction should affect
only that item. Keep units and coordinate conventions explicit at the boundary.

The VR Grips tab should display the actual runtime glove and item together and
save a versioned, reviewable profile containing:

| Data | Purpose |
| --- | --- |
| Model asset key, optional archetype override | Shared visual fitting with exceptions for how an item is used; never a runtime entity ID. |
| Primary and optional support anchor | Object-local position/orientation and eligible hand; allow multiple candidate grips. |
| Optional pose-family hint | Cylindrical, pinch, broad grasp, or trigger; omitted means automatic selection. |
| Optional finger corrections | Thumb/index/middle/ring/pinky blending, bone overrides, trigger animation mask; unspecified values remain automatic. |
| Grip region / snap range | Where acquisition is allowed; optional sliding interval for long handles. |
| Support-grab policy | Disabled, fixed socket, handle region, or broad surface; optional excluded surfaces and minimum separation from the primary grip. |
| Hand visibility policy | Glove normally; authored integrated arm for the psi amp. |
| Contact proxy and provenance | Reproducible auto-fit input; distinguish generated suggestions from approved fitting. |

Provide transform gizmos, finger sliders, left/right previews, authored-hand
ghost overlay, auto-fit, reset, and save/reload into the runtime. Preview an
approach-to-grab blend, not just the final screenshot. Mirror the hand pose with
an explicit coordinate conversion; inspect asymmetric objects rather than
assuming their geometry or interactions can be mirrored safely.

For weapon defaults, measure the baked hand or available wrist/arm frame to
seed automatic fitting. Do not trust its bounding-box endpoint as an exact
wrist: the existing library already documents that failure. Saved human
corrections take precedence when the automatic result needs improvement.

Remove authored arm/hand surfaces only in the VR render variant, using verified
material/mesh classification. Preserve weapon skeletons, animated parts, muzzle
markers, and melee contact geometry. Inspect fire/reload poses for detached
magazines or moving parts whose authored hands previously explained their motion.
The psi amp replaces that hand's glove without also drawing a duplicate watch
or sleeve; health remains accessible on the opposite wrist.

## Feedback: glove lights and a watch

Use a small wrist-mounted health number and bar, attached to a stable wrist
anchor. Keep health separate from interaction colors. Reuse shared UI layout and
health data; render the same watch canvas in flat debug and VR presentations.

Proposed interaction language:

| Light | Meaning | Reinforcement |
| --- | --- | --- |
| Off | No actionable target | None |
| Green, steady | Current input can perform the selected action now | Target indication and short haptic on acquisition |
| Amber, slow pulse | Recognized target, unmet condition | Brief reason: out of reach, missing credential, incompatible ammo |
| Red, brief pulse | Attempt failed | Rejection sound/haptic; return to the current hover state |

Lights must consume the same resolved target, action, and eligibility as input.
Do not run an independent lighting raycast. Define precedence between active
holds, near interactions, belt/backpack zones, world pointing, and modal UI;
retain a target briefly across small boundary movements to prevent flicker.
Pair color with pattern, sound, or haptics so color alone never carries the rule.

Add an emission-mask texture sampled independently of albedo:
`emission = mask * hand_state_color * intensity`. The existing skinned material
only adds albedo times a scalar emissivity; it needs a mask and per-hand tint.
Keep per-hand values isolated so one glove cannot overwrite the other's state.
Separate mask channels are useful only if different light regions need different
meanings. Verify readability in bright/dark rooms without relying on bloom.

## Body interactions

**Belt card:** a persistent, returning representation of the player's collected
credentials. Swipe a reader to call the same access check as ordinary door use.
Keep implicit credential unlocking available; the card neither occupies an
inventory slot nor becomes a losable quest item. Collecting a new key updates
the credential set rather than creating another belt object.

**Ammo pouch:** reach with the free hand to withdraw compatible ammo for the
other hand's current weapon, then use the existing clip-to-gun reload gesture.
Withdraw from real inventory stock atomically; do not fabricate an infinite
clip. A visible selected ammo type resolves alternatives. Define dual-wield
behavior explicitly: two occupied hands cannot withdraw another object.

**Thigh holsters:** one weapon slot on the preferred leg; the Pack-Rat O/S trait
unlocks a second slot on the opposite leg. This is an additional proposed VR
benefit: [Pack-Rat already increases backpack capacity](../shock2vr/src/inventory/mod.rs),
and that effect remains. Update the trait description to explain both benefits.
Holsters are dedicated carried slots outside the backpack grid; moving a weapon
between hand, pack, and holster transfers its location without creating another
weapon or leaving an invisible inventory duplicate.

Bring a held weapon to an empty holster, receive a docking preview/haptic, then
release to stow. Grip the holstered weapon to draw it into an empty hand. Occupied
slots reject docking without silently swapping or dropping their contents. Keep
the first implementation weapon-only and exclude the integrated psi amp until
its equip/unequip behavior has a deliberate design. Profile data can optionally
specify the holstered orientation and draw anchor for awkwardly shaped weapons.

With head/controller tracking alone, these are hip-relative side slots that
visually read as thigh holsters, not accurately tracked knees or legs. Make
height, side, and lateral offset adjustable, including a seated preset; keep
their zones distinct from the ammo pouch and away from ordinary arm swings.
Save slot occupancy and preserve weapon ammo/condition through stow/draw and
level transitions. If a trait reset removes the second slot, relocate its weapon
to available storage or defer disabling that occupied slot until it is emptied;
never delete its contents.

**Backpack:** entering the shoulder zone with a storable item previews acceptance
and gives a haptic; releasing commits the inventory transfer. Full inventory
rejects before destroying/removing anything and gives a visible explanation.
An explicit rejected-stow state can keep the item attached until the hand leaves
the zone and the player deliberately drops or retries. Ordinary release outside
the zone still drops. Test this exception for clarity with real users.

Anchor the belt using estimated torso yaw and calibrated height, not full head
rotation. Head tilt must not tilt the belt. Keep acquisition zones stable during
a gesture; tune standing/seated placement and handedness. Behind-shoulder tracking
loss must not look like a release: commit only on valid input, and reset transient
gesture states on tracking recovery, scene changes, and menu transitions.

**Two-handed melee:** the main hand owns the item; the support hand grips a
secondary socket, handle segment, or allowed surface without acquiring a second
inventory owner.
**Agreed two-hand orientation:** align the weapon-local vector from the primary
grip anchor to the support anchor with the tracked vector from the primary palm
to the supporting palm. Keep the primary anchor at the primary palm and use
the primary hand's orientation to resolve the remaining twist about that vector.
This preserves the relationship between the barrel and the actual grip points:
grabbing the side of a weapon must not aim its barrel along the hand-to-hand line.
Use the same solver for fixed sockets and arbitrary surface support anchors.

Keep weapon scale fixed; different real hand spacing cannot generally place
both rigid anchors exactly at both tracked palms. Preserve the primary anchor
and handle support-hand distance mismatch with a bounded visual adjustment or
an explicitly sliding grip region, releasing support beyond a tuned tolerance.
The exact tolerance remains a headset-tuning decision.
Smooth support attach/detach and handle nearly coincident hands safely, retaining
a stable prior orientation when the direction is undefined. Include off-axis
support points and a nearly opposite direction change in solver verification.
Keep collider, rendered weapon, and damage trace on the same solved transform.
Decide primary-hand release explicitly: initially release support too and drop,
then consider deliberate hand transfer.

**Oversized weapons:** use the fusion cannon and worm launcher to test a broad
support-grab policy: any reachable, permitted part of the weapon body can accept
the second hand. Find a nearby surface from the supporting palm, fit the fingers
there, and latch the resulting anchor in weapon-local space until release.
Do not pick a new nearest point every frame, which could make the hand skate
around the surface. Sliding is an explicit policy for designated regions.
Keep the primary grip authoritative for ownership and firing; the second hand
supports orientation and cannot fire or duplicate the weapon independently.

Broad support regions should be generated from the weapon geometry by default,
with overrides able to exclude muzzle openings, moving parts, or surfaces that
produce implausible support. Preview the eligible region before acquisition.
Require enough separation between hands for a stable orientation solve; nearby
or coincident grips must not cause flips. A basketball also tests broad surface
support without implying that two hands can physically wrap around its entire
circumference: visible palm/finger contact is sufficient.

Preserve intentional weapon size initially. First inspect real-world scale and
available support surfaces on Quest, then correct an asset's scale only if the
viewmodel exaggeration is demonstrably unsuitable. Support grabbing improves
handling but cannot by itself fix clipping into the torso, excessive apparent
weight, or a weapon that does not fit a thigh holster. Record holster eligibility
for these large models explicitly rather than shrinking every weapon to fit.

**Melee balance experiment:** for weapons designated as benefiting from two-hand
support, keep authored damage at a multiplier of `1.0` while supported and apply
a tunable multiplier below `1.0` when used one-handed. This implements the user's
suggestion as a one-handed penalty; the exact multiplier and eligible weapons
remain to be tested. Do not penalize every melee item automatically, and do not
apply this rule to ranged damage just because a gun has a support grip.

Derive the modifier from a valid support attachment, not proximity of the second
controller. Apply it once in the existing melee damage path, preserving stats,
skills, and other modifiers. Latch support eligibility at the start of a swing
and clear it if support is released before impact: briefly grabbing just before
contact must not upgrade an otherwise one-handed swing. Invalid tracking cannot
create a new support attachment or damage benefit. Verify repeated contacts do
not deal duplicate damage. Keep this contextual VR modifier separate from flat
combat, whose input does not express support-hand attachment. Expose both damage
states in the test scene so the penalty can be judged against actual combat feel.

**Melee perk extension:** consider removing the one-handed penalty through an
existing melee O/S trait. The current trait list includes **Smasher** and
**Lethal Weapon**; choose one rather than granting the same exemption through
both. Smasher is a candidate alongside the proposed overhead/charged attack;
Lethal Weapon is the alternative for a more general melee damage specialization.
Keep the trait's other intended benefit, including charge-up if Smasher is chosen.
This is a proposed extension, not a claim that those attack effects are already
implemented. Final trait selection remains open.

Model the exemption as restoring the grip multiplier to `1.0`, not adding a
second damage bonus: eligible weapon + one hand + no exempting trait uses the
penalty; either valid two-hand support or the selected trait uses `1.0`.
Existing damage modifiers and charged-attack rules then compose normally.
The trait does not pretend a second hand is attached or grant physical support
benefits such as aim stabilization. It also does not waive charge-up conditions
for a charged attack. Expose trait on/off in `debug_interactions` and compare
one-/two-handed ordinary and charged swings. Update the trait description when
the choice is made so the VR benefit is discoverable.

## Incremental delivery and acceptance

### Before implementation

Working branch: `astra-vr-interactions-round-2`. Keep subsequent implementation
branches under the same `astra-` prefix, with independently reviewable changes.

- Capture the current glove and representative held weapons before altering
  them, including neutral controller alignment on Quest. Record the source
  commit and capture sequence for reproducible comparisons.
- Audit the core rack assets: exact model/template identities, dimensions,
  available baked hands, removable arm surfaces, and usable contact geometry.
  Treat Fable's asset table as leads to verify, not an assumption about every
  model. Use coarse bounds for candidate queries, then surface geometry or
  dedicated proxies for actual finger contact.
- Define the controller/palm/model coordinate contract and profile precedence
  before adding offsets. For cached fits include hand, scale, profile revision,
  and grip anchor as applicable; a model-only cache cannot represent arbitrary
  support locations. Keep contact fitting bounded and measure its acquisition
  cost on Quest before increasing solver complexity.
- Establish a small baseline check set around held-weapon transforms, physical
  reload, melee contacts, and shared HUD layout. Build the rack and Explorer
  preview early enough to inspect the first grip change, not after fitting has
  already been implemented across many models.
- The glove model and original color texture are present. The emissive mask
  still needs authoring; material/state work can use a temporary diagnostic mask
  while final art is prepared. Do not let that asset block grip work.

No further design decision blocks the first slice. Leave exact damage penalty,
Smasher versus Lethal Weapon, support-release tolerance, and oversized-weapon
holster eligibility to focused trials. Profile naming/format and numerical
solver details are routine implementation decisions within this direction.

### Delivery slices

1. **Glove baseline:** restore glove appearance, calibrate wrist/palm, add watch
   and masked feedback. Verify neutral, point, and fist at real controller poses.
2. **Automatic fitting and overrides:** build `debug_interactions` and the matching
   SS2 Explorer VR Grips previews first, then use them to develop profile loading,
   the bounded contact solver, and selective baked-hand removal. Keep the full
   representative rack available; begin fitting with the six core item types,
   then the two oversized weapons. Save overrides where needed and include the
   psi exception.
3. **Body inventory:** card, ammo pouch, shoulder stow, then thigh holster and
   Pack-Rat second slot as separate changes. Verify rejected operations, quantity
   conservation, save/load, trait changes, and input ownership.
4. **Support grip:** two-handed wrench first; test attachment, release, tracking
   loss, and melee contacts, then broad surface support on the basketball,
   fusion cannon, and worm launcher. Follow with a separate, tunable one-handed
   melee penalty experiment and the optional Smasher/Lethal Weapon exemption.
5. **Long-tail fitting:** expand the item corpus, improve automatic grasp seeds
   and proxies, and author only the overrides needed to correct poor results.

## Shared interaction testbed: `debug_interactions` and SS2 Explorer

Add a dedicated `debug_interactions` scene with a labeled, reachable rack of
representative items, all present together. The scene is a repeatable workbench
for holding, inspecting, releasing, and reacquiring objects with either hand.
Reset restores the rack and item state so consumed, thrown, or stowed items do
not disappear from the test set. Use real game templates and assets, discovered
by stable template/model identity; confirm exact variants through `dark_query`
when implementing the fixture.

| Representative item | Coverage |
| --- | --- |
| Coffee mug — core | Handle versus body grasp, concavity, thumb clearance; a coarse hull must not fill the handle hole. |
| Printed magazine — core | Thin, broad object; pinch/edge grasp and readable front face. |
| Basketball — core | Large sphere, broad contact, two-hand surface support without full finger enclosure. |
| Pistol | Trigger grip, asymmetric geometry, aim alignment, index animation. |
| Shotgun | Larger weapon, support-hand placement, long-object clearance. |
| Wrench | Cylindrical handle, sliding support region, two-handed melee. |
| Fusion cannon — oversized | Broad support region, reach, torso clearance, scale, and holster eligibility. |
| Worm launcher — oversized | Irregular large body, automatically fitted support contact, and excluded surfaces. |
| Crystal shard | Irregular melee geometry; distinguish handle from dangerous end. |
| Access card | Thin pinch grip, visible card face, reader presentation. |
| Compatible pistol ammo clip | Small box grasp and clip-to-gun insertion. |
| Med hypo | Narrow grasp, thumb placement, explicit use transition. |
| Grenade | Rounded grasp, release and throw. |
| Portable battery | Broad grasp and palm penetration checks. |
| One asymmetric junk prop | Unhinted automatic fitting; choose an actual carryable model after asset inspection. |
| Psi amp | Authored integrated-hand exception and opposite-wrist health visibility. |

Include a safe inspection area, a melee target, and a card-reader fixture.
Add body-slot previews and inventory state readouts as pouch, backpack, and
holster work lands. The card fixture must exercise the credential-backed belt
representation and collection behavior, not turn a key pickup into an ordinary
inventory object just to make it grabbable. Test controls may restore ammo,
consumables, credentials, and Pack-Rat state without changing shipping behavior.

For every rack model, SS2 Explorer's VR Grips view should show the item with
the actual posed glove, left/right selection, approach-to-hold preview, optional
authored-hand ghost, primary/support anchors, and contact-proxy overlays. Show
which fields came from automatic fitting, a family hint, or a specific override.
Provide automatic-only versus resolved-profile comparison, so an override's
effect is immediately visible. Keep these diagnostics in development tools.

The scene and Explorer must use the same grip resolver, glove renderer, model
variant, and profile loader. The scene's manifest should link each rack entry to
its Explorer model/profile identity, and saving a profile should allow reloading
it in the running scene. Avoid a separate approximation of VR placement in the
Explorer that could look correct while gameplay is misaligned.

Expected harness entry point: `cargo dbgr --mission debug_interactions --vr`.
Also support loading it in flat presentation for scene and shared-UI checks;
the VR-mode harness exercises two-hand input. Make stable rack identities and
resolved grip state inspectable through the debug API so SDK scenarios can
replay grab/release, override reload, support attachment, and inventory transfers.
Capture both an item close-up and the approach/grab/release sequence. Verify
the same scene on Quest to assess tracked alignment and physical reach.

For each increment, capture deterministic before/after media in the appropriate
debug scenes and verify shared UI in flat and VR. Then test a fresh Quest build:
controller fit, readable watch, comfortable reach, tracking recovery, and actual
frame cost. The debug harness can establish geometry and state transitions;
comfort and convincing alignment require wearing the headset.

Acceptance should include both hands, representative grip styles, transitions
into/out of holding, and repeated pouch/stow operations without loss or duplication.
Record grab success, accidental actions, correction effort, visible penetration,
and device timing against the baseline. Set final reach distances and blend times
from these trials rather than hard-coding guesses into the design.
