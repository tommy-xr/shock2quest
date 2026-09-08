# Astra prepared pickup grips

Prepared grips cover the coffee mug, printed magazine (`magci`), basketball
(`hamball`), four hypos, maintenance tool, French-Epstein device, auto-repair
unit, an inert access-card sample, implants, worm beakers, GamePig, and ICE-Pick in `debug_interactions`, for both hands.
The 23 unique models produce 46 prepared grips. This preserves
the headset-approved glove scale and wrist/palm calibration. Fourteen weapon
models now have a separate prepared library, described below; the psi amp keeps
its integrated authored forearm.

Fitting is an **offline bake**, not a gameplay-frame search. The debug build's
candidate searches took seconds; loading prepared results took roughly 0.3–1.5 ms
on the development Mac. Those are host debug measurements, not Quest frame-budget
claims. A held item's placement and finger curls remain fixed relative to its
tracked hand until release. Trigger input does not collapse a prepared pickup
pose into a fist; the item's normal trigger action still runs.

## Bake and inspect

Build `debug_runtime` and the SDK, then use Node 22+:

```sh
cargo build -p debug_runtime
cd tools/shock2-sdk
npm run build
node scripts/bake-vr-grips.mjs
```

The script launches `debug_interactions --vr`, discovers a representative for each of the 23 fitting models by
stable template ID, grabs each with each hand, writes
`assets/astra-vr-grips.json`, and shuts down its runtime. An optional first
argument selects an output path. It enables `--experimental astra-bake-vr-grips`:
**that authoring mode intentionally blocks stepping while it searches**. Normal
gameplay never enables the search or silently rebakes a missing entry.

Launch normally to review the generated resource:

```sh
cargo dbgr --mission debug_interactions --vr --experimental astra-grip-overlay
```

`GET /v1/info` → `player.hand_grips` reports each active hand's model, source
(`prepared`, `bake`, or `missing_or_stale`), lookup/bake time, palm and palm normal,
item-local anchor, hand-local item offset/rotation, pose family, five curls, and
item-local contact points. Cyan overlay cubes mark those same resolved contacts.
A null contact means that finger could not reach a surface; its curl minimizes
remaining fingertip distance rather than closing fully through empty space.

Use `/v1/control/input` or SDK `game.input.set` to move and rotate the controller,
then `/v1/camera` to inspect both sides of the hand. The existing rack regression
now checks prepared lookup, fixed grips during tracked motion, the resulting
world-space item position, and clearing on release for both hands.

## Inputs and overrides

`assets/astra-vr-grip-hints.json` contains optional inputs keyed by model name.
The stable integer `pose_family` is 0 cylindrical, 1 pinch, 2 broad grasp,
3 trigger. Omission selects a family from the mesh dimensions. Families adjust candidate ranking: pinch prioritizes thumb/index contact,
cylindrical favors even finger closure, broad favors open support, and trigger
favors a less curled index. They share the glove's open-to-fist arcs; additional
thumb opposition and family-specific pose endpoints remain future work.

Each field is optional; unspecified fingers continue to fit automatically:

```json
{
  "example_model": {
    "pose_family": 0,
    "keep_upright": true,
    "anchor": [0.08, 0.0, 0.0],
    "rotation": [1.0, 0.0, 0.0, 0.0],
    "curls": [null, 0.35, null, null, null]
  }
}
```

`anchor_region` is an optional pair of inclusive item-local minimum/maximum
bounds, in game world units. It restricts automatic surface candidates; the
magazine uses this to avoid its corners, and the auto-repair unit restricts
candidates to its carry handle. An explicit `anchor` can replace the
selected contact after candidate selection. The mug overrides only its thumb curl
to leave more clearance.

