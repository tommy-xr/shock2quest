mod debug_render_pipeline;
mod physics_events;
pub(crate) mod util;

use collision::Aabb3;
use engine::profile;
use std::collections::{HashMap, HashSet};
use util::*;

use bitflags::bitflags;
use cgmath::{InnerSpace, Point3, Quaternion, Vector3, point3, vec3};
use dark::{SCALE_FACTOR, mission::SystemShock2Level};
use engine::scene::SceneObject;
use rapier3d::{
    control::{CharacterLength, KinematicCharacterController},
    na::UnitQuaternion,
    prelude::*,
};
use shipyard::EntityId;

use physics_events::*;

use self::debug_render_pipeline::DebugRenderer;

const MOVEMENT_STEP_SIZE: f32 = 20.0;

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
    /// Coefficient of restitution (bounciness). Already calibrated from Dark's
    /// authored `elasticity` at the call site; see `entity_creator`. The default
    /// reproduces the value dynamic bodies used before per-object attributes
    /// were threaded through.
    pub restitution: f32,
    /// Coefficient of friction, from Dark's authored `friction`. The default
    /// reproduces Rapier's default friction (what dynamic bodies used before).
    pub friction: f32,
}

impl Default for DynamicPhysicsOptions {
    fn default() -> DynamicPhysicsOptions {
        DynamicPhysicsOptions {
            gravity_scale: 1.0,
            restitution: 0.7,
            friction: 0.5,
        }
    }
}

pub struct CollisionGroup(InteractionGroups);

impl CollisionGroup {
    pub fn hitbox() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: (InternalCollisionGroups::HITBOX.bits
                | InternalCollisionGroups::RAYCAST.bits)
                .into(),
            filter: InternalCollisionGroups::RAYCAST.bits.into(),
            test_mode: Default::default(),
        })
    }

    pub fn ui() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::UI.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
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
            test_mode: Default::default(),
        })
    }

    pub fn selectable() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    /// Collision group for ragdoll limb bodies. Members are `SELECTABLE` (so
    /// they remain raycast/selectable) and collide with `WORLD` geometry *and*
    /// each other (`SELECTABLE`), so limbs don't pass through the torso/head.
    ///
    /// Limb-vs-limb collision between *directly jointed* bodies is disabled at the
    /// joint level (`contacts_enabled(false)`), not here - otherwise adjacent
    /// bodies that spawn slightly overlapping get violently ejected (the original
    /// ragdoll "explosion"). The filter still excludes the leftover creature
    /// capsule (`ENTITY`) and per-joint hitboxes (`HITBOX`).
    pub fn ragdoll() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: (InternalCollisionGroups::WORLD.bits
                | InternalCollisionGroups::SELECTABLE.bits)
                .into(),
            test_mode: Default::default(),
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
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
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

