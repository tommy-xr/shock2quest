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
    na,
    na::UnitQuaternion,
    prelude::*,
};
use shipyard::EntityId;

use physics_events::*;

use self::debug_render_pipeline::DebugRenderer;

/// Player capsule dimensions (SS2 ft): total height and radius. Kept at the
/// old cuboid's footprint (4.8 tall, 1.6 wide) - resizing toward the original
/// engine's 6.0 x 2.4 player is deferred until crouch can shrink the collider.
const PLAYER_HEIGHT: f32 = 4.8;
const PLAYER_RADIUS: f32 = 0.8;

/// Crouched capsule height (SS2 ft). The original engine's crouched COLLISION
/// profile (a stack of two 1.2 ft spheres, body-bottom -1.8 to head-top +1.0
/// around the object origin) is ~2.8 ft tall - its taller "crouch height" is
/// only the camera. The authored crawl routes are built for that: the MedSci
/// air shaft behind keypad 45100 chokes to ~3.1 ft between its floor grate
/// and entrance lip, which a 2.8 ft capsule plus the 0.1 ft contact offset
/// clears with margin while 3.0+ scrapes.
const PLAYER_CROUCH_HEIGHT: f32 = 2.8;

/// Margin (SS2 ft) for the stand-up headroom test capsule: its radius is
/// shrunk by this and its pose lifted by it, keeping the test top exactly at
/// the standing crown while floating the test bottom off the floor. Without
/// it, grazing contacts (the floor rest gap, walls the crouched capsule
/// already touches) would falsely refuse standing; anything the lateral
/// shrink lets through is well inside the controller's contact offset and
/// resolves over the next frames.
const PLAYER_STAND_TEST_MARGIN: f32 = 0.05;

/// How far (world units) the collider CENTER sits below its standing height
/// while crouched (the feet stay planted while the capsule shrinks). Save
/// code uses this to store a standing-equivalent center so a game saved
/// while crouched doesn't reload a standing capsule embedded in the floor.
pub fn player_crouch_center_shift() -> f32 {
    (PLAYER_HEIGHT - PLAYER_CROUCH_HEIGHT) / 2.0 / SCALE_FACTOR
}

/// Gap (SS2 ft) the character controller keeps between the player collider
/// and world geometry (`KinematicCharacterController::offset`).
const PLAYER_CONTACT_OFFSET: f32 = 0.1;

/// Extra height (SS2 ft) the player is lifted each grounded frame, keeping the
/// resting gap a hair above `PLAYER_CONTACT_OFFSET`. Load-bearing: movement
/// casts use `PLAYER_CONTACT_OFFSET` as their target distance, so a capsule
/// resting at exactly that gap starts every cast already "in contact" with
/// its own floor. On a yaw-ROTATED support (e.g. the earth.mis tram floor
/// slab) the rotated top-face normal carries ~1e-6 float error, which makes
/// even purely tangential walking read as "approaching" - every solver
/// iteration then re-hits the same contact at toi=0, applies zero
/// translation, and the player freezes in place. (Axis-aligned floors yield
/// an exact (0,1,0) normal, where tangential motion reports no hit - which is
/// why flat test floors work without this.) The next frame's gravity pass
/// consumes the lift again, so the rest height is stable. Regression-tested
/// by `player_walks_on_rotated_platform`.
const PLAYER_REST_LIFT: f32 = 0.01;

/// Maximum ledge height (SS2 ft) the player steps up automatically, and how
/// far below their feet the ground is snapped to when walking down. 2 ft is
/// the original engine's step-probe height, so stairs climb and descend
/// without a jump.
const PLAYER_STEP_HEIGHT: f32 = 2.0;

/// Horizontal reach (world units) for ladder detection: the player grips a
/// climbable surface when their collider, inflated by this much radially,
/// overlaps it. Standing flush against a ladder leaves a small gap between the
/// collider and the rungs, so the un-inflated shapes never intersect.
const CLIMB_REACH: f32 = 1.0 / SCALE_FACTOR;

/// Climb speed as a fraction of walk speed (the into-ladder input component is
/// redirected to vertical movement at this scale).
const CLIMB_SPEED_SCALE: f32 = 0.6;

/// Half-Life-style flat ladder movement: convert the into-ladder component of
/// the desired movement into a vertical climb. `toward_ladder` is the
/// horizontal unit **face normal** from the player toward the climbable
/// surface (from the contact query - NOT the collider-center direction, which
/// gains a lateral component whenever the player is off-center and steers the
/// player sideways off the ladder). Returns `None` when the player is not
/// pushing toward the ladder (no grip - normal walking/gravity applies).
/// Looking down (a downward-pitched desired movement) descends instead of
/// ascending, so the same input walks down a shaft ladder; the sign flip at
/// the pitch threshold matches classic ladder feel.
///
/// The into-ladder component is removed from the horizontal movement: the
/// character controller's slope limiting treats a vertical climbable face as
/// an unclimbable slope and cancels the ascent when the player also pushes
/// into it (empirically: ascent drops from ~7u to ~0.3u over 240 frames).
/// Lateral (along-ladder) movement is preserved. The vertical component of
/// `desired` (head pitch / debug fly channel) is dropped so climb speed
/// depends only on the into-ladder push.
fn climb_redirect(desired: Vector<Real>, toward_ladder: Vector<Real>) -> Option<Vector<Real>> {
    let desired_h = vector![desired.x, 0.0, desired.z];
    let into = desired_h.dot(&toward_ladder);
    if into <= 1e-6 {
        return None;
    }
    let vertical = if desired.y < -0.25 * into {
        -into
    } else {
        into
    };
    Some(desired_h - toward_ladder * into + Vector::y() * vertical * CLIMB_SPEED_SCALE)
}

