use cgmath::{Matrix4, Vector3};
use dark::properties::PropPosition;
use shipyard::{Get, IntoIter, IntoWithId, View, ViewMut};

use crate::runtime_props::{RuntimePropAttachment, RuntimePropTransform};
use crate::util::get_rotation_from_matrix;

///
/// run_attachment_update
///
/// For every entity bolted to a parent (`RuntimePropAttachment`), recompute its
/// transform as `parent.transform * local_transform`, so it tracks the parent
/// each frame (e.g. a muzzle flash following the first-person weapon viewmodel).
/// Updates both `RuntimePropTransform` (the render transform) and `PropPosition`
/// (the logical position), mirroring `synchronize_physics_positions`, so the
/// rendered pose and the reported position stay consistent. Entities whose
/// parent has gone away keep their last transform.
pub fn run_attachment_update(
    v_attach: View<RuntimePropAttachment>,
    mut v_transform: ViewMut<RuntimePropTransform>,
    mut v_position: ViewMut<PropPosition>,
) {
    // Read parents first (the immutable borrow ends with the statement), then
    // write children, to avoid aliasing the transform view.
    let updates = (&v_attach)
        .iter()
        .with_id()
        .filter_map(|(id, attach)| {
            (&v_transform)
                .get(attach.parent)
                .ok()
                .map(|parent_xform| (id, parent_xform.0 * attach.local_transform))
        })
        .collect::<Vec<_>>();

    for (id, xform) in updates {
        if let Ok(transform) = (&mut v_transform).get(id) {
            transform.0 = xform;
        }
        // Keep the logical position in sync with the render transform, when the
        // entity carries one (real spawned entities always do; some test
        // entities may not).
        if let Ok(position) = (&mut v_position).get(id) {
            position.position = translation_of(&xform);
            position.rotation = get_rotation_from_matrix(&xform);
        }
    }
}

/// The translation component (4th column) of a transform matrix.
fn translation_of(m: &Matrix4<f32>) -> Vector3<f32> {
    Vector3::new(m.w.x, m.w.y, m.w.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Matrix4, SquareMatrix, Vector3};
    use shipyard::World;

    fn translation(x: f32, y: f32, z: f32) -> Matrix4<f32> {
        Matrix4::from_translation(Vector3::new(x, y, z))
    }

    #[test]
    fn child_tracks_parent_after_parent_moves() {
        let mut world = World::new();

        // Parent at the origin; child one unit ahead of it (local offset +1 z).
        let parent = world.add_entity((RuntimePropTransform(Matrix4::identity()),));
        let local = translation(0.0, 0.0, 1.0);
        let child = world.add_entity((
            RuntimePropTransform(local),
            // PropPosition so we also exercise the logical-position sync (this is
            // what the debug runtime's /v1/entities endpoint reports).
            PropPosition {
                position: Vector3::new(0.0, 0.0, 1.0),
                cell: 0,
                rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            RuntimePropAttachment {
                parent,
                local_transform: local,
            },
        ));

        // Move the parent. Without the system the child stays put (negative case);
        // running the system re-anchors the child to parent * local.
        world.run(|mut v_transform: ViewMut<RuntimePropTransform>| {
            (&mut v_transform).get(parent).unwrap().0 = translation(10.0, 0.0, 0.0);
        });

        // Negative check: child has not moved yet.
        world.run(|v_transform: View<RuntimePropTransform>| {
            let c = v_transform.get(child).unwrap().0;
            assert_eq!(c, local, "child should not move until the system runs");
        });

        world.run(run_attachment_update);

        // Child should now be at parent (10,0,0) composed with local (+1 z) - in
        // both the render transform and the reported logical position.
        world.run(
            |v_transform: View<RuntimePropTransform>, v_position: View<PropPosition>| {
                let c = v_transform.get(child).unwrap().0;
                let expected = translation(10.0, 0.0, 0.0) * local;
                assert_eq!(c, expected, "child should follow the moved parent");

                let pos = v_position.get(child).unwrap().position;
                assert_eq!(
                    pos,
                    Vector3::new(10.0, 0.0, 1.0),
                    "logical position should track the parent too"
                );
            },
        );
    }

    #[test]
    fn child_keeps_transform_when_parent_missing() {
        let mut world = World::new();

        let local = translation(0.0, 1.0, 0.0);
        // Parent id that has no transform (e.g. a destroyed weapon).
        let orphan_parent = world.add_entity(());
        let child = world.add_entity((
            RuntimePropTransform(local),
            RuntimePropAttachment {
                parent: orphan_parent,
                local_transform: local,
            },
        ));

        world.run(run_attachment_update);

        world.run(|v_transform: View<RuntimePropTransform>| {
            let c = v_transform.get(child).unwrap().0;
            assert_eq!(c, local, "child keeps its last transform without a parent");
        });
    }
}