/// Clamp a collider's full size to finite, positive, bounded values. Some Dark
/// objects (notably certain trigger/`Ecology` objects) resolve to a non-finite
/// (e.g. infinite) collider dimension, producing a collider with a NaN/infinite
/// AABB. The old SAP broad-phase tolerated that; rapier's BVH broad-phase panics
/// on it (parry binned build, "index out of bounds"). Sanitize at the source and
/// log the offender so the bad data is traceable.
fn sanitize_collider_size(entity_id: EntityId, context: &str, size: Vector3<f32>) -> Vector3<f32> {
    const MIN_SIZE: f32 = 0.01;
    const MAX_SIZE: f32 = 1.0e4;
    let clamp = |v: f32| -> f32 {
        if v.is_finite() && v > 0.0 {
            v.min(MAX_SIZE)
        } else {
            MIN_SIZE
        }
    };
    let out = Vector3::new(clamp(size.x), clamp(size.y), clamp(size.z));
    if out != size {
        tracing::warn!(
            "[physics] {} entity {:?}: invalid collider size {:?} -> clamped to {:?}",
            context,
            entity_id,
            size,
            out
        );
    }
    out
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

        let mut collider = ColliderBuilder::trimesh(vertices, indices)
            .expect("level geometry trimesh")
            .build();
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(InteractionGroups {
            memberships: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        });
        self.collider_set.insert(collider);
    }

    pub fn add_collider(&mut self, entity_id: EntityId, mut collider: Collider) {
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(InteractionGroups {
            memberships: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
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

    pub fn apply_impulse(&mut self, handle: RigidBodyHandle, impulse: Vector3<f32>) {
        if let Some(rigid_body) = self.rigid_body_set.get_mut(handle) {
            rigid_body.apply_impulse(vec_to_nvec(impulse), true);
        }
    }

    /// Shove every dynamic body within `radius` of `center` directly away
    /// from it, adding `speed * (1 - d/radius)` to its velocity (explosion
    /// blasts). Mass-independent, like the game's other impulse-as-speed
    /// launches (flinderize).
    pub fn apply_radial_impulse(&mut self, center: Vector3<f32>, radius: f32, speed: f32) {
        for (_handle, body) in self.rigid_body_set.iter_mut() {
            if !body.is_dynamic() {
                continue;
            }
            let translation = body.translation();
            let offset = vec3(translation.x, translation.y, translation.z) - center;
            let distance = offset.magnitude();
            if distance >= radius {
                continue;
            }
            // A body at the exact center has no outward direction; toss it up.
            let direction = if distance > 1e-3 {
                offset / distance
            } else {
                vec3(0.0, 1.0, 0.0)
            };
            let delta = direction * speed * (1.0 - distance / radius);
            let new_velocity = body.linvel() + vec_to_nvec(delta);
            body.set_linvel(new_velocity, true);
        }
    }

    pub fn remove_rigid_body_handle(&mut self, handle: RigidBodyHandle) {
        self.rigid_body_set.remove(
            handle,
            &mut self.island_manager,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            true,
        );
    }

    pub fn remove_impulse_joint(&mut self, handle: ImpulseJointHandle) {
        self.impulse_joint_set.remove(handle, true);
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

    /// The character body's current translation, without stepping the
    /// simulation. Used while time is frozen (debug-runtime pause) so
    /// teleports - which write the physics body directly - are still
    /// reflected in `PlayerInfo`/introspection before the next real step.
    pub fn get_player_translation(&self, player_handle: &PlayerHandle) -> Vector3<f32> {
        let character_body = self
            .rigid_body_set
            .get(player_handle.character_handle)
            .unwrap();
        nvec_to_cgmath(*character_body.translation())
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
            .pose(test)
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
                    .restitution(opts.restitution)
                    .friction(opts.friction)
                    .build()
            }
            PhysicsShape::Cuboid(size) => {
                let size = sanitize_collider_size(entity_id, "add_dynamic", size);
                ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(opts.restitution)
                    .friction(opts.friction)
                    .build()
            }
            PhysicsShape::Sphere(size) => {
                ColliderBuilder::ball(size)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(opts.restitution)
                    .friction(opts.friction)
                    .build()
            }
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
            .pose(test)
            .build();
        rigid_body.user_data = entity_id.inner() as u128;
        let handle = &self.rigid_body_set.insert(rigid_body);
        let size = sanitize_collider_size(entity_id, "add_kinematic", size);
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

        let player_entity_user_data = player_entity.inner() as u128;
        rigid_body.user_data = player_entity_user_data;
        let character_handle = self.rigid_body_set.insert(rigid_body);
        let mut collider =
            ColliderBuilder::cuboid(0.8 / SCALE_FACTOR, 2.4 / SCALE_FACTOR, 0.8 / SCALE_FACTOR);
        collider = collider.collision_groups(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        ));
        collider = collider.user_data(player_entity_user_data);

        self.collider_set
            .insert_with_parent(collider, character_handle, &mut self.rigid_body_set);

        let mut controller = KinematicCharacterController::default();

        controller.offset = CharacterLength::Absolute(0.1 / SCALE_FACTOR);
        controller.snap_to_ground = Some(CharacterLength::Absolute(0.1 / SCALE_FACTOR));
        controller.normal_nudge_factor = 0.1;

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
        let broad_phase = DefaultBroadPhase::new();
        let narrow_phase = NarrowPhase::new();
        let impulse_joint_set = ImpulseJointSet::new();
        let multibody_joint_set = MultibodyJointSet::new();
        let ccd_solver = CCDSolver::new();

        let debug_pipeline = DebugRenderPipeline::new(
            DebugRenderStyle::default(),
            DebugRenderMode::default()
                | DebugRenderMode::COLLIDER_AABBS
                | DebugRenderMode::COLLIDER_SHAPES
                | DebugRenderMode::CONTACTS,
        );

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

        // Joint-anchor overlay: each impulse joint constrains a point on the
        // parent body (anchor1, cyan cross) to coincide with a point on the child
        // body (anchor2, magenta cross). A line connects them - a satisfied joint
        // has overlapping crosses and an invisible line; a sagging/separated joint
        // (e.g. the hips) shows a visible gap.
        for (_h, joint) in self.impulse_joint_set.iter() {
            if let (Some(b1), Some(b2)) = (
                self.rigid_body_set.get(joint.body1),
                self.rigid_body_set.get(joint.body2),
            ) {
                let a1 = b1.position() * joint.data.local_frame1;
                let a2 = b2.position() * joint.data.local_frame2;
                let p1 = Vector3::new(a1.translation.x, a1.translation.y, a1.translation.z);
                let p2 = Vector3::new(a2.translation.x, a2.translation.y, a2.translation.z);
                debug_renderer.add_cross(p1, 0.12, Vector3::new(0.0, 0.6, 1.0)); // anchor1 blue
                debug_renderer.add_cross(p2, 0.12, Vector3::new(1.0, 0.0, 1.0)); // anchor2 magenta
                debug_renderer.add_line(p1, p2, Vector3::new(1.0, 1.0, 0.0)); // gap (yellow)
            }
        }

        debug_renderer.render()
    }

    pub fn update(
        &mut self,
        desired_movement: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> (Vector3<f32>, Vec<CollisionEvent>) {
        /* Run the game loop, stepping the simulation once per frame. */
        profile!(scope: "physics", level: TRACE, "physics.step", {
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
                &(),
                &self.events,
            )
        });

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
        let character_user_data = character_body.user_data;
        let character_collider = &self.collider_set[character_body.colliders()[0]];
        let _character_mass = character_body.mass();

        // In rapier 0.31 the `QueryPipeline` is a transient view built from the
        // broad-phase BVH, and it borrows the body/collider sets. Snapshot the
        // character shape (cheap Arc clone) and position up front so those
        // borrows are released before we build the query pipeline below.
        let character_shape = character_collider.shared_shape().clone();
        let character_pos = *character_collider.position();

        let step_size = Vector::y() * MOVEMENT_STEP_SIZE * self.integration_parameters.dt;

        // We do our player movement in two passes
        // First: move the player forward and a bit upwards
        // Second: Drop the player down for gravity
        // This wasn't necessary until upgrading to rapier v0.19.0 - when we upgraded to that version,
        // we started to snag on geometry.
        let movement_with_upward = desired_movement + step_size;

        let mut gravity = -0.5 / SCALE_FACTOR;
        gravity *= self.rigid_body_set[player_handle.character_handle].gravity_scale();

        let gravity_movement = Vector::y() * gravity - step_size;

        // Filter shared by both movement passes: only collide with the
        // collidable groups as the player, ignore the player body and sensors.
        let movement_filter = QueryFilter::new()
            .groups(InteractionGroups::new(
                InternalCollisionGroups::PLAYER.bits.into(),
                InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
                Default::default(),
            ))
            .exclude_rigid_body(player_handle.character_handle)
            .exclude_sensors();
        let dispatcher = self.narrow_phase.query_dispatcher();

        //let mut collisions = vec![];
        let (mvt1, mvt2) = profile!(scope: "physics", level: TRACE, "physics.move_player", {
            // HACK: For rapier v0.19.0, our previous strategy of combining the movement + gravity
            // caused us to snag on physics geometry. In order to counter this, we'll do the movement in two phases
            // a forward phase to move and then an application of gravity
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                movement_filter,
            );
            (player_handle.controller.move_shape(
                self.integration_parameters.dt,
                &queries,
                character_shape.as_ref(),
                &character_pos,
                movement_with_upward.cast::<Real>(),
                |_c| (),
                //|c| collisions.push(c),
            ),

            // Second pass: Apply gravity and undo our step size
            player_handle.controller.move_shape(
                self.integration_parameters.dt,
                &queries,
                character_shape.as_ref(),
                &character_pos,
                gravity_movement.cast::<Real>(),
                |_c| (),
                //|c| collisions.push(c),
            ))
        });

        let mut collision_events = Vec::new();
        let mut current_sensor_intersections = HashSet::new();
        profile!(scope: "physics", level: TRACE, "physics.intersections_with_shape", {
            // Only consider sensor colliders for player/sensor intersections.
            let sensor_filter = QueryFilter::new()
                .predicate(&|_collider_handle: ColliderHandle, _c: &Collider| _c.is_sensor());
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                sensor_filter,
            );
            for (handle, collider) in
                queries.intersect_shape(original_position, character_shape.as_ref())
            {
                if collider.is_sensor() {
                    if let (Some(_entity1_id), Some(entity2_id)) = (
                        EntityId::from_inner(character_user_data as u64),
                        EntityId::from_inner(collider.user_data as u64),
                    ) {
                        current_sensor_intersections.insert(entity2_id);
                    }
                }
                let _ = handle;
            }
        });

        let player_id = EntityId::from_inner(character_user_data as u64).unwrap();

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
        character_body.set_next_kinematic_translation(
            pos.translation.vector + mvt1.translation + mvt2.translation,
        );
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
        // Guard against degenerate rays. A zero-length direction normalizes to
        // NaN, and a NaN/zero ray direction sends parry's `clip_aabb_line` down
        // its `near_side == 0` path (see parry3d clip_aabb_line.rs): when the
        // ray origin also lies inside a collider AABB it returns face index 0,
        // and parry's feature math then computes `0u32 - 1`, which panics with
        // "attempt to subtract with overflow" in debug builds (issue #405) and
        // silently returns garbage hits in release. Reject such rays here - the
        // single choke point every raycast funnels through - rather than let a
        // bad caller crash the whole runtime.
        // Reject only truly non-normalizable input: an exactly-zero or
        // non-finite direction (both normalize to NaN) or a non-finite origin. A
        // tiny-but-nonzero direction still normalizes cleanly, so - unlike a
        // `< EPSILON` bound - this never rejects legitimate short rays such as
        // `ray_cast3`'s `end - start`.
        let norm_sq = direction.magnitude2();
        if !(start_point.x.is_finite() && start_point.y.is_finite() && start_point.z.is_finite())
            || !norm_sq.is_finite()
            || norm_sq == 0.0
        {
            tracing::debug!(
                "ray_cast2: rejecting degenerate ray (origin={:?}, direction={:?})",
                start_point,
                direction
            );
            return None;
        }
        let direction = direction / norm_sq.sqrt();
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
            Default::default(),
        ));

        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );

        if let Some((handle, intersection)) = queries.cast_ray_and_get_normal(&ray, max_toi, solid)
        {
            // This is similar to `QueryPipeline::cast_ray` illustrated above except
            // that it also returns the normal of the collider shape at the hit point.
            let hit_point = ray.point_at(intersection.time_of_impact);
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

    pub fn ray_cast3(
        &self,
        start_point: Point3<f32>,
        end_point: Point3<f32>,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
    ) -> Option<RayCastResult> {
        let direction = end_point - start_point;
        self.ray_cast2(
            start_point,
            direction,
            direction.magnitude(),
            collision_groups,
            entity_to_ignore,
            ignore_sensors,
        )
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

    // ============================================================================
    // Ragdoll Physics Utilities
    // ============================================================================

    /// Create a dynamic rigid body without requiring an EntityId
    /// Returns the handle for use in ragdoll systems
    pub fn create_dynamic_body(
        &mut self,
        isometry: Isometry<Real>,
        user_tag: Option<EntityId>,
    ) -> RigidBodyHandle {
        let mut rigid_body = RigidBodyBuilder::dynamic().pose(isometry).build();

        // Set user data if provided
        if let Some(entity_id) = user_tag {
            rigid_body.user_data = entity_id.inner() as u128;
        }

        self.rigid_body_set.insert(rigid_body)
    }

    /// Set linear and angular damping on a rigid body. Ragdoll limbs need
    /// non-zero damping (especially angular) so that a limb not in contact with
    /// the world bleeds off momentum and comes to rest, instead of spinning or
    /// flailing indefinitely.
    pub fn set_body_damping(&mut self, handle: RigidBodyHandle, linear: f32, angular: f32) {
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            body.set_linear_damping(linear);
            body.set_angular_damping(angular);
        }
    }

    /// Create a static (fixed) rigid body without requiring an EntityId
    /// Returns the handle for use in ragdoll systems
    pub fn create_static_body(
        &mut self,
        isometry: Isometry<Real>,
        user_tag: Option<EntityId>,
    ) -> RigidBodyHandle {
        let mut rigid_body = RigidBodyBuilder::fixed().pose(isometry).build();

        if let Some(entity_id) = user_tag {
            rigid_body.user_data = entity_id.inner() as u128;
        }

        self.rigid_body_set.insert(rigid_body)
    }

    /// Attach a collider to an existing rigid body
    pub fn attach_collider(
        &mut self,
        handle: RigidBodyHandle,
        shape: SharedShape,
        density: f32,
        collision_group: CollisionGroup,
    ) {
        self.attach_collider_with_offset(
            handle,
            shape,
            Vector3::new(0.0, 0.0, 0.0),
            density,
            collision_group,
        );
    }

    /// Attach a collider to a body with a local-space translation offset. Used by
    /// the ragdoll rig so a limb's collider can sit at its hitbox center rather
    /// than at the joint origin.
    pub fn attach_collider_with_offset(
        &mut self,
        handle: RigidBodyHandle,
        shape: SharedShape,
        offset: Vector3<f32>,
        density: f32,
        collision_group: CollisionGroup,
    ) {
        let collider = ColliderBuilder::new(shape)
            .density(density)
            .translation(vec_to_nvec(offset))
            .collision_groups(collision_group.0)
            .active_events(ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS)
            .build();

        self.collider_set
            .insert_with_parent(collider, handle, &mut self.rigid_body_set);
    }

    /// Create an impulse joint between two rigid bodies
    pub fn create_impulse_joint(
        &mut self,
        parent: RigidBodyHandle,
        child: RigidBodyHandle,
        joint_params: GenericJoint,
    ) -> ImpulseJointHandle {
        self.impulse_joint_set
            .insert(parent, child, joint_params, true)
    }

    /// Create a reduced-coordinate (multibody) joint between `parent` and `child`.
    /// Unlike an impulse joint, the constraint is structural - translation is not a
    /// DOF, so the bodies cannot separate, and no spring energy is injected. The
    /// child must currently be the root of its own multibody (each rigid body has at
    /// most one parent link); returns `None` if that invariant is violated (e.g. a
    /// duplicate edge / cycle). Removing either body (via `remove_rigid_body_handle`)
    /// also removes the joint.
    pub fn create_multibody_joint(
        &mut self,
        parent: RigidBodyHandle,
        child: RigidBodyHandle,
        joint_params: GenericJoint,
    ) -> Option<MultibodyJointHandle> {
        self.multibody_joint_set
            .insert(parent, child, joint_params, true)
    }

    /// Get the transform (position and rotation) of a rigid body by handle
    pub fn get_body_transform(&self, handle: RigidBodyHandle) -> Option<Isometry<Real>> {
        self.rigid_body_set.get(handle).map(|body| *body.position())
    }

    /// Linear and angular velocity of a rigid body by handle.
    pub fn body_velocities(&self, handle: RigidBodyHandle) -> Option<(Vector3<f32>, Vector3<f32>)> {
        self.rigid_body_set.get(handle).map(|body| {
            let l = body.linvel();
            let a = body.angvel();
            (Vector3::new(l.x, l.y, l.z), Vector3::new(a.x, a.y, a.z))
        })
    }

    /// World-space AABB enclosing all of a body's colliders (min, max).
    pub fn body_world_aabb(&self, handle: RigidBodyHandle) -> Option<(Vector3<f32>, Vector3<f32>)> {
        let body = self.rigid_body_set.get(handle)?;
        let mut min: Option<Vector3<f32>> = None;
        let mut max: Option<Vector3<f32>> = None;
        for collider_handle in body.colliders() {
            if let Some(collider) = self.collider_set.get(*collider_handle) {
                let aabb = collider.compute_aabb();
                let lo = Vector3::new(aabb.mins.x, aabb.mins.y, aabb.mins.z);
                let hi = Vector3::new(aabb.maxs.x, aabb.maxs.y, aabb.maxs.z);
                min = Some(match min {
                    Some(m) => Vector3::new(m.x.min(lo.x), m.y.min(lo.y), m.z.min(lo.z)),
                    None => lo,
                });
                max = Some(match max {
                    Some(m) => Vector3::new(m.x.max(hi.x), m.y.max(hi.y), m.z.max(hi.z)),
                    None => hi,
                });
            }
        }
        Some((min?, max?))
    }

    /// Scan every collider's world AABB for values that break physics queries:
    /// NaN/infinite bounds, degenerate (zero/negative) extents, or bounds far
    /// outside any plausible level. A single bad AABB can make raycasts return
    /// garbage or (in debug builds) panic inside parry3d, so this is the
    /// first-line check when a level misbehaves. Cheap enough to call on demand
    /// or per frame while diagnosing.
    pub fn audit_colliders(&self) -> Vec<ColliderIssue> {
        const EXTREME: f32 = 1.0e5;
        let mut issues = Vec::new();
        for (_handle, collider) in self.collider_set.iter() {
            let aabb = collider.compute_aabb();
            let mn = aabb.mins;
            let mx = aabb.maxs;
            let finite = mn.x.is_finite()
                && mn.y.is_finite()
                && mn.z.is_finite()
                && mx.x.is_finite()
                && mx.y.is_finite()
                && mx.z.is_finite();

            let kind = if !finite {
                Some(ColliderIssueKind::NonFinite)
            } else if mx.x <= mn.x || mx.y <= mn.y || mx.z <= mn.z {
                Some(ColliderIssueKind::Degenerate)
            } else if [mn.x, mn.y, mn.z, mx.x, mx.y, mx.z]
                .iter()
                .any(|c| c.abs() > EXTREME)
            {
                Some(ColliderIssueKind::Extreme)
            } else {
                None
            };

            if let Some(kind) = kind {
                let entity_id =
                    EntityId::from_inner(collider.user_data as u64).map(|id| id.inner() as i32);
                issues.push(ColliderIssue {
                    entity_id,
                    kind,
                    aabb_min: [mn.x, mn.y, mn.z],
                    aabb_max: [mx.x, mx.y, mx.z],
                    is_sensor: collider.is_sensor(),
                });
            }
        }
        issues
    }

    /// Enumerate every rigid body in the simulation for debug tooling.
    ///
    /// This iterates the raw Rapier `RigidBodySet` rather than the
    /// `entity_id_to_body` map, so it surfaces *all* bodies - including the
    /// many bodies that share a single `EntityId` (e.g. ragdoll limbs) and
    /// bodies with no entity at all.
    pub fn debug_list_bodies(&self) -> Vec<DebugBodyInfo> {
        self.rigid_body_set
            .iter()
            .map(|(handle, body)| self.debug_body_info(handle, body))
            .collect()
    }

    /// Look up a single body's debug info by its `body_id` (the rigid body
    /// handle index, as reported by [`debug_list_bodies`]).
    pub fn debug_body_detail(&self, body_id: u32) -> Option<DebugBodyInfo> {
        self.rigid_body_set
            .iter()
            .find(|(handle, _)| handle.into_raw_parts().0 == body_id)
            .map(|(handle, body)| self.debug_body_info(handle, body))
    }

    /// Enumerate every impulse joint with its anchor separation and applied
    /// impulse, for ragdoll diagnostics. A healthy ball joint at rest has
    /// `separation ≈ 0` and a small impulse; a persistent separation/impulse
    /// means the constraint can't be satisfied (the rig fights itself).
    pub fn debug_list_joints(&self) -> Vec<DebugJointInfo> {
        self.impulse_joint_set
            .iter()
            .filter_map(|(_handle, joint)| {
                let b1 = self.rigid_body_set.get(joint.body1)?;
                let b2 = self.rigid_body_set.get(joint.body2)?;
                let a1 = b1.position() * joint.data.local_frame1;
                let a2 = b2.position() * joint.data.local_frame2;
                let separation = (a1.translation.vector - a2.translation.vector).norm();
                // impulses: first 3 components are linear (translation), last 3 angular.
                let imp = joint.impulses;
                let linear_impulse = (imp[0] * imp[0] + imp[1] * imp[1] + imp[2] * imp[2]).sqrt();
                let angular_impulse = if imp.len() >= 6 {
                    (imp[3] * imp[3] + imp[4] * imp[4] + imp[5] * imp[5]).sqrt()
                } else {
                    0.0
                };
                Some(DebugJointInfo {
                    body1_id: joint.body1.into_raw_parts().0,
                    body2_id: joint.body2.into_raw_parts().0,
                    anchor1: [a1.translation.x, a1.translation.y, a1.translation.z],
                    anchor2: [a2.translation.x, a2.translation.y, a2.translation.z],
                    separation,
                    linear_impulse,
                    angular_impulse,
                })
            })
            .collect()
    }

    fn debug_body_info(&self, handle: RigidBodyHandle, body: &RigidBody) -> DebugBodyInfo {
        let (index, generation) = handle.into_raw_parts();

        let entity_id = if body.user_data != 0 {
            Some(body.user_data as i32)
        } else {
            None
        };

        let body_type = match body.body_type() {
            RigidBodyType::Dynamic => "dynamic",
            RigidBodyType::Fixed => "static",
            RigidBodyType::KinematicPositionBased | RigidBodyType::KinematicVelocityBased => {
                "kinematic"
            }
        };

        let translation = body.translation();
        let rotation = body.rotation();
        let linvel = body.linvel();
        let angvel = body.angvel();
        let com = body.center_of_mass();

        // Pull shape/group/sensor data from the body's first collider, if any.
        let mut collision_groups = Vec::new();
        let mut is_sensor = false;
        if let Some(collider_handle) = body.colliders().first() {
            if let Some(collider) = self.collider_set.get(*collider_handle) {
                is_sensor = collider.is_sensor();
                collision_groups =
                    collision_group_names(collider.collision_groups().memberships.bits());
            }
        }

        DebugBodyInfo {
            body_id: index,
            generation,
            entity_id,
            body_type,
            position: [translation.x, translation.y, translation.z],
            rotation: [rotation.i, rotation.j, rotation.k, rotation.w],
            linear_velocity: [linvel.x, linvel.y, linvel.z],
            angular_velocity: [angvel.x, angvel.y, angvel.z],
            mass: body.mass(),
            center_of_mass: [com.x, com.y, com.z],
            gravity_scale: body.gravity_scale(),
            linear_damping: body.linear_damping(),
            angular_damping: body.angular_damping(),
            collision_groups,
            is_sensor,
            is_enabled: body.is_enabled(),
            is_sleeping: body.is_sleeping(),
        }
    }
}

