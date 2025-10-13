mod debug_render_pipeline;
mod physics_events;
mod util;

use collision::Aabb3;
use engine::profile;
use std::collections::{HashMap, HashSet};
use util::*;

use bitflags::bitflags;
use cgmath::{point3, InnerSpace, Matrix4, Point3, Quaternion, Vector3};
use dark::{mission::SystemShock2Level, SCALE_FACTOR};
use engine::scene::SceneObject;
use rapier3d::{
    control::{CharacterAutostep, CharacterLength, KinematicCharacterController},
    na::UnitQuaternion,
    prelude::*,
};
use shipyard::EntityId;

use physics_events::*;

use self::debug_render_pipeline::DebugRenderer;

bitflags! {
    pub struct InternalCollisionGroups: u32 {
        const WORLD = 1 << 0; // 1
        const ENTITY = 1 << 1; // 2
        const SELECTABLE = 1 << 2; // 2
        const PLAYER = 1 << 3;
        const UI = 1 << 4;
        const HITBOX = 1 << 5;
        const RAYCAST = 1 << 6;
        const ALL_COLLIDABLE = Self::WORLD.bits | Self::ENTITY.bits | Self::PLAYER.bits | Self::SELECTABLE.bits;
        const ALL = Self::ALL_COLLIDABLE.bits | Self::UI.bits | Self::HITBOX.bits | Self::RAYCAST.bits;
    }
}

pub struct DynamicPhysicsOptions {
    pub gravity_scale: f32,
}

impl Default for DynamicPhysicsOptions {
    fn default() -> DynamicPhysicsOptions {
        DynamicPhysicsOptions { gravity_scale: 1.0 }
    }
}

pub struct CollisionGroup(InteractionGroups);

impl CollisionGroup {
    pub fn world() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::WORLD.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
        })
    }

    pub fn hitbox() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: (InternalCollisionGroups::HITBOX.bits
                | InternalCollisionGroups::RAYCAST.bits)
                .into(),
            filter: InternalCollisionGroups::RAYCAST.bits.into(),
        })
    }

    pub fn ui() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::UI.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
        })
    }

    pub fn entity() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::ENTITY.bits.into(),
            filter: (InternalCollisionGroups::WORLD.bits
                | InternalCollisionGroups::PLAYER.bits
                | InternalCollisionGroups::SELECTABLE.bits
                | InternalCollisionGroups::ENTITY.bits)
                .into(),
        })
    }

    pub fn projectile() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::RAYCAST.bits.into(),
            filter: (InternalCollisionGroups::WORLD.bits | InternalCollisionGroups::HITBOX.bits)
                .into(),
        })
    }

    pub fn selectable() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct RayCastResult {
    pub hit_point: Point3<f32>,
    pub hit_normal: Vector3<f32>,
    pub maybe_entity_id: Option<EntityId>,
    pub maybe_rigid_body_handle: Option<RigidBodyHandle>,
    pub is_sensor: bool,
    // TODO:
    // entity_id
}

#[derive(Clone, Debug)]
pub enum CollisionEvent {
    BeginIntersect {
        sensor_id: EntityId,
        entity_id: EntityId,
    },
    EndIntersect {
        sensor_id: EntityId,
        entity_id: EntityId,
    },
    CollisionStarted {
        entity1_id: EntityId,
        entity2_id: EntityId,
    },
}

#[derive(Debug)]
pub enum PhysicsShape {
    Capsule { height: f32, radius: f32 },
    Cuboid(Vector3<f32>),
    Sphere(f32),
}

pub struct PlayerHandle {
    // Player
    controller: KinematicCharacterController,
    character_handle: RigidBodyHandle,
}

pub struct PhysicsWorld {
    gravity: Vector<Real>,
    integration_parameters: IntegrationParameters,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: BroadPhase,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
    query_pipeline: QueryPipeline,
    collider_set: ColliderSet,
    rigid_body_set: RigidBodySet,

