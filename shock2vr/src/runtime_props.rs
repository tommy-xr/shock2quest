/**
 * `runtime_props.rs`
 *
 * Runtime properties are properties that are convenience properties for running the game.
 *
 * Notably:
 * - They are not part of SS2 / Dark - just convenience properties for implementing the game.
 * - They are not serialized / deserialized
 */
use cgmath::Matrix4;
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