/// Rapier-free description of an impulse joint, for ragdoll diagnostics.
#[derive(Debug, Clone)]
pub struct DebugJointInfo {
    pub body1_id: u32,
    pub body2_id: u32,
    /// World anchor on each body (should coincide for a satisfied ball joint).
    pub anchor1: [f32; 3],
    pub anchor2: [f32; 3],
    /// Distance between the two anchors - the translation-constraint violation.
    pub separation: f32,
    /// Magnitude of the linear (translation) constraint impulse this step.
    pub linear_impulse: f32,
    /// Magnitude of the angular (limit) constraint impulse this step.
    pub angular_impulse: f32,
}

/// A malformed collider found by [`PhysicsWorld::audit_colliders`]. Bad
/// collider AABBs feed garbage into the physics queries (parry3d's ray-AABB
/// math can even integer-overflow on a degenerate box in debug builds), so
/// this is a data-hygiene check surfaced for any level on demand.
#[derive(Debug, Clone)]
pub struct ColliderIssue {
    /// Owning entity (from `Collider::user_data`), if any.
    pub entity_id: Option<i32>,
    /// What's wrong with the collider's world AABB.
    pub kind: ColliderIssueKind,
    /// The offending world-space AABB (min, max) - may contain NaN/inf.
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
    pub is_sensor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColliderIssueKind {
    /// An AABB bound is NaN or infinite - poisons every query it touches.
    NonFinite,
    /// Zero (or negative) extent on some axis - a flat/degenerate box.
    Degenerate,
    /// Bounds far outside any plausible level extent (|coord| > 1e5) -
    /// usually an entity that fell out of the world or a bad transform.
    Extreme,
}

impl ColliderIssueKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ColliderIssueKind::NonFinite => "non_finite",
            ColliderIssueKind::Degenerate => "degenerate",
            ColliderIssueKind::Extreme => "extreme",
        }
    }
}