    rigid_bodies_with_forces: Vec<RigidBodyHandle>,

    entity_id_to_body: HashMap<EntityId, RigidBodyHandle>,

    // TODO:
    // physics_hooks: Box<dyn PhysicsHooks>,
    // event_handler: Box<dyn EventHandler>,

    // Debug
    debug_pipeline: DebugRenderPipeline,

    // Sensor Intersection List
    player_sensor_intersections: HashSet<EntityId>,

    // Collision Events
    events: PhysicsEvents,
}

impl PhysicsWorld {
    pub fn add_level_geometry(&mut self, entity_id: EntityId, level: &SystemShock2Level) {
        /* Create the ground. */
        //let collider = ColliderBuilder::cuboid(100.0, 0.1, 100.0).build();

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for geo in &level.all_geometry {
            let verts = &geo.verts;

            let mut idx = 0;
            let len = verts.len();

            while idx < len {
                let dest_idx = vertices.len() as u32;

                vertices.push(vec_to_npoint(verts[idx].position));
                vertices.push(vec_to_npoint(verts[idx + 1].position));
                vertices.push(vec_to_npoint(verts[idx + 2].position));

                indices.push([dest_idx, dest_idx + 1, dest_idx + 2]);

                idx += 3;
            }
        }

        let mut collider = ColliderBuilder::trimesh(vertices, indices).build();
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(InteractionGroups {
            memberships: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
        });
        self.collider_set.insert(collider);
    }