/// Step-up probe (the original engine's stair-climbing approach: probe up,
/// forward, then down from the blocked position). Called when the player's
/// horizontal movement was mostly blocked; returns the extra translation that
/// hops the player onto a stair-sized ledge ahead, or `None` when there is no
/// steppable ledge (a full wall, no headroom, or a too-tall/too-steep step).
///
/// This exists because rapier's built-in autostep never fires against a step
/// with a capsule: the step's top edge contacts the bottom sphere above its
/// center, which parry classifies as a ceiling-ish hit, not a wall.
///
/// `pos` is the collider pose after the blocked move; `desired` the movement
/// input for the frame; `applied` the translation the move actually achieved.
/// The probe (all casts collision-checked, so the result is a valid pose):
/// 1. headroom: the capsule must fit `PLAYER_STEP_HEIGHT` straight up;
/// 2. clearance: at that height it must fit forward far enough to plant the
///    capsule axis past the riser face (radius + contact gaps) - otherwise
///    the overhanging capsule gets pulled back down by snap-to-ground;
/// 3. tread: dropping back down must land on a walkable (mostly-horizontal)
///    surface above the starting feet - the landing defines the step height.
fn try_step_up(
    queries: &QueryPipeline,
    shape: &dyn Shape,
    pos: &Isometry<Real>,
    desired: Vector<Real>,
    applied: Vector<Real>,
) -> Option<Vector<Real>> {
    let desired_h = vector![desired.x, 0.0, desired.z];
    let desired_norm = desired_h.norm();
    if desired_norm < 1.0e-6 {
        return None;
    }
    let dir = desired_h / desired_norm;
    // Not blocked: the move achieved most of the desired horizontal distance.
    if applied.dot(&dir) > 0.5 * desired_norm {
        return None;
    }

    let step_height = PLAYER_STEP_HEIGHT / SCALE_FACTOR;
    let contact_offset = PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
    // Far enough forward that the capsule axis (its lowest point) stands on
    // the tread: the capsule radius, the gap to the riser (which can exceed
    // the contact offset when the walk stalled early), and margin on top.
    let forward = PLAYER_RADIUS / SCALE_FACTOR + 4.0 * contact_offset;
    // `target_distance` counts lateral grazes (e.g. the riser edge the player
    // is pressed against) as immediate hits, so the clearance casts (up,
    // forward) use 0; only the landing cast keeps the contact offset so the
    // player comes to rest at the normal gap above the tread.
    let cast = |from: &Isometry<Real>, dir: Vector<Real>, max_dist: f32, target: f32| {
        queries.cast_shape(
            from,
            &dir,
            shape,
            rapier3d::parry::query::ShapeCastOptions {
                max_time_of_impact: max_dist,
                target_distance: target,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            },
        )
    };

    // 1) Headroom directly above.
    if cast(pos, Vector::y(), step_height, 0.0).is_some() {
        return None;
    }
    // 2) Forward clearance at the lifted height.
    let lifted = Translation::from(Vector::y() * step_height) * pos;
    if cast(&lifted, dir, forward, 0.0).is_some() {
        return None;
    }
    // 3) Drop onto the tread.
    let planted = Translation::from(dir * forward) * lifted;
    let (_, hit) = cast(&planted, -Vector::y(), step_height, contact_offset)?;
    let lift = step_height - hit.time_of_impact;
    // Too small to matter (the rounded capsule bottom slides over it anyway),
    // or a downward/steep landing normal (not a tread). The threshold sits
    // just above cos(45 deg) so the probe can't hop up slopes the controller's
    // slope limit (default 45 deg) refuses to walk.
    if lift < 0.05 / SCALE_FACTOR || hit.normal1.y < 0.72 {
        return None;
    }
    Some(Vector::y() * lift + dir * forward)
}

/// Maximum distance (world units) a single validated player move may advance.
/// A request for a farther target is clamped to this, so an automated tester
/// navigates in short, collision-checked hops instead of one long teleport.
pub const MAX_PLAYER_MOVE_DISTANCE: f32 = 5.0;

/// Skin margin (world units) subtracted from the shape-cast time-of-impact so
/// the player stops just short of the geometry it hit rather than flush against
/// (or slightly inside) it.
const PLAYER_MOVE_SKIN_MARGIN: f32 = 0.1;

