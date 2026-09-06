# Astra prepared pickup grips

Prepared grips cover the coffee mug, printed magazine (`magci`), basketball
(`hamball`), four hypos, maintenance tool, French-Epstein device, auto-repair
unit, and an inert access-card sample in `debug_interactions`, for both hands.
The eleven unique models produce 22 prepared grips. This preserves
the headset-approved glove scale and wrist/palm calibration. Weapons, including
their authored hands, continue through the existing weapon path.

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

The script launches `debug_interactions --vr`, discovers the eleven fitting fixtures by
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
full authored bone overrides, and the SS2 Explorer editing UI are subsequent
increments. The basketball remains a one-hand support pose until support grips
are implemented.