/// Rapier-free description of a rigid body, for debug tooling / HTTP introspection.
#[derive(Debug, Clone)]
pub struct DebugBodyInfo {
    /// Stable-within-session id: the rigid body handle's index.
    pub body_id: u32,
    /// Handle generation - distinguishes a reused index across removals.
    pub generation: u32,
    /// Owning entity (from `RigidBody::user_data`). Non-unique: many bodies
    /// (e.g. ragdoll limbs) can report the same entity.
    pub entity_id: Option<i32>,
    pub body_type: &'static str,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub mass: f32,
    pub center_of_mass: [f32; 3],
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub collision_groups: Vec<String>,
    pub is_sensor: bool,
    pub is_enabled: bool,
    pub is_sleeping: bool,
}

/// Decode an `InteractionGroups` membership bitmask into human-readable names.
fn collision_group_names(bits: u32) -> Vec<String> {
    let mut names = Vec::new();
    let candidates = [
        (InternalCollisionGroups::WORLD, "world"),
        (InternalCollisionGroups::ENTITY, "entity"),
        (InternalCollisionGroups::SELECTABLE, "selectable"),
        (InternalCollisionGroups::PLAYER, "player"),
        (InternalCollisionGroups::UI, "ui"),
        (InternalCollisionGroups::HITBOX, "hitbox"),
        (InternalCollisionGroups::RAYCAST, "raycast"),
    ];
    for (group, name) in candidates {
        if bits & group.bits != 0 {
            names.push(name.to_string());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    //! Verify that per-object `DynamicPhysicsOptions` actually drive the Rapier
    //! simulation. These are deterministic, headless physics tests (fixed 1/60
    //! step, no mission/asset load) so an agent can confirm the plumbing.
    use super::*;
    use cgmath::{Quaternion, vec3};

    fn identity_quat() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }

    /// A world with a large static floor whose top surface is at `y = 0`, plus a
    /// throwaway player far away (so it never interacts with the test bodies but
    /// satisfies `update`'s signature).
    fn world_with_floor() -> (PhysicsWorld, PlayerHandle) {
        let mut world = PhysicsWorld::new();
        let floor = world.create_static_body(
            Isometry::translation(0.0, -1.0, 0.0),
            EntityId::from_inner(1000),
        );
        world.attach_collider(
            floor,
            SharedShape::cuboid(100.0, 1.0, 100.0),
            1.0,
            CollisionGroup::entity(),
        );
        let player = world.create_player(
            vec3(1000.0, 1000.0, 1000.0),
            EntityId::from_inner(1001).unwrap(),
        );
        (world, player)
    }

    fn step(world: &mut PhysicsWorld, player: &mut PlayerHandle, frames: usize) {
        for _ in 0..frames {
            world.update(Vector3::new(0.0, 0.0, 0.0), player);
        }
    }

    /// A higher-elasticity object must rebound higher than a low-elasticity one
    /// dropped from the same height. (Negative-first: before `add_dynamic`
    /// honored `opts.restitution`, both used the hardcoded 0.7 and reached the
    /// same height, so this assertion failed.)
    #[test]
    fn higher_restitution_bounces_higher() {
        let (mut world, mut player) = world_with_floor();

        let low = world.add_dynamic(
            EntityId::from_inner(1).unwrap(),
            vec3(-5.0, 5.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            PhysicsShape::Sphere(0.5),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions {
                restitution: 0.1,
                ..Default::default()
            },
        );
        let high = world.add_dynamic(
            EntityId::from_inner(2).unwrap(),
            vec3(5.0, 5.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            PhysicsShape::Sphere(0.5),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions {
                restitution: 0.95,
                ..Default::default()
            },
        );

        // Both balls fall identically (~58 frames to first impact from y=5).
        // Measure the rebound apex *after* that first contact, so the shared
        // initial drop height doesn't mask the difference.
        const SETTLE_FRAMES: usize = 70;
        let mut low_peak = f32::MIN;
        let mut high_peak = f32::MIN;
        for frame in 0..300 {
            step(&mut world, &mut player, 1);
            if frame >= SETTLE_FRAMES {
                low_peak = low_peak.max(world.get_position(low).unwrap().y);
                high_peak = high_peak.max(world.get_position(high).unwrap().y);
            }
        }
        assert!(
            high_peak > low_peak + 0.1,
            "high-restitution apex {high_peak} should exceed low-restitution apex {low_peak}"
        );
    }

    /// A degenerate ray (zero-length or non-finite direction) whose origin sits
    /// inside a collider AABB must return `None`, not panic. (Negative-first:
    /// without the guard in `ray_cast2`, parry's `clip_aabb_line` returns face
    /// index 0 for this case and the feature math computes `0u32 - 1`, panicking
    /// with "attempt to subtract with overflow" in debug builds - issue #405.)
    #[test]
    fn degenerate_ray_does_not_panic() {
        let (mut world, mut player) = world_with_floor();
        // A dynamic *cuboid* the query pipeline will actually see (static bodies
        // don't enter the query BVH until they move). The shape must be a cuboid:
        // parry's face-index overflow is in the ray-vs-box path; a ball uses a
        // different ray cast and never hits it. AABB ~ [-1, 1, -1] .. [1, 3, 1].
        world.add_dynamic(
            EntityId::from_inner(42).unwrap(),
            vec3(0.0, 2.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        step(&mut world, &mut player, 1);

        // Origin inside the cuboid's AABB - the exact condition that makes
        // parry's `clip_aabb_line` return face index 0 for a degenerate ray.
        let origin = point3(0.0, 2.0, 0.0);

        let zero_dir = world.ray_cast2(
            origin,
            Vector3::new(0.0, 0.0, 0.0),
            100.0,
            InternalCollisionGroups::ALL_COLLIDABLE,
            None,
            false,
        );
        assert!(zero_dir.is_none(), "zero-direction ray should return None");

        let nan_dir = world.ray_cast2(
            origin,
            Vector3::new(f32::NAN, 0.0, 0.0),
            100.0,
            InternalCollisionGroups::ALL_COLLIDABLE,
            None,
            false,
        );
        assert!(nan_dir.is_none(), "NaN-direction ray should return None");

        // Regression guard: a valid ray must still hit (the check must reject
        // only degenerate rays, not good ones).
        let valid = world.ray_cast2(
            point3(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            100.0,
            InternalCollisionGroups::ALL_COLLIDABLE,
            None,
            false,
        );
        assert!(valid.is_some(), "valid downward ray should hit the cuboid");
    }

    /// A higher-friction box, given the same initial horizontal velocity on the
    /// floor, must travel less far than a low-friction one. (Negative-first:
    /// before `add_dynamic` honored `opts.friction`, both used Rapier's default
    /// and traveled the same distance.)
    #[test]
    fn higher_friction_slides_less() {
        let (mut world, mut player) = world_with_floor();

        let make_box = |world: &mut PhysicsWorld, id: u64, z: f32, friction: f32| {
            let entity = EntityId::from_inner(id).unwrap();
            let handle = world.add_dynamic(
                entity,
                vec3(0.0, 0.5, z),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
                CollisionGroup::entity(),
                false,
                DynamicPhysicsOptions {
                    friction,
                    ..Default::default()
                },
            );
            // Slide, don't tumble: isolate sliding friction from rolling.
            world.set_enabled_rotations(entity, false, false, false);
            world.set_velocity(entity, vec3(10.0, 0.0, 0.0));
            handle
        };

        let slippery = make_box(&mut world, 1, -5.0, 0.0);
        let grippy = make_box(&mut world, 2, 5.0, 1.0);

        step(&mut world, &mut player, 120);

        let slippery_x = world.get_position(slippery).unwrap().x;
        let grippy_x = world.get_position(grippy).unwrap().x;
        assert!(
            slippery_x > grippy_x + 1.0,
            "low-friction box ({slippery_x}) should out-slide high-friction box ({grippy_x})"
        );
    }
}
