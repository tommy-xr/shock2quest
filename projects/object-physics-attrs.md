# Object Physics Attributes → Rapier: remaining work

`PropPhysAttr` elasticity and friction drive dynamic Rapier bodies
(`DynamicPhysicsOptions` in `entity_creator.rs`, applied in `physics::add_dynamic`).
Elasticity is scaled by `ELASTICITY_TO_RESTITUTION = 0.7` so the shipped default
(`1.0`) settles like the old hardcoded restitution. Unit tests:
`higher_restitution_bounces_higher`, `higher_friction_slides_less`.

Only creatures, frob-`MOVE` grab items, and `!immobile && SPHERE` objects are
dynamic; everything else is a kinematic cuboid these attributes don't touch.

## Remaining

- **density → mass.** Authored `density` (default `1.0`) would replace the
  hardcoded `0.1`, roughly 10× the mass of dynamic objects. This changes how
  grabbed/thrown items feel, so it needs an in-headset check. Clamp the
  `density = 1000000.0` "immovable" sentinel so it can't destabilize the solver.
- **center of mass (`cog`)** via `set_center_of_mass`.
- **`rotation_axes` / `rest_axes`** → `set_enabled_rotations` (today rotations
  are only ever fully locked, on creatures).
- **Mobility-based dynamic bodies.** Choose dynamic vs kinematic from a mobility
  flag (`PropImmobile` / Dark's translatable bit) instead of shape, so mobile OBB
  props can tumble. This is what makes the attributes visible on most props.
- **Friction calibration.** Cross-check Dark's friction model; the direct
  mapping may need a scale factor.