`anchor` is an item-local palm contact point in game world units. `rotation` is
item-to-hand orientation in **[w, x, y, z]** order (different from the debug input
API's [x, y, z, w]). Changing either reruns automatic finger contact at that
placement during the bake. `keep_upright` keeps the item's Y axis near the hand's
Y axis and restricts candidates to the middle of its height, avoiding rim/base
grips on vessels. These hints are authoring inputs; rebake after editing them.

`upright_axis` optionally names an item-local direction, for example `[0, 0, 1]`
for the ICE-Pick cable. During automatic orientation selection, that direction
must point near the calibrated hand's +Y axis. It does not restrict anchor height
or lock the object to world gravity: the baked pose still follows the controller.
An explicit `rotation` overrides automatic orientation selection. Omitting this
axis preserves the existing solver policy and fingerprints; opting in changes
that model's hint fingerprint and requires rebaking.

Pronged implants constrain contact to the housing; GamePig constrains it to the
middle of a side. These are model-space region hints rather than changes to glove
calibration or asset scale. The gallery remains a review tool. The
[Explorer override editor](astra-vr-grip-editor.md) now supports direct pickup
and weapon pose editing and saves.

The output resource has a versioned schema and one entry per model and hand.
Mesh, glove-kinematics, and hints fingerprints plus a solver revision prevent a
bake from being applied after its inputs change. Fingerprints quantize coordinates to 0.0001 world units to
ignore insignificant floating-point differences. Missing, stale, malformed, or
unsupported entries retain the existing generic grip and report that fallback.

## Resolver boundaries

`dark::importers::GRIP_SURFACE_IMPORTER` reuses the rendered LGMD triangulation
and sub-object transforms. It does not use coarse physics spheres or a convex
hull. `GloveRenderer::grip_kinematics` samples the actual retargeted glove rig and
uses the same model-to-hand transform as drawing. `vr_grip::GripSurface::resolve`
is independent of gameplay and rendering resources, so an Explorer preview or
future bulk baker can call the same resolver.

The search seeds surface contacts with bounded rays, aligns to the actual surface
normal, tries rotations/clearances, and closes each finger along its sampled arc.
It rejects initial interior penetration for watertight meshes and verifies final
finger samples/segments. Open meshes (including the shipped mug) use surface and
segment checks rather than assuming a closed solid. The fixture bake requires at
least three supported fingers before writing results. This remains approximate
contact fitting, not articulated collision physics.
The mug currently uses a body grasp; automatic recognition of a semantic handle,
and full authored bone overrides remain subsequent increments. The basketball remains a one-hand support pose until support grips
are implemented.


## Weapon fitting

`assets/astra-vr-weapon-grips.json` stores 28 poses: both hands for `atek_h`,
`ar15_h`, `sg_h`, `lasehand`, `empgun_h`, `gren_h`, `sfg_h`, `fsn_h`, `al_h`,
`viro_h`, `wrench_h`, `rapier_h`, `shard_h`, and `psword_h`. The psi amp is excluded.
To regenerate the weapon library, with the same runtime/SDK prerequisites:

```sh
node scripts/bake-vr-grips.mjs --weapons
node scripts/astra-grip-gallery.mjs --weapons --output /tmp/astra-weapon-gallery
```

An optional output path after `--weapons` writes a separate file. Authored edits
are preserved under the same fingerprint/concurrent-edit safeguards as pickups.
Weapon candidates with weak finger contact remain reviewable drafts; unlike the
pickup bake, they do not require three contacts to write. Inspect their images.

The dedicated importer separates arm materials from weapon materials, preserving
weapon hierarchy and muzzle points. Skinned melee geometry is sampled in the
same idle pose and skinning palette used to render it. A failed remaster import
retains the legacy model and suppresses the extra glove. Flat viewmodels keep
their existing authored hands.

The offline search fits near authored arm geometry or the posed melee fist,
trying several uniform weapon scales. Typical candidates include 0.7, the old
melee default. It starts from a unit-scale melee frame so this is applied once;
the calibrated glove stays the same size. Some guns have no useful authored hand,
and the shotgun's authored hand is on the fore-end. Explicit overrides provide
the intended primary grip in these cases. This is approximate contact fitting,
not semantic recognition of every handle.

Normal gameplay only loads prepared results. Held melee colliders, contact
origins, and rendered meshes use the same scale; dropping restores world size.
Scaled guns keep scaled muzzle positions but unit projectile direction/speed.
`player.hand_grips[].item_bounds` reports the fitted weapon bounds in controller
space for repeatable gallery framing. Mesh fingerprints include the transformed
weapon and arm guide, separately for each hand.


### Initial weapon pose review

These are editable starting poses, with visual refinement still in progress.
The wrench is the strongest automatic result; the other melee models also use
plausible basal handle grasps. Gun primary-grip locations needed manual correction
because several authored meshes supply a support hand. Most guns now use 0.55
uniform scale; the wrench remains 0.7. Every shipped model has the same scale in
both hands, though its offsets and curls can be edited separately.

Close-up review still finds finger/handle intersections or gaps on several guns,
particularly the grenade launcher and bulky/organic weapons. The stasis model
also contains remote geometry that inflates its full-object framing; use the
contact close-ups to inspect its hand. Successful prepared lookup and passing
interaction tests are not visual approval. Keep this slice in draft until the
remaining contact issues and headset feel have been reviewed.
