/**
 * `runtime_props.rs`
 *
 * Runtime properties are properties that are convenience properties for running the game.
 *
 * Notably:
 * - They are not part of SS2 / Dark - just convenience properties for implementing the game.
 * - They are not serialized / deserialized
 */
use cgmath::{Matrix4, Point3, Vector3};
use dark::ss2_bin_obj_loader::Vhot;
use shipyard::Component;

// RuntimePropGazeAmount - track how much the player is gazing at a prop
#[derive(Component)]
#[allow(dead_code)]
pub struct RuntimePropGazeAmount(pub f32);

#[derive(Component)]
pub struct RuntimePropTransform(pub Matrix4<f32>);

#[derive(Component)]
pub struct RuntimePropJointTransforms(pub [Matrix4<f32>; 40]);

#[derive(Component)]
pub struct RuntimePropSpawnTimeInSeconds(pub f32);

#[derive(Component)]
pub struct RuntimeBitmapAnimationFrameCount(pub u32);

#[derive(Component, Debug)]
pub struct RuntimePropVhots(pub Vec<Vhot>);

// RuntimePropDoNotSerialize - runtime prop to signal that this prop should not be serialized
#[derive(Component)]
pub struct RuntimePropDoNotSerialize;

// RuntimePropProxyEntity - pointer to the parent entity (for example, hitboxes use this to point to the parent entity)
#[derive(Component)]
pub struct RuntimePropProxyEntity(pub shipyard::EntityId);

// RuntimePropAttachment - rigidly bolts an entity to a parent's transform. Each
// frame the child's RuntimePropTransform is recomputed as
// `parent.transform * local_transform`, so short-lived attached effects (e.g. the
// muzzle flash, and later shell ejection / smoke) track a moving parent - notably
// the flat first-person weapon, which is re-placed against the camera every frame
// - instead of staying pinned at their spawn pose. `local_transform` is captured
// at spawn as `parent.transform.invert() * child.transform`, preserving the
// child's own scale/orientation relative to the parent.
#[derive(Component, Clone, Copy)]
pub struct RuntimePropAttachment {
    pub parent: shipyard::EntityId,
    pub local_transform: Matrix4<f32>,
}

// RuntimePropFlatAim - the flatscreen camera/crosshair fire ray (world space),
// set each frame on the player's wielded weapon. When present, weapon firing
// spawns projectiles from `origin` along `forward` (camera-origin aim) instead
// of the weapon's barrel transform, so shots track the crosshair regardless of
// the viewmodel's framing. Absent on AI/VR weapons, leaving their aim unchanged.
#[derive(Component, Clone, Copy)]
pub struct RuntimePropFlatAim {
    pub origin: Point3<f32>,
    pub forward: Vector3<f32>,
}