    pub fn set_position_rotation2(
        &mut self,
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            self.set_position_rotation(*handle, position, rotation);
        }
    }

    pub fn set_transform2(&mut self, entity_id: EntityId, transform: Matrix4<f32>) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                let xform = mat_to_nmat(transform);
                if rigid_body.is_kinematic() {
                    rigid_body.set_next_kinematic_position(xform);
                } else {
                    rigid_body.set_position(xform, true);
                    rigid_body.reset_torques(true);
                    rigid_body.reset_forces(true);
                }
            }
        }
    }

    pub fn set_position_rotation(
        &mut self,
        handle: RigidBodyHandle,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            let nquat = nalgebra::geometry::Quaternion::new(
                rotation.s,
                rotation.v.x,
                rotation.v.y,
                rotation.v.z,
            );
            let nquat_unit = UnitQuaternion::from_quaternion(nquat);
            let mut xform = Isometry::identity();
            xform.append_rotation_mut(&nquat_unit);
            xform.translation = Translation {
                vector: vec_to_nvec(position),
            };
            if rigid_body.is_kinematic() {
                rigid_body.set_next_kinematic_position(xform);
            } else {
                rigid_body.set_position(xform, true);
                rigid_body.reset_torques(true);
                rigid_body.reset_forces(true);
            }
        }
    }

    pub fn set_translation(&mut self, handle: RigidBodyHandle, position: Vector3<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            rigid_body.set_next_kinematic_translation(vec_to_nvec(position));
        }
    }

    pub fn set_rotation(&mut self, handle: RigidBodyHandle, quat: Quaternion<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            if rigid_body.is_kinematic() {
                rigid_body.set_next_kinematic_rotation(quat_to_nquat(quat));
            } else {
                rigid_body.set_rotation(quat_to_nquat(quat), true);
            }
        }
    }

    pub fn set_rotation2(&mut self, entity_id: EntityId, quat: Quaternion<f32>) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_rotation(quat_to_nquat(quat), true);
            }
        }
    }

    pub fn set_gravity(&mut self, entity_id: EntityId, percent: f32) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_gravity_scale(percent, true);
            }
        }
    }

    pub fn get_size(&self, handle: RigidBodyHandle) -> Option<f32> {
        let maybe_rigid_body = self.rigid_body_set.get(handle);
        maybe_rigid_body.map(|rigid_body| {
            let character_collider = &self.collider_set[rigid_body.colliders()[0]];
            let aabb = character_collider.compute_aabb();

            let size0 = (aabb.maxs[0] - aabb.mins[0]).abs();
            let size1 = (aabb.maxs[1] - aabb.mins[1]).abs();
            let size2 = (aabb.maxs[2] - aabb.mins[2]).abs();

            size0.max(size1).max(size2)
        })
    }

    pub fn clear_forces(&mut self) {
        for rigid_body_handle in &self.rigid_bodies_with_forces {
            let rigid_body = &mut self.rigid_body_set[*rigid_body_handle];
            rigid_body.reset_forces(true);
            rigid_body.reset_torques(true);
        }

        self.rigid_bodies_with_forces = Vec::new();
    }

    pub fn apply_force(&mut self, handle: RigidBodyHandle, force: Vector3<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            rigid_body.add_force(vec_to_nvec(force), true);
            self.rigid_bodies_with_forces.push(handle);
        }
    }

    pub fn apply_torque(&mut self, handle: RigidBodyHandle, force: Vector3<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            rigid_body.add_torque(vec_to_nvec(force), true);
            self.rigid_bodies_with_forces.push(handle);
        }
    }

    pub fn set_player_translation(
        &mut self,
        position: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) {
        let character_body = self
            .rigid_body_set
            .get_mut(player_handle.character_handle)
            .unwrap();
        character_body.set_translation(vec_to_nvec(position), true)
    }

    pub fn get_position2(&self, entity_id: EntityId) -> Option<Vector3<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);

            maybe_rigid_body.map(|rigid_body| nvec_to_cgmath(*rigid_body.translation()))
        } else {
            None
        }
    }

    pub fn get_aabb2(&self, entity_id: EntityId) -> Option<Aabb3<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);
            maybe_rigid_body.map(|rigid_body| {
                let character_collider = &self.collider_set[rigid_body.colliders()[0]];
                let aabb = character_collider.compute_aabb();
                Aabb3 {
                    min: point3(aabb.mins.x, aabb.mins.y, aabb.mins.z),
                    max: point3(aabb.maxs.x, aabb.maxs.y, aabb.maxs.z),
                }
            })
        } else {
            None
        }
    }

    pub fn get_position(&self, handle: RigidBodyHandle) -> Option<Vector3<f32>> {
        let maybe_rigid_body = self.rigid_body_set.get(handle);

        maybe_rigid_body.map(|rigid_body| nvec_to_cgmath(*rigid_body.translation()))
    }

    pub fn get_angular_velocity(&self, handle: RigidBodyHandle) -> Option<Vector3<f32>> {
        let maybe_rigid_body = self.rigid_body_set.get(handle);

        maybe_rigid_body.map(|rigid_body| nvec_to_cgmath(*rigid_body.angvel()))
    }

    pub fn get_velocity(&self, entity_id: EntityId) -> Option<Vector3<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);

            maybe_rigid_body.map(|rigid_body| {
                nvec_to_cgmath(rigid_body.velocity_at_point(rigid_body.center_of_mass()))
            })
        } else {
            None
        }
    }

    pub fn set_velocity(&mut self, entity_id: EntityId, velocity: Vector3<f32>) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_linvel(vec_to_nvec(velocity), true);
            }
        }
    }

    pub fn get_rotation(&self, handle: RigidBodyHandle) -> Option<Quaternion<f32>> {
        let maybe_rigid_body = self.rigid_body_set.get(handle);

        maybe_rigid_body.map(|rigid_body| nquat_to_quat(*rigid_body.rotation()))
    }

    pub fn get_rotation2(&self, entity_id: EntityId) -> Option<Quaternion<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);

            maybe_rigid_body.map(|rigid_body| nquat_to_quat(*rigid_body.rotation()))
        } else {
            None
        }
    }

    pub fn add_dynamic(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
        offset: Vector3<f32>,
        shape: PhysicsShape,
        collision_group: CollisionGroup,
        is_sensor: bool,
        opts: DynamicPhysicsOptions,
    ) -> RigidBodyHandle {
        let nquat =
            nalgebra::geometry::Quaternion::new(facing.s, facing.v.x, facing.v.y, facing.v.z);
        let nquat_unit = UnitQuaternion::from_quaternion(nquat);

        let mut test = Isometry::identity();
        test.append_rotation_mut(&nquat_unit);
        test.translation = Translation {
            vector: vec_to_nvec(pos),
        };

        let mut rigid_body = RigidBodyBuilder::dynamic()
            // TODO: How can we make this more reliable? Seems to slow down the projectile randomly...
            // .ccd_enabled(true)
            .position(test)
            .build();
        rigid_body.user_data = entity_id.inner() as u128;
        rigid_body.set_gravity_scale(opts.gravity_scale, false);
        //rigid_body.set_additional_mass(5.0, false);
        let handle = &self.rigid_body_set.insert(rigid_body);
        let mut collider = match shape {
            PhysicsShape::Capsule { height, radius } => {
                assert!(height > 0.0 && radius > 0.0);
                ColliderBuilder::capsule_y(height / 2.0, radius)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(0.7)
                    .build()
            }
            PhysicsShape::Cuboid(size) => {
                assert!(size.x > 0.0 && size.y > 0.0 && size.z > 0.0);
                ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(0.7)
                    .build()
            }
            PhysicsShape::Sphere(size) => {
                ColliderBuilder::ball(size)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(0.7)
                    .build()
            }
            _ => panic!("Unknown shape"),
        };

        self.entity_id_to_body.insert(entity_id, *handle);
        collider.set_density(0.1);
        collider.set_enabled(true);
        collider.set_sensor(is_sensor);
        collider.set_collision_groups(collision_group.0);
        collider
            .set_active_events(ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS);
        collider.user_data = entity_id.inner() as u128;
        self.collider_set
            .insert_with_parent(collider, *handle, &mut self.rigid_body_set);
        *handle
    }

    pub fn add_kinematic(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
        offset: Vector3<f32>,
        size: Vector3<f32>,
        collision_groups: CollisionGroup,
        is_sensor: bool,
    ) -> RigidBodyHandle {
        //for (pos, size, facing, id, is_sensor) in &phys_objs {
        let nquat =
            nalgebra::geometry::Quaternion::new(facing.s, facing.v.x, facing.v.y, facing.v.z);
        let nquat_unit = UnitQuaternion::from_quaternion(nquat);
        // let angles = nquat_unit.euler_angles();

        // let r0 = 33.75f32.to_radians();
        // let r1 = 90f32.to_radians();
        // let r2 = 0.0;

        // r0-r1-r2
        // r0-r2-r1
        //let quat = UnitQuaternion::from_euler_angles(facing.y, facing.x, facing.z);

        let mut test = Isometry::identity();
        test.append_rotation_mut(&nquat_unit);
        test.translation = Translation {
            vector: vec_to_nvec(pos),
        };

        let mut rigid_body = RigidBodyBuilder::kinematic_position_based()
            .position(test)
            .build();
        rigid_body.user_data = entity_id.inner() as u128;
        let handle = &self.rigid_body_set.insert(rigid_body);
        assert!(size.x >= 0.0 && size.y >= 0.0 && size.z >= 0.0);
        let mut collider = ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0)
            //.rotation(vector!(angles.0, angles.1, angles.2))
            //.rotation(vector!(facing.z, facing.x, facing.y))
            .translation(vec_to_nvec(offset))
            //.position(test)
            .restitution(0.7)
            .build();

        self.entity_id_to_body.insert(entity_id, *handle);

        collider.set_enabled(true);
        collider.set_sensor(is_sensor);
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(collision_groups.0);

        self.collider_set
            .insert_with_parent(collider, *handle, &mut self.rigid_body_set);
        *handle
    }

    pub fn remove(&mut self, entity_id: EntityId) {
        let entity_as_int = entity_id.inner() as u128;
        let mut bodies_to_remove = Vec::new();
        for (handle, body) in self.rigid_body_set.iter() {
            if body.user_data == entity_as_int {
                bodies_to_remove.push(handle);
            }
        }
        for handle in bodies_to_remove {
            self.rigid_body_set.remove(
                handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
        self.entity_id_to_body.remove(&entity_id);
    }

    pub fn create_player(
        &mut self,
        start_pos: Vector3<f32>,
        player_entity: EntityId,
    ) -> PlayerHandle {
        let mut rigid_body = RigidBodyBuilder::kinematic_position_based()
            .translation(vec_to_nvec(start_pos))
            .ccd_enabled(true)
            .build();
        //rigid_body.ccd_enabled(true);
        let player_entity_user_data = player_entity.inner() as u128;
        rigid_body.user_data = player_entity_user_data;
        let character_handle = self.rigid_body_set.insert(rigid_body);
        let mut collider =
            ColliderBuilder::cuboid(0.8 / SCALE_FACTOR, 2.4 / SCALE_FACTOR, 0.8 / SCALE_FACTOR);
        //let mut collider = ColliderBuilder::capsule_y(2.4 / SCALE_FACTOR, 0.8 / SCALE_FACTOR);
        collider = collider.collision_groups(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
        ));
        collider = collider.user_data(player_entity_user_data);

        //collider.user_data = player_entity_user_data;
        self.collider_set
            .insert_with_parent(collider, character_handle, &mut self.rigid_body_set);

        let mut controller = KinematicCharacterController::default();
        controller.autostep = Some(CharacterAutostep {
            include_dynamic_bodies: true,
            ..CharacterAutostep::default()
        });
        controller.offset = CharacterLength::Absolute(0.2 / SCALE_FACTOR);

        self.entity_id_to_body
            .insert(player_entity, character_handle);

        PlayerHandle {
            controller,
            character_handle,
        }
    }

    pub fn new() -> PhysicsWorld {
        let rigid_body_set = RigidBodySet::new();
        let collider_set = ColliderSet::new();

        /* Create other structures necessary for the simulation. */
        let gravity = vector![0.0, -9.81, 0.0];
        let integration_parameters = IntegrationParameters {
            max_ccd_substeps: 1,
            ..IntegrationParameters::default()
        };
        let physics_pipeline = PhysicsPipeline::new();
        let island_manager = IslandManager::new();
        let broad_phase = BroadPhase::new();
        let narrow_phase = NarrowPhase::new();
        let impulse_joint_set = ImpulseJointSet::new();
        let multibody_joint_set = MultibodyJointSet::new();
        let ccd_solver = CCDSolver::new();
        let mut query_pipeline = QueryPipeline::new();

        let debug_pipeline = DebugRenderPipeline::new(
            DebugRenderStyle::default(),
            DebugRenderMode::default()
                | DebugRenderMode::COLLIDER_AABBS
                | DebugRenderMode::COLLIDER_SHAPES
                | DebugRenderMode::CONTACTS,
        );

        query_pipeline.update(&rigid_body_set, &collider_set);
        PhysicsWorld {
            gravity,
            integration_parameters,
            collider_set,
            physics_pipeline,
            island_manager,
            broad_phase,
            narrow_phase,
            impulse_joint_set,
            multibody_joint_set,
            ccd_solver,
            query_pipeline,
            rigid_body_set,
            rigid_bodies_with_forces: Vec::new(),
            // TODO:
            // physics_hooks: Box::new(physics_hooks),
            // event_handler: Box::new(event_handler),
            entity_id_to_body: HashMap::new(),

            debug_pipeline,

            player_sensor_intersections: HashSet::new(),

            events: PhysicsEvents::new(),
        }
    }

    pub fn debug_render(&mut self) -> Vec<SceneObject> {
        let mut debug_renderer = DebugRenderer::new();

        self.debug_pipeline.render(
            &mut debug_renderer,
            &self.rigid_body_set,
            &self.collider_set,
            &self.impulse_joint_set,
            &self.multibody_joint_set,
            &self.narrow_phase,
        );

        debug_renderer.render()
    }

    pub fn update(
        &mut self,
        desired_movement: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> (Vector3<f32>, Vec<CollisionEvent>) {
        /* Run the game loop, stepping the simulation once per frame. */
        profile!(
            "physics.step",
            self.physics_pipeline.step(
                &self.gravity,
                &self.integration_parameters,
                &mut self.island_manager,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.rigid_body_set,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                &mut self.ccd_solver,
                Some(&mut self.query_pipeline),
                &(),
                &self.events,
            )
        );

        profile!(
            "physics.update_query_pipeline",
            self.query_pipeline
                .update(&self.rigid_body_set, &self.collider_set)
        );

        // Update character controller
        let desired_movement = vec_to_nvec(desired_movement);
        let (mut collision_events, character_body) =
            { self.move_player(desired_movement, player_handle) };
        let translation = nvec_to_cgmath(*character_body.translation());

        let mut additional_collision_events = { self.events.get_and_clear_events() };

        collision_events.append(&mut additional_collision_events);

        // Output result
        (translation, collision_events)
    }

    fn move_player(
        &mut self,
        desired_movement: Vector<Real>,
        player_handle: &mut PlayerHandle,
    ) -> (Vec<CollisionEvent>, &RigidBody) {
        let character_body = &self.rigid_body_set[player_handle.character_handle];
        let original_position = *character_body.position();
        let character_collider = &self.collider_set[character_body.colliders()[0]];
        let _character_mass = character_body.mass();

        let mut gravity = -0.5 / SCALE_FACTOR;
        gravity *= character_body.gravity_scale();

        let movement_with_gravity = desired_movement + Vector::y() * gravity;

        //let mut collisions = vec![];
        let mvt = profile!(
            "physics.move_player",
            player_handle.controller.move_shape(
                self.integration_parameters.dt,
                &self.rigid_body_set,
                &self.collider_set,
                &self.query_pipeline,
                character_collider.shape(),
                character_collider.position(),
                movement_with_gravity.cast::<Real>(),
                QueryFilter::new()
                    .groups(InteractionGroups::new(
                        InternalCollisionGroups::PLAYER.bits.into(),
                        InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
                    ))
                    .exclude_rigid_body(player_handle.character_handle)
                    .exclude_sensors(),
                |_c| (),
                //|c| collisions.push(c),
            )
        );

        let mut collision_events = Vec::new();
        let mut current_sensor_intersections = HashSet::new();
        profile!("physics.intersections_with_shape", {
            &self.query_pipeline.intersections_with_shape(
                &self.rigid_body_set,
                &self.collider_set,
                &original_position,
                //&(mvt.translation + vector![0.00, 0.01, 0.00]),
                character_collider.shape(),
                //1.1,
                //true,
                //QueryFilter::new().exclude_rigid_body(player_handle.character_handle),
                QueryFilter::new()
                    .predicate(&|_collider_handle: ColliderHandle, _c: &Collider| _c.is_sensor()),
                |handle| {
                    let collider = &self.collider_set.get(handle).unwrap();
                    // if collider.is_sensor() {
                    //     println!("!!--!! COLLISION: {} {:?}", collider.user_data, toi)
                    // }
                    if collider.is_sensor() {
                        if let (Some(_entity1_id), Some(entity2_id)) = (
                            EntityId::from_inner(character_body.user_data as u64),
                            EntityId::from_inner(collider.user_data as u64),
                        ) {
                            current_sensor_intersections.insert(entity2_id);
                        }
                    }
                    true
                },
            )
        });

        let player_id = EntityId::from_inner(character_body.user_data as u64).unwrap();

        let new_collisions: HashSet<EntityId> = current_sensor_intersections
            .difference(&self.player_sensor_intersections)
            .cloned()
            .collect::<HashSet<EntityId>>();

        let no_longer_collisions: HashSet<EntityId> = self
            .player_sensor_intersections
            .difference(&current_sensor_intersections)
            .cloned()
            .collect();

        for expired_collision in no_longer_collisions {
            collision_events.push(CollisionEvent::EndIntersect {
                sensor_id: expired_collision,
                entity_id: player_id,
            });
        }

        for new_collision in new_collisions {
            collision_events.push(CollisionEvent::BeginIntersect {
                sensor_id: new_collision,
                entity_id: player_id,
            });
        }

        self.player_sensor_intersections = current_sensor_intersections;

        // for collision in &collisions {
        //     let _collider = &self.collider_set[collision.handle];
        //     self.controller.solve_character_collision_impulses(
        //         self.integration_parameters.dt,
        //         &mut self.rigid_body_set,
        //         &self.collider_set,
        //         &self.query_pipeline,
        //         character_collider.shape(),
        //         character_mass,
        //         collision,
        //         QueryFilter::new().exclude_rigid_body(self.character_handle),
        //     )
        // }
        let character_body = &mut self.rigid_body_set[player_handle.character_handle];
        let _original_pos = character_body.position().translation.vector;
        let pos = character_body.position();
        character_body.set_next_kinematic_translation(pos.translation.vector + mvt.translation);
        (collision_events, character_body)
    }

    pub fn ray_cast2(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        max_toi: f32,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
    ) -> Option<RayCastResult> {
        let direction = direction.normalize();
        let ray = Ray::new(
            point![start_point.x, start_point.y, start_point.z],
            vector![direction.x, direction.y, direction.z],
        );
        // TODO: Take end point instead
        let solid = true;
        let mut filter = QueryFilter::default();

        if ignore_sensors {
            filter = filter.exclude_sensors()
        };

        let binding = |_collider_handle: ColliderHandle, collider: &Collider| {
            let data = collider.user_data;
            let maybe_entity_id = EntityId::from_inner(data as u64);

            maybe_entity_id != entity_to_ignore
        };
        filter = filter.predicate(&binding);

        filter = filter.groups(InteractionGroups::new(
            InternalCollisionGroups::ALL.bits.into(),
            collision_groups.bits.into(),
        ));

        if let Some((handle, intersection)) = self.query_pipeline.cast_ray_and_get_normal(
            &self.rigid_body_set,
            &self.collider_set,
            &ray,
            max_toi,
            solid,
            filter,
        ) {
            // This is similar to `QueryPipeline::cast_ray` illustrated above except
            // that it also returns the normal of the collider shape at the hit point.
            let hit_point = ray.point_at(intersection.toi);
            let hit_normal = intersection.normal;
            let collider = self.collider_set.get(handle).unwrap();
            let maybe_rigid_body_handle = collider.parent();
            let data = collider.user_data;

            let maybe_entity_id = EntityId::from_inner(data as u64);

            // let rigid_body_handle = self.collider_set.get(handle).unwrap().parent().unwrap();
            // let rigid_body = self.rigid_body_set.get(rigid_body_handle).unwrap();

            // println!(
            //     "Collider {:?} hit at point {} with normal {} collider_data: {}",
            //     handle, hit_point, hit_normal, data
            // );

            Some(RayCastResult {
                hit_point: npoint_to_cgmath(hit_point),
                hit_normal: nvec_to_cgmath(hit_normal),
                maybe_entity_id,
                maybe_rigid_body_handle,
                is_sensor: collider.is_sensor(),
            })
        } else {
            None
        }
    }

    pub fn ray_cast(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        collision_groups: InternalCollisionGroups,
    ) -> Option<RayCastResult> {
        self.ray_cast2(start_point, direction, 100.0, collision_groups, None, true)
    }

    pub(crate) fn set_enabled_rotations(
        &mut self,
        entity_id: EntityId,
        arg_1: bool,
        arg_2: bool,
        arg_3: bool,
    ) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_enabled_rotations(arg_1, arg_2, arg_3, true);
            }
        }
    }
}
