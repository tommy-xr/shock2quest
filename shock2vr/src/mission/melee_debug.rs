//! World-space wireframe overlay of the melee contact volume (WIP).
//!
//! The owner asked to be able to *see* where a VR melee weapon's damage volume
//! actually is while swinging, because melee damage that a headless harness
//! measures reliably was not landing in practice. Drawing it in world space
//! means it renders identically in flat and in VR (it is an ordinary
//! `SceneObject` in the shared world pass), which is the only way to inspect
//! it in the presentation where the problem was reported.
//!
//! Colors:
//! - **red**   - the held weapon's live contact volume (what actually deals
//!   melee damage on contact).
//!
//! Toggle it at runtime with the `DebugToggleMeleeVolumes` input action - see
//! [`crate::debug_toggles`] for why this is not a dev param yet.
//!
//! WIP / not yet drawn (see the branch notes): the per-frame *swept* path of
//! the contact volume, and the victim-side damageable volumes.

use cgmath::{Matrix4, Vector3};
use dark::hit_box::{HitBoxShape, draw_debug_hit_box_shapes};
use engine::scene::SceneObject;
use shipyard::EntityId;

use crate::physics::{DebugColliderShape, DebugColliderVolume, PhysicsWorld};

/// Red: the live contact volume that deals melee damage.
const CONTACT_COLOR: Vector3<f32> = Vector3::new(1.0, 0.15, 0.1);

/// A ball is exactly a capsule with coincident endpoints, but the shared
/// capsule wireframe builder bails on a zero-length segment. Separating the
/// endpoints by this much renders a sphere to within a tenth of a millimetre
/// while reusing that builder rather than growing a parallel one.
const BALL_SEGMENT_EPSILON: f32 = 1.0e-4;

/// Wireframes for the contact volumes of the entities held in the player's
/// hands. Empty when nothing is held or the held body has no drawable
/// primitive collider.
pub fn draw_melee_contact_volumes(
    physics: &PhysicsWorld,
    held: (Option<EntityId>, Option<EntityId>),
) -> Vec<SceneObject> {
    let (left, right) = held;
    let volumes: Vec<DebugColliderVolume> = [left, right]
        .into_iter()
        .flatten()
        .flat_map(|entity_id| physics.debug_entity_collider_volumes(entity_id))
        .collect();

    draw_volumes(&volumes, CONTACT_COLOR)
}

/// Convert world-space collider volumes into a single wireframe object,
/// reusing the hitbox overlay's line builders (`dark::hit_box`) rather than
/// growing a second debug-draw system.
fn draw_volumes(volumes: &[DebugColliderVolume], color: Vector3<f32>) -> Vec<SceneObject> {
    let mut shapes = std::collections::HashMap::new();
    let mut transforms = Vec::new();

    for volume in volumes {
        let Some(shape) = to_hit_box_shape(volume.shape) else {
            continue;
        };
        // `draw_debug_hit_box_shapes` indexes its transform list by joint id,
        // so give each volume its own slot.
        shapes.insert(transforms.len() as u32, shape);
        transforms
            .push(Matrix4::from_translation(volume.position) * Matrix4::from(volume.rotation));
    }

    draw_debug_hit_box_shapes(&shapes, &transforms, color)
}

fn to_hit_box_shape(shape: DebugColliderShape) -> Option<HitBoxShape> {
    match shape {
        DebugColliderShape::Ball { radius } => Some(HitBoxShape::Capsule {
            a: Vector3::new(0.0, -BALL_SEGMENT_EPSILON, 0.0),
            b: Vector3::new(0.0, BALL_SEGMENT_EPSILON, 0.0),
            radius,
        }),
        DebugColliderShape::Capsule { a, b, radius } => Some(HitBoxShape::Capsule { a, b, radius }),
        DebugColliderShape::Cuboid { half_extents } => Some(HitBoxShape::Cuboid {
            half_extents,
            center: Vector3::new(0.0, 0.0, 0.0),
        }),
        DebugColliderShape::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ball_draws_as_a_non_degenerate_capsule() {
        // Negative case for the builder's zero-length bail: a ball mapped to a
        // capsule with coincident endpoints would draw nothing at all, which
        // is exactly the failure this overlay exists to avoid.
        let Some(HitBoxShape::Capsule { a, b, radius }) =
            to_hit_box_shape(DebugColliderShape::Ball { radius: 0.046 })
        else {
            panic!("a ball must map to a drawable capsule");
        };
        assert!((b - a).y > 1e-5, "endpoints must not be coincident");
        assert_eq!(radius, 0.046);
    }

    #[test]
    fn unsupported_shapes_are_skipped_rather_than_approximated() {
        assert!(to_hit_box_shape(DebugColliderShape::Other).is_none());
        assert!(
            draw_volumes(
                &[DebugColliderVolume {
                    shape: DebugColliderShape::Other,
                    position: Vector3::new(0.0, 0.0, 0.0),
                    rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    is_sensor: false,
                }],
                CONTACT_COLOR
            )
            .is_empty()
        );
    }
}
