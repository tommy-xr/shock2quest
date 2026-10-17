use super::*;

impl PhysicsWorld {
    /// Find room for a flat inventory throw before detaching the item. A long
    /// prop spawned through a door can wedge against the wall instead of
    /// falling (#2209). Keep the requested pose whenever it fits, otherwise
    /// search at most 1.2 units sideways; never place through a wall from the eye.
    pub(crate) fn clear_item_release_position(
        &self,
        entity: EntityId,
        eye: Point3<f32>,
        desired: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Option<Vector3<f32>> {
        let handle = *self.entity_id_to_body.get(&entity)?;
        let body = self.rigid_body_set.get(handle)?;
        let colliders: Vec<_> = body
            .colliders()
            .iter()
            .filter_map(|h| self.collider_set.get(*h))
            .filter(|c| !c.is_sensor())
            .collect();
        if colliders.is_empty() {
            return None;
        }
        let clear = |position: Vector3<f32>| {
            let pose = Isometry::from_parts(
                Translation::from(vec_to_nvec(position)),
                quat_to_nquat(rotation),
            );
            let delta = position - vec3(eye.x, eye.y, eye.z);
            colliders.iter().all(|collider| {
                // Match the released body's real contacts, including its
                // player exclusion and ray-only interaction proxies.
                let queries = self.broad_phase.as_query_pipeline(
                    self.narrow_phase.query_dispatcher(),
                    &self.rigid_body_set,
                    &self.collider_set,
                    QueryFilter::default()
                        .groups(collider.collision_groups())
                        .exclude_sensors()
                        .exclude_rigid_body(handle),
                );
                let local = collider.position_wrt_parent().copied().unwrap_or_default();
                queries
                    .intersect_shape(pose * local, collider.shape())
                    .next()
                    .is_none()
                    && (delta.magnitude2() == 0.0
                        || queries
                            .cast_ray(
                                &Ray::new(point![eye.x, eye.y, eye.z], vec_to_nvec(delta)),
                                1.0,
                                true,
                            )
                            .is_none())
            })
        };
        if clear(desired) {
            return Some(desired);
        }
        for radius in [0.3, 0.6, 0.9, 1.2] {
            for (x, z) in [
                (1.0, 0.0),
                (-1.0, 0.0),
                (0.0, 1.0),
                (0.0, -1.0),
                (0.7071, 0.7071),
                (0.7071, -0.7071),
                (-0.7071, 0.7071),
                (-0.7071, -0.7071),
            ] {
                let candidate = desired + vec3(x * radius, 0.0, z * radius);
                if clear(candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(obstacle: Option<(Vector3<f32>, Vector3<f32>)>) -> (PhysicsWorld, EntityId) {
        let mut world = PhysicsWorld::new();
        let item = EntityId::from_inner(1).unwrap();
        world.add_dynamic(
            item,
            vec3(10.0, 10.0, 10.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.8, 0.2, 0.2)),
            CollisionGroup::entity().non_solid_to_player(),
            false,
            DynamicPhysicsOptions::default(),
        );
        if let Some((at, size)) = obstacle {
            world.add_kinematic(
                EntityId::from_inner(2).unwrap(),
                at,
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                size,
                CollisionGroup::entity(),
                false,
            );
        }
        let mut player = world.create_player(vec3(0.0, 0.0, 0.0), EntityId::from_inner(3).unwrap());
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        (world, item)
    }

    #[test]
    fn clear_release_preserves_open_space_and_ignores_the_player() {
        let (world, item) = fixture(None);
        let desired = vec3(0.0, 0.0, 0.0);
        assert_eq!(
            world.clear_item_release_position(
                item,
                point3(0.0, 1.0, 0.0),
                desired,
                Quaternion::new(1.0, 0.0, 0.0, 0.0)
            ),
            Some(desired)
        );
    }

    #[test]
    fn clear_release_moves_the_whole_long_prop_off_a_door_face() {
        let (world, item) = fixture(Some((vec3(0.7, 0.0, 0.0), vec3(0.2, 4.0, 4.0))));
        let at = world
            .clear_item_release_position(
                item,
                point3(0.0, 1.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            )
            .unwrap();
        assert!(at.x + 0.9 < 0.6);
        assert!(at.magnitude() <= 1.2);
    }

    #[test]
    fn clear_release_refuses_enclosed_space_and_cannot_cross_a_thin_wall() {
        let (world, item) = fixture(Some((vec3(0.0, 0.0, 0.0), vec3(6.0, 4.0, 6.0))));
        assert_eq!(
            world.clear_item_release_position(
                item,
                point3(0.0, 1.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0)
            ),
            None
        );
        let (world, item) = fixture(Some((vec3(0.0, 0.0, 0.0), vec3(0.1, 4.0, 6.0))));
        assert_eq!(
            world.clear_item_release_position(
                item,
                point3(-1.0, 0.0, 0.0),
                vec3(2.0, 0.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0)
            ),
            None
        );
    }
    #[test]
    fn clear_release_respects_rotation_and_each_offset_collider() {
        let (mut world, item) = fixture(Some((vec3(0.7, 0.0, 0.0), vec3(0.2, 4.0, 4.0))));
        let desired = vec3(0.0, 0.0, 0.0);
        let quarter_turn = Quaternion::new(0.70710677, 0.0, 0.70710677, 0.0);
        assert_eq!(
            world.clear_item_release_position(item, point3(0.0, 1.0, 0.0), desired, quarter_turn),
            Some(desired)
        );
        // The rotated secondary sphere reaches the door even though the main
        // cuboid fits. Test its local offset, not just the first collider.
        let handle = world.entity_id_to_body[&item];
        let collider = ColliderBuilder::ball(0.3)
            .translation(vector![0.0, 0.0, 0.5])
            .collision_groups(CollisionGroup::entity().non_solid_to_player().collision)
            .build();
        world
            .collider_set
            .insert_with_parent(collider, handle, &mut world.rigid_body_set);
        let at = world
            .clear_item_release_position(item, point3(0.0, 1.0, 0.0), desired, quarter_turn)
            .unwrap();
        assert!(at.x < 0.0);
    }
}