bitflags! {
    pub struct InternalCollisionGroups: u32 {
        const WORLD = 1 << 0; // 1
        const ENTITY = 1 << 1; // 2
        const SELECTABLE = 1 << 2; // 2
        const PLAYER = 1 << 3;
        const UI = 1 << 4;
        const HITBOX = 1 << 5;
        const RAYCAST = 1 << 6;
        // Marker membership for climbable surfaces (PropPhysAttr.climbable != 0,
        // e.g. ladders). Nothing filters on it for collision - it exists so the
        // player movement code can query "am I touching a ladder?" cheaply.
        const CLIMBABLE = 1 << 7;
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

#[derive(Clone, Copy)]
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

    /// Same collision behavior as `entity()`, plus the `CLIMBABLE` marker
    /// membership so player movement can detect ladder contact (see
    /// `PropPhysAttr.climbable`).
    pub fn climbable_entity() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: (InternalCollisionGroups::ENTITY.bits
                | InternalCollisionGroups::CLIMBABLE.bits)
                .into(),
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

    /// Ragdoll limbs that collide with the world but NOT with each other (nor
    /// other selectables). Used for death-handoff rigs spawned in a crumpled,
    /// limb-overlapping pose: the many simultaneous deep limb-limb contacts
    /// there can drive the articulated (multibody) solve to non-finite
    /// positions in a single step. Floor contact is what matters for a lying
    /// corpse; limb self-collision is cosmetic in that pose.
    pub fn ragdoll_no_self() -> CollisionGroup {
        CollisionGroup(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: InternalCollisionGroups::WORLD.bits.into(),
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

/// Result of a bounded, shape-cast-validated player move (see
/// [`PhysicsWorld::move_player_validated`]).
#[derive(Clone, Debug)]
pub struct MoveResult {
    /// Whether the player position actually changed.
    pub moved: bool,
    /// Whether the shape cast hit geometry before the full clamped distance,
    /// stopping the move short of the target.
    pub blocked: bool,
    /// The player's new world position after the move.
    pub new_position: Vector3<f32>,
    /// How far the player actually advanced (world units).
    pub distance_moved: f32,
    /// The distance the move was allowed to attempt this call: `min(target
    /// distance, MAX_PLAYER_MOVE_DISTANCE)`.
    pub requested_distance: f32,
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
    // Whether the collider is currently the crouched capsule. Mutated only by
    // `PhysicsWorld::set_player_crouch`, which keeps the shape and this flag
    // in sync.
    is_crouched: bool,
}

impl PlayerHandle {
    /// Whether the player collider is currently the crouched capsule. This is
    /// the *actual* state (stand-up can be refused for lack of headroom), not
    /// the requested input.
    pub fn is_crouched(&self) -> bool {
        self.is_crouched
    }
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
            // Clamp up as well as down: a tiny-but-positive size (e.g. medsci1's
            // "Lift 1 Walls" wall segment) yields a point-like AABB, and parry's
            // debug-build ray-AABB test overflows on those (FeatureId::Face(0 - 1))
            // - any AI vision/ground probe crossing it panics the game thread.
            v.clamp(MIN_SIZE, MAX_SIZE)
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

/// Scalar variant of [`sanitize_collider_size`] for ball colliders. A zero
/// radius (e.g. medsci1's "Lift 1 Walls": a dynamic SPHERE with radius 0)
/// yields a point AABB, and rapier 0.31's BVH broad-phase build panics on
/// zero/non-finite AABBs (parry `bvh_binned_build` "index out of bounds") -
/// the crash is nondeterministic because it depends on the bin layout around
/// the degenerate AABB.
fn sanitize_collider_radius(entity_id: EntityId, context: &str, radius: f32) -> f32 {
    const MIN_RADIUS: f32 = 0.005;
    const MAX_RADIUS: f32 = 5.0e3;
    let out = if radius.is_finite() && radius > 0.0 {
        radius.clamp(MIN_RADIUS, MAX_RADIUS)
    } else {
        MIN_RADIUS
    };
    if out != radius {
        tracing::warn!(
            "[physics] {} entity {:?}: invalid collider radius {} -> clamped to {}",
            context,
            entity_id,
            radius,
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

    /// Move the player toward `target`, but bounded and collision-validated.
    ///
    /// The displacement `target - current` is clamped to at most
    /// [`MAX_PLAYER_MOVE_DISTANCE`], then the player's character-controller
    /// collider is shape-cast along that direction. If it hits geometry before
    /// the clamped distance, the player stops just short of the contact
    /// (time-of-impact minus [`PLAYER_MOVE_SKIN_MARGIN`], clamped >= 0) and the
    /// result is marked `blocked`. Unlike `set_player_translation`, this can
    /// never move the player through a wall or out of bounds.
    pub fn move_player_validated(
        &mut self,
        target: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> MoveResult {
        let current = self.get_player_translation(player_handle);
        let delta = target - current;
        let dist = delta.magnitude();

        // Degenerate request (zero or non-finite): nothing to do. Guard before
        // computing `requested_distance` so a NaN target doesn't report a
        // bogus clamp value (`NaN.min(5.0) == 5.0`).
        if !dist.is_finite() || dist == 0.0 {
            return MoveResult {
                moved: false,
                blocked: false,
                new_position: current,
                distance_moved: 0.0,
                requested_distance: 0.0,
            };
        }

        // Distance we are allowed to attempt this call.
        let requested_distance = dist.min(MAX_PLAYER_MOVE_DISTANCE);
        let dir = delta / dist; // normalized direction

        // Snapshot the character shape + pose (cheap Arc clone) before building
        // the query pipeline, which borrows the body/collider sets. Mirrors the
        // pattern in `move_player`.
        //
        // Cast from the *body* pose, not the collider's cached pose: a kinematic
        // body's collider position is only re-synced during a physics step, so
        // after a prior `set_player_translation` (below) with no step in between
        // - e.g. two `move_player_validated` calls back to back - the collider
        // pose is stale and would restart the cast from the old spot, letting
        // the player tunnel through walls. The body's own `position()` updates
        // immediately. The collider is parented at the body origin (identity
        // local transform), so the body pose is the collider's true world pose.
        let character_body = &self.rigid_body_set[player_handle.character_handle];
        let character_collider = &self.collider_set[character_body.colliders()[0]];
        let character_shape = character_collider.shared_shape().clone();
        let character_pos = *character_body.position();

        // Same collision filter the real player movement uses: collide with the
        // collidable groups, ignore the player's own body and all sensors.
        let filter = QueryFilter::new()
            .groups(InteractionGroups::new(
                InternalCollisionGroups::PLAYER.bits.into(),
                InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
                Default::default(),
            ))
            .exclude_rigid_body(player_handle.character_handle)
            .exclude_sensors();

        // Shape-cast the character collider along the (normalized) direction.
        // With a unit velocity the time-of-impact is a distance in world units,
        // mirroring how `ray_cast2` treats `max_toi` as a distance.
        //
        // `stop_at_penetration: false` so a start that is already touching /
        // slightly penetrating geometry (e.g. after a raw `/v1/player/teleport`
        // dropped the player against a wall) doesn't return `toi == 0` and pin
        // the player as permanently `blocked` - a move *away* from the contact
        // (separating velocity) is then discarded at t=0 and proceeds normally,
        // while a move *into* it still blocks.
        let shape_vel = vector![dir.x, dir.y, dir.z];
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: requested_distance,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };

        let allowed_distance = {
            let queries = self.broad_phase.as_query_pipeline(
                self.narrow_phase.query_dispatcher(),
                &self.rigid_body_set,
                &self.collider_set,
                filter,
            );

            match queries.cast_shape(
                &character_pos,
                &shape_vel,
                character_shape.as_ref(),
                options,
            ) {
                Some((_handle, hit)) => Some(
                    (hit.time_of_impact - PLAYER_MOVE_SKIN_MARGIN).clamp(0.0, requested_distance),
                ),
                None => None,
            }
        };

        let (distance_moved, blocked) = match allowed_distance {
            Some(d) => (d, true),
            None => (requested_distance, false),
        };

        // When `distance_moved == 0`, `current + dir * 0` is exactly `current`.
        let new_position = current + dir * distance_moved;

        if distance_moved > 0.0 {
            self.set_player_translation(new_position, player_handle);
        }

        MoveResult {
            moved: distance_moved > 0.0,
            blocked,
            new_position,
            distance_moved,
            requested_distance,
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
                let radius = sanitize_collider_radius(entity_id, "add_dynamic", size);
                ColliderBuilder::ball(radius)
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
        // A capsule with the same footprint as the old cuboid: 4.8 SS2 ft tall,
        // 1.6 ft wide (the original engine's player is a stack of spheres - a
        // rounded shape slides cleanly along corners/seams the box snagged on).
        let mut collider = ColliderBuilder::capsule_y(
            (PLAYER_HEIGHT / 2.0 - PLAYER_RADIUS) / SCALE_FACTOR,
            PLAYER_RADIUS / SCALE_FACTOR,
        );
        collider = collider.collision_groups(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        ));
        collider = collider.user_data(player_entity_user_data);

        self.collider_set
            .insert_with_parent(collider, character_handle, &mut self.rigid_body_set);

        let mut controller = KinematicCharacterController::default();

        controller.offset = CharacterLength::Absolute(PLAYER_CONTACT_OFFSET / SCALE_FACTOR);
        // Walking down stairs stays grounded (snapped onto the next tread)
        // instead of chaining micro-falls. Deliberate tradeoff: this also
        // absorbs any intended drop up to the step height (the player glues
        // to <= 2 ft ledges rather than falling) - revisit when gravity
        // becomes an integrated velocity. Stepping UP is handled by an
        // explicit probe in `move_player` (see `try_step_up`) - rapier's
        // built-in autostep needs a wall-classified contact, but a capsule
        // touching a step edge above its bottom-sphere center reads as a
        // ceiling and never triggers it.
        controller.snap_to_ground =
            Some(CharacterLength::Absolute(PLAYER_STEP_HEIGHT / SCALE_FACTOR));

        self.entity_id_to_body
            .insert(player_entity, character_handle);

        PlayerHandle {
            controller,
            character_handle,
            is_crouched: false,
        }
    }

    /// Set the player's crouch state, resizing the capsule with the feet
    /// planted (the body translation is the collider center, so half the
    /// height difference is added/removed from it). Standing up is refused
    /// while there is not enough headroom for the standing capsule; the
    /// returned bool is the *resulting* crouch state.
    pub fn set_player_crouch(
        &mut self,
        want_crouch: bool,
        player_handle: &mut PlayerHandle,
    ) -> bool {
        if want_crouch == player_handle.is_crouched {
            return player_handle.is_crouched;
        }

        let character_handle = player_handle.character_handle;
        let collider_handle = self.rigid_body_set[character_handle].colliders()[0];
        // Feet-planted center shift between the two capsule sizes.
        let center_shift = (PLAYER_HEIGHT - PLAYER_CROUCH_HEIGHT) / 2.0 / SCALE_FACTOR;

        if want_crouch {
            let crouched = SharedShape::capsule_y(
                (PLAYER_CROUCH_HEIGHT / 2.0 - PLAYER_RADIUS) / SCALE_FACTOR,
                PLAYER_RADIUS / SCALE_FACTOR,
            );
            self.collider_set[collider_handle].set_shape(crouched);
            let body = &mut self.rigid_body_set[character_handle];
            let mut translation = *body.translation();
            translation.y -= center_shift;
            body.set_translation(translation, true);
            player_handle.is_crouched = true;
        } else {
            // Headroom check: intersect a test capsule at the feet-planted
            // standing pose against the same groups the movement casts use.
            // The test capsule keeps the full segment but shrinks the radius
            // by the margin and is lifted by the margin, so its TOP sits
            // exactly at the standing crown (a ceiling lower than standing
            // height always blocks) while its BOTTOM floats 2x the margin
            // above the standing feet (the floor/steps the player rests on
            // never falsely block).
            let standing_pos = Translation::from(
                Vector::y() * (center_shift + PLAYER_STAND_TEST_MARGIN / SCALE_FACTOR),
            ) * self.rigid_body_set[character_handle].position();
            let test_shape = Capsule::new_y(
                (PLAYER_HEIGHT / 2.0 - PLAYER_RADIUS) / SCALE_FACTOR,
                (PLAYER_RADIUS - PLAYER_STAND_TEST_MARGIN) / SCALE_FACTOR,
            );
            let filter = QueryFilter::new()
                .groups(InteractionGroups::new(
                    InternalCollisionGroups::PLAYER.bits.into(),
                    InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
                    Default::default(),
                ))
                .exclude_rigid_body(character_handle)
                .exclude_sensors();
            let dispatcher = self.narrow_phase.query_dispatcher();
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                filter,
            );
            let blocked = queries
                .intersect_shape(standing_pos, &test_shape)
                .next()
                .is_some();

            if !blocked {
                let standing = SharedShape::capsule_y(
                    (PLAYER_HEIGHT / 2.0 - PLAYER_RADIUS) / SCALE_FACTOR,
                    PLAYER_RADIUS / SCALE_FACTOR,
                );
                self.collider_set[collider_handle].set_shape(standing);
                let body = &mut self.rigid_body_set[character_handle];
                let mut translation = *body.translation();
                translation.y += center_shift;
                body.set_translation(translation, true);
                player_handle.is_crouched = false;
            }
        }

        player_handle.is_crouched
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

        // Joint-anchor overlay: each joint constrains a point on the parent
        // body (anchor1, cyan cross) to coincide with a point on the child
        // body (anchor2, magenta cross). A line connects them - a satisfied joint
        // has overlapping crosses and an invisible line; a sagging/separated joint
        // (e.g. the hips) shows a visible gap. Impulse and multibody joints both
        // draw (multibody anchors should always coincide - translation is not a
        // DOF there).
        let impulse_anchors = self
            .impulse_joint_set
            .iter()
            .filter_map(|(_h, joint)| {
                let b1 = self.rigid_body_set.get(joint.body1)?;
                let b2 = self.rigid_body_set.get(joint.body2)?;
                Some((
                    b1.position() * joint.data.local_frame1,
                    b2.position() * joint.data.local_frame2,
                ))
            })
            .collect::<Vec<_>>();
        let multibody_anchors = self
            .multibody_joint_anchor_pairs()
            .into_iter()
            .map(|(_, _, a1, a2)| (a1, a2));
        for (a1, a2) in impulse_anchors.into_iter().chain(multibody_anchors) {
            let p1 = Vector3::new(a1.translation.x, a1.translation.y, a1.translation.z);
            let p2 = Vector3::new(a2.translation.x, a2.translation.y, a2.translation.z);
            debug_renderer.add_cross(p1, 0.12, Vector3::new(0.0, 0.6, 1.0)); // anchor1 blue
            debug_renderer.add_cross(p2, 0.12, Vector3::new(1.0, 0.0, 1.0)); // anchor2 magenta
            debug_renderer.add_line(p1, p2, Vector3::new(1.0, 1.0, 0.0)); // gap (yellow)
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

        // In rapier 0.31 the `QueryPipeline` is a transient view built from the
        // broad-phase BVH, and it borrows the body/collider sets. Snapshot the
        // character shape (cheap Arc clone) and position up front so those
        // borrows are released before we build the query pipeline below.
        let character_shape = character_collider.shared_shape().clone();
        let character_pos = *character_collider.position();

        let mut gravity = -0.5 / SCALE_FACTOR;
        gravity *= self.rigid_body_set[player_handle.character_handle].gravity_scale();

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

        // Flat climbing: when the player overlaps a climbable surface (ladder)
        // and pushes toward it, redirect that input to vertical movement and
        // suppress the gravity pass for this frame (see `climb_redirect`).
        let climb_movement = {
            let climb_filter = QueryFilter::new()
                .groups(InteractionGroups::new(
                    InternalCollisionGroups::PLAYER.bits.into(),
                    InternalCollisionGroups::CLIMBABLE.bits.into(),
                    Default::default(),
                ))
                .exclude_rigid_body(player_handle.character_handle)
                .exclude_sensors();
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                climb_filter,
            );
            // Broad-phase candidate query: the capsule's bounds inflated by
            // CLIMB_REACH on x/z only (a cuboid, so the reach stays horizontal
            // - inflating the capsule radius would also extend the caps
            // vertically and grip ladders from above/below their ends).
            character_shape.as_capsule().and_then(|capsule| {
                let half_height = capsule.half_height() + capsule.radius;
                let inflated = Cuboid::new(vector![
                    capsule.radius + CLIMB_REACH,
                    half_height,
                    capsule.radius + CLIMB_REACH
                ]);
                // Grip the closest climbable within reach, by contact distance,
                // and take the contact's *face normal* as the climb direction.
                // (The collider-center direction is wrong when the player is
                // off-center: its lateral component steers the player sideways
                // off the ladder - a positive-feedback drift.)
                let mut nearest: Option<(f32, Vector<Real>)> = None;
                for (_handle, collider) in queries.intersect_shape(character_pos, &inflated) {
                    let contact = rapier3d::parry::query::contact(
                        &character_pos,
                        character_shape.as_ref(),
                        collider.position(),
                        collider.shape(),
                        CLIMB_REACH,
                    );
                    if let Ok(Some(contact)) = contact {
                        // normal1 points from the player toward the climbable.
                        let toward_h = vector![contact.normal1.x, 0.0, contact.normal1.z];
                        let toward_norm = toward_h.norm();
                        // A mostly-vertical normal means the player is on top of
                        // (or under) the surface - that's standing, not climbing.
                        if toward_norm > 0.5 && nearest.is_none_or(|(d, _)| contact.dist < d) {
                            nearest = Some((contact.dist, toward_h / toward_norm));
                        }
                    }
                }
                nearest.and_then(|(_, toward)| climb_redirect(desired_movement, toward))
            })
        };

        let mvt = profile!(scope: "physics", level: TRACE, "physics.move_player", {
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                movement_filter,
            );
            // Walk and gravity run as separate passes - NOT the old up-bump
            // hack (there is no artificial upward movement): a combined
            // walk+gravity cast points into the floor the player rests on,
            // which degenerates into zero-progress resting contacts (see
            // `PLAYER_REST_LIFT`). While gripping a ladder the climb vector
            // replaces both passes.
            let (walk, apply_gravity) = match climb_movement {
                Some(climb) => (climb, false),
                None => (desired_movement, true),
            };
            let mut mvt = player_handle.controller.move_shape(
                self.integration_parameters.dt,
                &queries,
                character_shape.as_ref(),
                &character_pos,
                walk,
                |_c| (),
            );
            if apply_gravity {
                let after_walk = Translation::from(mvt.translation) * character_pos;
                let fall = player_handle.controller.move_shape(
                    self.integration_parameters.dt,
                    &queries,
                    character_shape.as_ref(),
                    &after_walk,
                    Vector::y() * gravity,
                    |_c| (),
                );
                mvt.translation += fall.translation;
                mvt.grounded = fall.grounded;
                if mvt.grounded {
                    mvt.translation += Vector::y() * (PLAYER_REST_LIFT / SCALE_FACTOR);
                }
            }
            // Stairs: if grounded walking was blocked, probe for a step and
            // hop onto it. (Grounded-only: an airborne player pressed against
            // a wall must not ratchet up ledges.)
            if climb_movement.is_none() && mvt.grounded {
                if let Some(step) = try_step_up(
                    &queries,
                    character_shape.as_ref(),
                    &(Translation::from(mvt.translation) * character_pos),
                    desired_movement,
                    mvt.translation,
                ) {
                    mvt.translation += step;
                }
            }
            mvt
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

        let character_body = &mut self.rigid_body_set[player_handle.character_handle];
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
            if maybe_entity_id == entity_to_ignore {
                return false;
            }
            // A degenerate collider (zero-extent / non-finite AABB, e.g.
            // medsci1's point-sized "Lift 1 Walls") poisons parry's ray-AABB
            // clip the same way a degenerate ray does: face index 0 ->
            // `0u32 - 1` overflow panic in debug builds, NaN normals in
            // release. Skip such colliders - a point can't meaningfully block
            // a ray, and letting one through crashes AI vision/ground probes.
            // (`GET /v1/physics/colliders/validate` audits the data root
            // cause.)
            let aabb = collider.compute_aabb();
            aabb.mins.iter().all(|v| v.is_finite())
                && aabb.extents().iter().all(|e| e.is_finite() && *e > 1.0e-5)
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

    /// Raise a body's angular sleep threshold so residual solver noise (e.g. a
    /// ragdoll extremity buzzing against the floor) still counts as "at rest".
    /// One awake body keeps its whole jointed island awake, so without this a
    /// settled ragdoll never sleeps. Sleeping bodies auto-wake on contact or
    /// applied force, so the corpse stays interactive. (The linear threshold is
    /// left at rapier's default - the residual buzz is angular.)
    pub fn set_body_angular_sleep_threshold(&mut self, handle: RigidBodyHandle, angular: f32) {
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            body.activation_mut().angular_threshold = angular;
        }
    }

    /// Clamp ragdoll body velocities to finite, bounded magnitudes. The spawn
    /// transient of a ragdoll can (rarely, and nondeterministically across
    /// processes) enter a runaway feedback loop - a contact spike begets a
    /// bigger spike next frame until a position goes non-finite and the parry
    /// BVH broad-phase panics on the NaN AABB. Clamping once per frame breaks
    /// the cascade while leaving normal collapse dynamics (peaks well below
    /// the cap) untouched.
    ///
    /// Multibody-linked bodies clamp the owning multibody's generalized (DOF)
    /// velocities - writing a link body's linvel/angvel would be overwritten
    /// by the reduced-coordinate readback. Each multibody is clamped once no
    /// matter how many of its links appear in `handles`. Free bodies clamp
    /// linvel/angvel directly. Returns the number of corrected values.
    pub fn sanitize_ragdoll_bodies(&mut self, handles: &[RigidBodyHandle], max_speed: f32) -> u32 {
        let mut corrected = 0u32;
        let mut seen_multibodies = Vec::new();
        for handle in handles {
            if let Some(link) = self.multibody_joint_set.rigid_body_link(*handle) {
                let index = link.multibody;
                if seen_multibodies.contains(&index) {
                    continue;
                }
                seen_multibodies.push(index);
                if let Some(multibody) = self.multibody_joint_set.get_multibody_mut(index) {
                    for v in multibody.generalized_velocity_mut().iter_mut() {
                        if !v.is_finite() {
                            *v = 0.0;
                            corrected += 1;
                        } else if v.abs() > max_speed {
                            *v = v.clamp(-max_speed, max_speed);
                            corrected += 1;
                        }
                    }
                }
            } else if let Some(body) = self.rigid_body_set.get_mut(*handle) {
                let lin = *body.linvel();
                let ang = *body.angvel();
                if !lin.iter().all(|v| v.is_finite()) || lin.norm() > max_speed {
                    let new_lin = if lin.iter().all(|v| v.is_finite()) {
                        lin * (max_speed / lin.norm())
                    } else {
                        na::zero()
                    };
                    body.set_linvel(new_lin, false);
                    corrected += 1;
                }
                if !ang.iter().all(|v| v.is_finite()) || ang.norm() > max_speed {
                    let new_ang = if ang.iter().all(|v| v.is_finite()) {
                        ang * (max_speed / ang.norm())
                    } else {
                        na::zero()
                    };
                    body.set_angvel(new_ang, false);
                    corrected += 1;
                }
            }
        }
        corrected
    }

    /// Apply a world-space impulse to a dynamic body by its debug `body_id`
    /// (the rigid body handle index, as reported by [`debug_list_bodies`]),
    /// waking it even for a zero impulse (rapier skips a zero `apply_impulse`
    /// entirely, so the wake is explicit - `{"impulse":[0,0,0]}` is a pure
    /// wake). Debug/testing hook - e.g. poke a sleeping ragdoll to verify
    /// wake-on-impulse. Matches by handle index like the other debug-endpoint
    /// lookups (the generation isn't exposed over HTTP), so callers must use
    /// ids from a fresh body listing. Returns false if no dynamic body matches.
    pub fn apply_body_impulse(&mut self, body_id: u32, impulse: Vector3<f32>) -> bool {
        let handle = self
            .rigid_body_set
            .iter()
            .find(|(handle, _)| handle.into_raw_parts().0 == body_id)
            .map(|(handle, _)| handle);
        let Some(handle) = handle else {
            return false;
        };
        self.apply_impulse_to_handle(handle, impulse)
    }

    /// Seed a multibody's free-root velocity with a world-space linear
    /// velocity (rapier free-joint DOF layout: linear xyz at generalized
    /// indices 0..3, angular at 3..6 - see `MultibodyJoint::integrate`).
    /// Body-level `set_linvel` is clobbered by the reduced-coordinate
    /// readback, so inherited motion (e.g. a dying creature's root-motion
    /// velocity carrying into its ragdoll) must be written into the
    /// generalized coordinates. `body` may be any link of the multibody.
    /// Returns false for non-multibody bodies.
    pub fn set_multibody_root_linvel(
        &mut self,
        body: RigidBodyHandle,
        linvel: Vector3<f32>,
    ) -> bool {
        // Sanitize the seed: it gets a full physics step before the ragdoll's
        // per-frame velocity clamp sees it, so a non-finite or runaway value
        // (e.g. a capsule mid-knockback) must not reach the solver verbatim.
        const MAX_SEED_SPEED: f32 = 30.0;
        if !(linvel.x.is_finite() && linvel.y.is_finite() && linvel.z.is_finite()) {
            return false;
        }
        let norm = linvel.magnitude();
        let linvel = if norm > MAX_SEED_SPEED {
            linvel * (MAX_SEED_SPEED / norm)
        } else {
            linvel
        };
        let Some(link) = self.multibody_joint_set.rigid_body_link(body) else {
            // Not articulated (e.g. a one-bone skeleton spawns a lone free
            // body): a direct write works there.
            if let Some(rigid_body) = self.rigid_body_set.get_mut(body) {
                rigid_body.set_linvel(vec_to_nvec(linvel), true);
                return true;
            }
            return false;
        };
        let index = link.multibody;
        if let Some(multibody) = self.multibody_joint_set.get_multibody_mut(index) {
            let mut generalized = multibody.generalized_velocity_mut();
            if generalized.len() >= 3 {
                generalized[0] = linvel.x;
                generalized[1] = linvel.y;
                generalized[2] = linvel.z;
                return true;
            }
        }
        false
    }

    /// Multibody-aware impulse on a body handle, waking it even for a zero
    /// impulse. A multibody link ignores direct velocity writes: the reduced-
    /// coordinate solver recomputes every link body's velocity from the joint
    /// velocities each step (`Multibody` forward kinematics), so
    /// `apply_impulse` is silently overwritten. Its forward *dynamics* does
    /// read the per-body user-force accumulator, so convert the impulse to a
    /// force over one physics step - `clear_forces` (called right after each
    /// step) makes it impulsive. Free dynamic bodies get a plain impulse.
    pub fn apply_impulse_to_handle(
        &mut self,
        handle: RigidBodyHandle,
        impulse: Vector3<f32>,
    ) -> bool {
        let is_multibody_link = self.multibody_joint_set.rigid_body_link(handle).is_some();
        let dt = self.integration_parameters.dt;
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            if body.body_type() == RigidBodyType::Dynamic {
                body.wake_up(true);
                if is_multibody_link {
                    body.add_force(vec_to_nvec(impulse) / dt, true);
                    self.rigid_bodies_with_forces.push(handle);
                } else {
                    body.apply_impulse(vec_to_nvec(impulse), true);
                }
                return true;
            }
        }
        false
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
        let impulse = self
            .impulse_joint_set
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
                    joint_type: "impulse",
                    anchor1: [a1.translation.x, a1.translation.y, a1.translation.z],
                    anchor2: [a2.translation.x, a2.translation.y, a2.translation.z],
                    separation,
                    linear_impulse,
                    angular_impulse,
                })
            });
        let multibody = self
            .multibody_joint_anchor_pairs()
            .into_iter()
            .map(|(b1, b2, a1, a2)| DebugJointInfo {
                body1_id: b1.into_raw_parts().0,
                body2_id: b2.into_raw_parts().0,
                joint_type: "multibody",
                anchor1: [a1.translation.x, a1.translation.y, a1.translation.z],
                anchor2: [a2.translation.x, a2.translation.y, a2.translation.z],
                separation: (a1.translation.vector - a2.translation.vector).norm(),
                linear_impulse: 0.0,
                angular_impulse: 0.0,
            });
        impulse.chain(multibody).collect()
    }

    /// `(parent body, child body, world anchor on parent, world anchor on child)`
    /// for every multibody joint. Translation is structurally not a DOF for
    /// these, so the anchors should always coincide.
    fn multibody_joint_anchor_pairs(
        &self,
    ) -> Vec<(
        RigidBodyHandle,
        RigidBodyHandle,
        Isometry<Real>,
        Isometry<Real>,
    )> {
        self.multibody_joint_set
            .iter()
            .filter_map(|(_handle, _link_id, multibody, link)| {
                let parent = multibody.link(link.parent_id()?)?;
                let b1_handle = parent.rigid_body_handle();
                let b2_handle = link.rigid_body_handle();
                let b1 = self.rigid_body_set.get(b1_handle)?;
                let b2 = self.rigid_body_set.get(b2_handle)?;
                Some((
                    b1_handle,
                    b2_handle,
                    b1.position() * link.joint.data.local_frame1,
                    b2.position() * link.joint.data.local_frame2,
                ))
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

/// Rapier-free description of a joint, for ragdoll diagnostics.
#[derive(Debug, Clone)]
pub struct DebugJointInfo {
    pub body1_id: u32,
    pub body2_id: u32,
    /// `"impulse"` or `"multibody"` - which joint set this came from.
    pub joint_type: &'static str,
    /// World anchor on each body (should coincide for a satisfied ball joint).
    pub anchor1: [f32; 3],
    pub anchor2: [f32; 3],
    /// Distance between the two anchors - the translation-constraint violation.
    /// Structurally ~0 for multibody joints (translation is not a DOF there);
    /// a persistent gap on a multibody joint means the frame setup is wrong.
    pub separation: f32,
    /// Magnitude of the linear (translation) constraint impulse this step.
    /// Rapier does not expose applied impulses for multibody links, so 0 there.
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
        (InternalCollisionGroups::CLIMBABLE, "climbable"),
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

    // --- Flat ladder climbing (`climb_redirect` + CLIMBABLE detection) ---

    /// Pushing toward the ladder redirects the input to an ascent.
    #[test]
    fn climb_redirect_ascends_when_pushing_into_ladder() {
        let toward = vector![1.0, 0.0, 0.0];
        let climb = climb_redirect(vector![0.1, 0.0, 0.0], toward).expect("should grip");
        assert!(climb.y > 0.0, "expected upward redirect, got {climb:?}");
    }

    /// Pushing away from (or parallel to) the ladder does not grip.
    #[test]
    fn climb_redirect_ignores_push_away_or_parallel() {
        let toward = vector![1.0, 0.0, 0.0];
        assert!(climb_redirect(vector![-0.1, 0.0, 0.0], toward).is_none());
        assert!(climb_redirect(vector![0.0, 0.0, 0.1], toward).is_none());
        assert!(climb_redirect(vector![0.0, 0.0, 0.0], toward).is_none());
    }

    /// A downward-pitched push (looking down) descends instead of ascending.
    #[test]
    fn climb_redirect_descends_when_looking_down() {
        let toward = vector![1.0, 0.0, 0.0];
        let climb = climb_redirect(vector![0.1, -0.1, 0.0], toward).expect("should grip");
        assert!(climb.y < 0.0, "expected downward redirect, got {climb:?}");
    }

    /// A player standing at the base of a climbable wall who pushes into it
    /// climbs it; against an identical NON-climbable wall the same input leaves
    /// the player on the floor. (Negative-first: without the CLIMBABLE
    /// detection + redirect in `move_player`, both cases stay at floor height
    /// and the ascent assertion fails.)
    ///
    /// Also guards against lateral drift: the player starts OFF-CENTER on the
    /// wall (z = 0.5 on a 2-wide wall). With a collider-center-based climb
    /// direction the residual lateral term steers the player sideways along
    /// the wall while climbing; the contact face normal keeps the climb
    /// straight (with center-delta the ascent itself also collapses here).
    #[test]
    fn player_climbs_climbable_wall_but_not_plain_wall() {
        let run = |group: CollisionGroup| -> (f32, f32) {
            let mut world = PhysicsWorld::new();
            // Floor top at y=0, built the way the game builds level geometry
            // (a parentless collider): a fixed-BODY floor never enters the
            // query BVH the character controller moves against, so the player
            // would fall straight through it.
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            // Off-center on the wall's z-extent (see doc comment). The scene
            // sits at x=-6 so the floor's triangle seam (the x=z diagonal)
            // stays away from the walk path - crossing the seam produces a
            // lateral slide artifact unrelated to climbing.
            let mut player =
                world.create_player(vec3(-6.0, 1.0, 0.5), EntityId::from_inner(2000).unwrap());
            // A tall thin "ladder" wall just +x of the player, feet on the floor.
            world.add_kinematic(
                EntityId::from_inner(2001).unwrap(),
                vec3(-5.0, 5.0, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(0.2, 10.0, 2.0),
                group,
                false,
            );
            // Let the player settle onto the floor, then push into the wall.
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let start = world.get_player_translation(&player);
            for _ in 0..240 {
                world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
            }
            let end = world.get_player_translation(&player);
            (end.y - start.y, end.z - start.z)
        };

        let (climbable_ascent, climbable_drift) = run(CollisionGroup::climbable_entity());
        let (plain_ascent, _) = run(CollisionGroup::entity());

        assert!(
            climbable_ascent > 2.0,
            "pushing into a climbable wall should ascend it, rose {climbable_ascent}"
        );
        assert!(
            climbable_drift.abs() < 0.1,
            "climbing straight up must not drift sideways, drifted {climbable_drift}"
        );
        assert!(
            plain_ascent.abs() < 0.5,
            "a plain wall must not be climbable, rose {plain_ascent}"
        );
    }

    /// Walking into a stair-sized ledge steps up onto it; a too-tall ledge
    /// blocks. The step limit is 2 SS2 ft (0.8 wu), the original engine's
    /// step-probe height - a 1.5 ft riser climbs, a 3 ft ledge doesn't.
    /// (Negative-first: the old two-pass up-bump stepped at most
    /// `MOVEMENT_STEP_SIZE * dt` = 0.33 wu per frame, so the 0.6 wu riser
    /// failed before native autostep.)
    #[test]
    fn player_steps_up_stairs_but_not_tall_ledges() {
        // Height gained after walking +x into a `step_height`-tall platform.
        let run = |step_height: f32| -> f32 {
            let mut world = PhysicsWorld::new();
            // Floor top at y=0 (parentless trimesh, like level geometry).
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            let mut player =
                world.create_player(vec3(-6.0, 1.0, 0.0), EntityId::from_inner(2000).unwrap());
            // A platform ahead of the player whose top sits at `step_height`.
            world.add_kinematic(
                EntityId::from_inner(2001).unwrap(),
                vec3(-3.0, step_height / 2.0, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(4.0, step_height, 4.0),
                CollisionGroup::entity(),
                false,
            );
            // Settle onto the floor, then walk into the step; report the
            // highest point reached (a successful step-up crosses the platform
            // and walks off the far side, so the END height is floor level
            // either way).
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let start = world.get_player_translation(&player);
            let mut max_y = start.y;
            for _ in 0..240 {
                world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
                max_y = max_y.max(world.get_player_translation(&player).y);
            }
            max_y - start.y
        };

        let riser = run(0.6); // 1.5 SS2 ft - a typical stair riser
        let ledge = run(1.2); // 3.0 SS2 ft - over the 2 ft step limit

        assert!(
            riser > 0.5,
            "a 1.5 ft riser should be stepped up, rose {riser}"
        );
        assert!(
            ledge < 0.1,
            "a 3 ft ledge must not be auto-stepped, rose {ledge}"
        );
    }

    /// Walking along the top of a yaw-ROTATED kinematic cuboid (e.g. the
    /// earth.mis tram floor slab) must make progress. The rotated top-face
    /// normal carries ~1e-6 float error, so tangential movement casts read as
    /// "approaching" and re-hit the resting contact at toi=0 every solver
    /// iteration and the player freezes in place after a frame or two.
    /// (Negative-first: fails without `PLAYER_REST_LIFT`; an AXIS-ALIGNED
    /// slab passes either way because its exact (0,1,0) normal reports no hit
    /// for tangential motion.)
    #[test]
    fn player_walks_on_rotated_platform() {
        use cgmath::Rotation3;
        let mut world = PhysicsWorld::new();
        // A tram-slab-like platform, yawed 90 degrees: authored 4 wide x 10
        // long, so after rotation its long axis lies along world x.
        world.add_kinematic(
            EntityId::from_inner(1001).unwrap(),
            vec3(0.0, 1.0, 0.0),
            Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), cgmath::Deg(90.0)),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(4.0, 0.4, 10.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player =
            world.create_player(vec3(-3.0, 3.0, 0.0), EntityId::from_inner(2000).unwrap());
        // Settle onto the slab, then walk along it.
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        for _ in 0..120 {
            world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
        }
        let end = world.get_player_translation(&player);
        let walked = end.x - start.x;
        assert!(
            walked > 3.0,
            "walking on a rotated platform should progress ~6 units, moved {walked}"
        );
    }
}
