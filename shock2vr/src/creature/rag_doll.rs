use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, SquareMatrix, Vector3, vec3};
use dark::hit_box::HitBoxShape;
use dark::model::Model;
use engine::scene::SceneObject;
use rapier3d::{
    na::{Point3 as NaPoint3, Translation3, UnitQuaternion},
    prelude::{
        GenericJointBuilder, ImpulseJointHandle, Isometry, JointAxesMask, JointAxis,
        RigidBodyHandle, SharedShape, SpringCoefficients,
    },
};
use shipyard::EntityId;

use crate::{
    physics::{CollisionGroup, PhysicsWorld, util::quat_to_nquat},
    util::{get_position_from_matrix, get_rotation_from_matrix, point3_to_vec3},
};

use super::JointLimit;

const DEFAULT_JOINT_RADIUS: f32 = 0.06;
/// Minimum collider half-extent, so degenerate joint AABBs (e.g. a joint with a
/// single skinned vertex) still get a valid, non-zero box.
const MIN_HALF_EXTENT: f32 = DEFAULT_JOINT_RADIUS;
/// Linear/angular damping on ragdoll limb bodies so they bleed off momentum and
/// settle instead of spinning or flailing forever (limbs out of world contact
/// have nothing else to slow them).
const LINEAR_DAMPING: f32 = 0.5;
const ANGULAR_DAMPING: f32 = 2.0;
/// Target mass for every ragdoll limb body. Densities are derived per-collider to
/// hit this, keeping connected-body mass ratios near 1:1 for solver stability.
const TARGET_BODY_MASS: f32 = 1.0;
/// Mass for the cuboid "core" bodies (pelvis hub / abdomen / torso). Heavier than
/// the limbs so the core anchors them - the tiny pelvis hub otherwise gets yanked
/// around by the heavy thighs (the hip-sag / never-settle). Kept a modest ratio
/// (impulse joints destabilize at extreme connected-body mass ratios).
const CORE_BODY_MASS: f32 = 4.0;
/// Floor on collider volume when deriving density, to avoid div-by-zero / huge
/// density on degenerate shapes.
const MIN_VOLUME: f32 = 1e-4;
/// Uniform per-axis angular limit (radians, ~60°) for the limb ball joints,
/// measured from the bind/rest pose. Keeps the rig from folding/twisting through
/// itself. Per-bone limit profiles (hinge knees, cone shoulders) are a follow-up.
const JOINT_CONE_LIMIT: f32 = 1.05;
/// Angular sleep threshold for ragdoll bodies (rad/s). A settled extremity can
/// hold a low-grade contact buzz (~0.5-1 rad/s) that sits above rapier's
/// default angular threshold (0.5), and one awake body keeps the whole jointed
/// island awake - so the corpse never sleeps and the buzz never stops. Raising
/// it lets the island sleep once bulk motion is done (rapier needs the entire
/// island below thresholds for ~2s continuously, so a corpse in real motion
/// won't freeze); sleep zeroes the buzz, and contact or an applied impulse
/// wakes the corpse (it stays pokeable). Tradeoff: a limb rotating alone at up
/// to ~1.5 rad/s for 2s could hard-stop - lower toward 1.0 if that's ever
/// visible.
const SLEEP_ANGULAR_THRESHOLD: f32 = 1.5;
/// Hard cap on ragdoll body velocities (per generalized-DOF component for the
/// multibody rig; linvel/angvel norms for free bodies). Healthy collapse
/// dynamics peak around 7-20; the cap only engages when a spawn transient
/// enters runaway feedback (which otherwise ends in a NaN AABB and a parry
/// BVH panic). Applied once per frame in `RagDoll::update`.
const MAX_BODY_SPEED: f32 = 30.0;
/// Impulse magnitude (N*s, bodies are ~1 mass unit each) applied to the struck
/// limb when a death ragdoll is seeded with the killing blow. Tuned in medsci1
/// A/B runs: 8.0 gives the struck limb a clearly visible ~2.5 m/s reaction
/// that propagates through the articulated rig without launching the corpse.
const KILLING_BLOW_IMPULSE: f32 = 8.0;

/// Quality/settle metrics for one ragdoll, for the verification harness.
#[derive(Clone, Debug)]
pub struct RagDollMetrics {
    pub body_count: usize,
    /// Max body linear speed (a settled ragdoll trends to ~0).
    pub max_linear_speed: f32,
    /// Max body angular speed (catches "spinning forever" / divergence).
    pub max_angular_speed: f32,
    /// Lowest body position (floor penetration shows up as min_y below the floor).
    pub min_y: f32,
    /// Largest interpenetration depth between non-adjacent limb AABBs - the
    /// realism signal for self-collision work (lower is better).
    pub max_nonadjacent_overlap: f32,
    /// Largest distance any body has moved from where it spawned.
    pub max_drift: f32,
}

pub struct RagDoll {
    physics_bodies: Vec<RigidBodyHandle>,
    /// Impulse joints to clean up on removal. Empty for multibody rigs, whose
    /// joints are removed automatically with their bodies.
    joint_handles: Vec<ImpulseJointHandle>,
    joint_to_body: HashMap<u32, RigidBodyHandle>,
    bone_frame_offsets: HashMap<u32, Matrix4<f32>>,
    latest_global_transforms: [Matrix4<f32>; 40],
    scene_objects: Vec<SceneObject>,
    /// Directly jointed body pairs (parent/child), excluded from the
    /// non-adjacent-overlap metric since they are meant to overlap at the joint.
    joint_pairs: Vec<(RigidBodyHandle, RigidBodyHandle)>,
    /// World position each body was spawned at, for the drift / pose-continuity
    /// metric.
    spawn_positions: HashMap<RigidBodyHandle, Vector3<f32>>,
}

impl RagDoll {
    #[allow(clippy::too_many_arguments)]
    fn new(
        joint_to_body: HashMap<u32, RigidBodyHandle>,
        physics_bodies: Vec<RigidBodyHandle>,
        joint_handles: Vec<ImpulseJointHandle>,
        initial_world: [Matrix4<f32>; 40],
        bone_frame_offsets: HashMap<u32, Matrix4<f32>>,
        scene_objects: Vec<SceneObject>,
        joint_pairs: Vec<(RigidBodyHandle, RigidBodyHandle)>,
        spawn_positions: HashMap<RigidBodyHandle, Vector3<f32>>,
    ) -> Self {
        Self {
            physics_bodies,
            joint_handles,
            joint_to_body,
            bone_frame_offsets,
            latest_global_transforms: initial_world,
            scene_objects,
            joint_pairs,
            spawn_positions,
        }
    }

    /// Compute settle/quality metrics for this ragdoll from the live physics state.
    fn metrics(&self, physics: &PhysicsWorld) -> RagDollMetrics {
        let mut max_linear_speed: f32 = 0.0;
        let mut max_angular_speed: f32 = 0.0;
        let mut min_y = f32::INFINITY;
        let mut max_drift: f32 = 0.0;

        for handle in &self.physics_bodies {
            if let Some((lin, ang)) = physics.body_velocities(*handle) {
                max_linear_speed = max_linear_speed.max(lin.magnitude());
                max_angular_speed = max_angular_speed.max(ang.magnitude());
            }
            if let Some(iso) = physics.get_body_transform(*handle) {
                min_y = min_y.min(iso.translation.y);
                if let Some(spawn) = self.spawn_positions.get(handle) {
                    let pos = Vector3::new(iso.translation.x, iso.translation.y, iso.translation.z);
                    max_drift = max_drift.max((pos - spawn).magnitude());
                }
            }
        }

        // Largest interpenetration between non-adjacent limb AABBs (axis-aligned
        // overlap depth = min over axes; 0 if separated on any axis).
        let adjacent = |a: RigidBodyHandle, b: RigidBodyHandle| {
            self.joint_pairs
                .iter()
                .any(|(p, c)| (*p == a && *c == b) || (*p == b && *c == a))
        };
        let mut max_nonadjacent_overlap: f32 = 0.0;
        for i in 0..self.physics_bodies.len() {
            for j in (i + 1)..self.physics_bodies.len() {
                let (ha, hb) = (self.physics_bodies[i], self.physics_bodies[j]);
                if adjacent(ha, hb) {
                    continue;
                }
                if let (Some((amin, amax)), Some((bmin, bmax))) =
                    (physics.body_world_aabb(ha), physics.body_world_aabb(hb))
                {
                    let ox = (amax.x.min(bmax.x) - amin.x.max(bmin.x)).max(0.0);
                    let oy = (amax.y.min(bmax.y) - amin.y.max(bmin.y)).max(0.0);
                    let oz = (amax.z.min(bmax.z) - amin.z.max(bmin.z)).max(0.0);
                    if ox > 0.0 && oy > 0.0 && oz > 0.0 {
                        max_nonadjacent_overlap = max_nonadjacent_overlap.max(ox.min(oy).min(oz));
                    }
                }
            }
        }

        RagDollMetrics {
            body_count: self.physics_bodies.len(),
            max_linear_speed,
            max_angular_speed,
            min_y: if min_y.is_finite() { min_y } else { 0.0 },
            max_nonadjacent_overlap,
            max_drift,
        }
    }

    /// Advance this ragdoll's render transforms from physics, sanitizing the
    /// rig on the way. Returns false when the rig is poisoned (a body position
    /// went non-finite/absurd) - the caller must despawn it before the next
    /// physics step, or the broad-phase BVH build will panic on the bad AABB.
    fn update(&mut self, physics: &mut PhysicsWorld) -> bool {
        // Break runaway spawn transients before they cascade to a NaN AABB
        // (which panics the parry BVH broad-phase). Normal collapse dynamics
        // peak well below the cap, so this is a no-op except in the rare
        // diverging run.
        let corrected = physics.sanitize_ragdoll_bodies(&self.physics_bodies, MAX_BODY_SPEED);
        if corrected > 0 {
            // Common during the spawn transient (several frames per death),
            // so debug rather than warn.
            tracing::debug!(
                "ragdoll velocity runaway: clamped {} velocity components",
                corrected
            );
        }
        // The velocity clamp bounds what enters each step, but a single solve
        // can still explode (deep-penetration recovery at spawn) and integrate
        // a non-finite position within that same step. The BVH panic only
        // happens on the NEXT step's broad-phase, so catching it here - and
        // having the manager despawn the rig - keeps the game alive.
        for handle in &self.physics_bodies {
            if let Some(iso) = physics.get_body_transform(*handle) {
                let t = iso.translation;
                let sane = |v: f32| v.is_finite() && v.abs() < 1.0e6;
                // The AABB is computed from the full isometry, so a non-finite
                // ROTATION poisons it just like a non-finite translation (an
                // angular DOF can blow up while linear stays bounded).
                let r = iso.rotation;
                let rot_finite =
                    r.i.is_finite() && r.j.is_finite() && r.k.is_finite() && r.w.is_finite();
                if !(sane(t.x) && sane(t.y) && sane(t.z) && rot_finite) {
                    tracing::warn!(
                        "ragdoll body transform diverged (pos {}, {}, {}; rot finite: {}) - despawning the corpse",
                        t.x,
                        t.y,
                        t.z,
                        rot_finite
                    );
                    return false;
                }
            }
        }
        for (joint_id, handle) in &self.joint_to_body {
            if let Some(isometry) = physics.get_body_transform(*handle) {
                let world = matrix_from_isometry(isometry);
                let idx = *joint_id as usize;
                if idx < self.latest_global_transforms.len() {
                    let offset = self
                        .bone_frame_offsets
                        .get(joint_id)
                        .copied()
                        .unwrap_or_else(Matrix4::identity);
                    self.latest_global_transforms[idx] = world * offset;
                }
            }
        }
        true
    }

    fn renderables(&self) -> Vec<SceneObject> {
        self.scene_objects
            .iter()
            .map(|obj| {
                let mut clone = obj.clone();
                clone.set_transform(Matrix4::identity());
                clone.set_skinning_data(self.latest_global_transforms);
                clone
            })
            .collect()
    }
}

pub struct RagDollManager {
    ragdolls: HashMap<EntityId, RagDoll>,
}

impl RagDollManager {
    pub fn new() -> Self {
        Self {
            ragdolls: HashMap::new(),
        }
    }

    /// Map each ragdoll body's raw handle id to the skeleton joint id it
    /// represents, so debug tooling can label physics bodies/joints by bone.
    pub fn body_to_joint_id(&self) -> HashMap<u32, u32> {
        let mut out = HashMap::new();
        for ragdoll in self.ragdolls.values() {
            for (joint_id, handle) in &ragdoll.joint_to_body {
                out.insert(handle.into_raw_parts().0, *joint_id);
            }
        }
        out
    }

    pub fn add_ragdoll(
        &mut self,
        entity_id: EntityId,
        model: &Model,
        root_transform: Matrix4<f32>,
        joint_transforms: &[Matrix4<f32>; 40],
        root_offset: Vector3<f32>,
        joint_limits: &HashMap<u32, JointLimit>,
        // When true (the default rig), use reduced-coordinate (multibody) joints
        // anchored at the articulation point: limbs structurally can't separate
        // (no hip gap) and the rig settles. When false (the legacy
        // `ragdoll_impulse` fallback), the impulse-joint rig (heavier core, soft
        // locked translation - settles but the hip can visibly sag/separate
        // under load).
        use_multibody: bool,
        // When false, limbs collide with the world but not with each other
        // (CollisionGroup::ragdoll_no_self). Death-handoff rigs spawn in a
        // crumpled, limb-overlapping pose whose many deep limb-limb contacts
        // can explode the articulated solve to non-finite positions in one
        // step; the debug scene's standing-fall spawn keeps self-collision.
        self_collide: bool,
        physics: &mut PhysicsWorld,
    ) -> bool {
        if !model.can_create_rag_doll() {
            return false;
        }
        let collision_group = if self_collide {
            CollisionGroup::ragdoll()
        } else {
            CollisionGroup::ragdoll_no_self()
        };

        let (bones, _) = match model.ragdoll_source() {
            Some(data) => data,
            None => return false,
        };

        // Per-joint fitted collision shapes (joint-local) - capsule spanning the
        // bone toward the child for chain joints, box otherwise. Shared with the
        // damage hitboxes (dark::hit_box). Covers the limbs far better than the
        // old per-joint AABB (which left bones uncovered).
        let hit_box_shapes = model.hit_box_shapes();

        let offset_transform = Matrix4::from_translation(root_offset) * root_transform;
        let mut world_joint_transforms = [Matrix4::identity(); 40];
        for bone in &bones {
            let idx = bone.joint_id as usize;
            if idx < world_joint_transforms.len() {
                world_joint_transforms[idx] = offset_transform * joint_transforms[idx];
            }
        }

        self.remove_entity(entity_id, physics);

        let mut body_handles = Vec::new();
        let mut joint_handles = Vec::new();
        let mut joint_to_body = HashMap::new();
        let mut bone_offsets = HashMap::new();
        let mut joint_pairs = Vec::new();
        let mut spawn_positions = HashMap::new();
        let mut joint_positions = vec![Vector3::new(0.0, 0.0, 0.0); world_joint_transforms.len()];

        for bone in &bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= world_joint_transforms.len() {
                continue;
            }
            // The Dark skeleton lists some joints twice (a torso's main joint is
            // also a fixed point on its parent torso - see ss2_skeleton::create).
            // Create exactly one body per joint id; a duplicate would otherwise
            // spawn a second, never-jointed body that falls away as a stray
            // (invisible: rendering only draws bodies tracked in joint_to_body).
            if joint_to_body.contains_key(&(bone.joint_id as u32)) {
                continue;
            }

            let world_matrix = world_joint_transforms[joint_idx];
            let pos_vec = point3_to_vec3(get_position_from_matrix(&world_matrix));
            joint_positions[joint_idx] = pos_vec;
            let rotation = get_rotation_from_matrix(&world_matrix);
            let isometry = isometry_from_parts(pos_vec, rotation);

            let handle = physics.create_dynamic_body(isometry, Some(entity_id));
            physics.set_body_damping(handle, LINEAR_DAMPING, ANGULAR_DAMPING);
            physics.set_body_angular_sleep_threshold(handle, SLEEP_ANGULAR_THRESHOLD);
            spawn_positions.insert(handle, pos_vec);

            // Build the collider from the joint's fitted shape. Density is chosen
            // so every limb has ~TARGET_BODY_MASS regardless of size: an
            // impulse-jointed chain is unstable when connected bodies have very
            // different masses. Inertia still scales with the shape. Shapes are in
            // joint-local space; the body origin is the joint, so skinning is
            // unaffected.
            match hit_box_shapes.get(&(bone.joint_id as u32)) {
                Some(HitBoxShape::Capsule { a, b, radius }) => {
                    let h = (*b - *a).magnitude();
                    let volume = std::f32::consts::PI * radius * radius * h
                        + 4.0 / 3.0 * std::f32::consts::PI * radius * radius * radius;
                    let density = TARGET_BODY_MASS / volume.max(MIN_VOLUME);
                    physics.attach_collider(
                        handle,
                        SharedShape::capsule(
                            NaPoint3::new(a.x, a.y, a.z),
                            NaPoint3::new(b.x, b.y, b.z),
                            *radius,
                        ),
                        density,
                        collision_group,
                    );
                }
                Some(HitBoxShape::Cuboid {
                    half_extents,
                    center,
                }) => {
                    let he = vec3(
                        half_extents.x.max(MIN_HALF_EXTENT),
                        half_extents.y.max(MIN_HALF_EXTENT),
                        half_extents.z.max(MIN_HALF_EXTENT),
                    );
                    let volume = 8.0 * he.x * he.y * he.z;
                    // The 4x "core" mass stabilizes the *impulse* hub-and-spoke rig
                    // (the tiny pelvis hub otherwise gets yanked by the heavy thighs).
                    // A reduced-coordinate multibody solves that structurally and is
                    // actually destabilized by the mass ratio, so it uses uniform mass.
                    let core_mass = if use_multibody {
                        TARGET_BODY_MASS
                    } else {
                        CORE_BODY_MASS
                    };
                    let density = core_mass / volume.max(MIN_VOLUME);
                    physics.attach_collider_with_offset(
                        handle,
                        SharedShape::cuboid(he.x, he.y, he.z),
                        vec3(center.x, center.y, center.z),
                        density,
                        collision_group,
                    );
                }
                None => {
                    let r = DEFAULT_JOINT_RADIUS;
                    let volume = 4.0 / 3.0 * std::f32::consts::PI * r * r * r;
                    let density = TARGET_BODY_MASS / volume.max(MIN_VOLUME);
                    physics.attach_collider(handle, SharedShape::ball(r), density, collision_group);
                }
            }

            joint_to_body.insert(bone.joint_id as u32, handle);
            bone_offsets.insert(bone.joint_id as u32, Matrix4::identity());
            body_handles.push(handle);
        }

        // A child body must be jointed to its parent exactly once. The Dark
        // skeleton legitimately lists some joints twice - once as a torso's main
        // joint and once as a fixed point on the parent torso (see
        // ss2_skeleton::create) - which would otherwise create a duplicate
        // impulse joint between the same body pair and over-constrain the hub
        // (e.g. the abdomen, joint 18, under the pelvis hub, joint 8).
        let mut jointed_children: HashMap<u32, ()> = HashMap::new();

        for bone in &bones {
            if let Some(parent_id) = bone.parent_id {
                if jointed_children.insert(bone.joint_id as u32, ()).is_some() {
                    continue;
                }
                let parent_handle = match joint_to_body.get(&(parent_id as u32)) {
                    Some(handle) => *handle,
                    None => continue,
                };
                let child_handle = match joint_to_body.get(&(bone.joint_id as u32)) {
                    Some(handle) => *handle,
                    None => continue,
                };

                let parent_idx = parent_id as usize;
                let child_idx = bone.joint_id as usize;
                if parent_idx >= joint_positions.len() || child_idx >= joint_positions.len() {
                    continue;
                }

                let parent_pos = joint_positions[parent_idx];
                let child_pos = joint_positions[child_idx];
                let parent_world = world_joint_transforms[parent_idx];
                let parent_rot = get_rotation_from_matrix(&parent_world);
                let child_world = world_joint_transforms[child_idx];
                let child_rot = get_rotation_from_matrix(&child_world);

                // Both rigs align the joint frames' rotation to the rest/death pose
                // (parent-local = R_parent^-1 * R_child, child-local = identity), so
                // the cone limits are measured from there and the relative angle is
                // exactly 0 at spawn - no energy injected. The anchor *translation*
                // differs by rig type (see each branch).
                let frame1_rot = quat_to_nquat(parent_rot.invert() * child_rot);
                // Per-bone cone limit from the creature definition (e.g. tight for
                // head/neck/spine, wide for shoulders/hips), falling back to a
                // uniform default for joints/creatures without a profile.
                let cone = joint_limits
                    .get(&(bone.joint_id as u32))
                    .map(|limit| limit.cone)
                    .unwrap_or(JOINT_CONE_LIMIT);
                let with_limits = |b: GenericJointBuilder| -> GenericJointBuilder {
                    b.limits(JointAxis::AngX, [-cone, cone])
                        .limits(JointAxis::AngY, [-cone, cone])
                        .limits(JointAxis::AngZ, [-cone, cone])
                        // Disable contacts between this directly-jointed parent/
                        // child pair so they can overlap at the joint without
                        // being ejected; non-adjacent limbs still collide.
                        .contacts_enabled(false)
                };

                if use_multibody {
                    // Reduced-coordinate ball joint anchored at the CHILD origin -
                    // the true articulation point (e.g. the hip socket). Translation
                    // is structurally not a DOF, so limbs can't separate (no hip gap)
                    // and no spring energy is injected. parent_world*frame1 ==
                    // child_world*frame2 at spawn, so multibody forward-kinematics
                    // reproduces the captured death pose (no snap).
                    let parent_to_child = child_pos - parent_pos;
                    let parent_local_anchor = parent_rot.conjugate().rotate_vector(parent_to_child);
                    let frame1 = Isometry::from_parts(
                        Translation3::new(
                            parent_local_anchor.x,
                            parent_local_anchor.y,
                            parent_local_anchor.z,
                        ),
                        frame1_rot,
                    );
                    let frame2 = Isometry::from_parts(
                        Translation3::new(0.0, 0.0, 0.0),
                        UnitQuaternion::identity(),
                    );
                    let joint = with_limits(
                        GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                            .local_frame1(frame1)
                            .local_frame2(frame2),
                    )
                    .build();
                    // Insert returns None only if the child is not a fresh root (a
                    // duplicate edge / cycle) - dedup above prevents that, but skip
                    // defensively rather than panic.
                    if physics
                        .create_multibody_joint(parent_handle, child_handle, joint)
                        .is_some()
                    {
                        joint_pairs.push((parent_handle, child_handle));
                    }
                } else {
                    // Impulse ball joint anchored at the PARENT origin (the heavy
                    // hub), keeping the low-inertia pelvis from being torqued by the
                    // joint spring. The locked translation is softened (compliant,
                    // ~60 Hz) so this hub-and-spoke rig settles instead of pumping
                    // energy - at the cost of the hip sagging/separating under load.
                    let child_to_parent = parent_pos - child_pos;
                    let child_local_anchor = child_rot.conjugate().rotate_vector(child_to_parent);
                    let frame1 = Isometry::from_parts(Translation3::new(0.0, 0.0, 0.0), frame1_rot);
                    let frame2 = Isometry::from_parts(
                        Translation3::new(
                            child_local_anchor.x,
                            child_local_anchor.y,
                            child_local_anchor.z,
                        ),
                        UnitQuaternion::identity(),
                    );
                    let joint = with_limits(
                        GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                            .local_frame1(frame1)
                            .local_frame2(frame2)
                            .softness(SpringCoefficients {
                                natural_frequency: 60.0,
                                damping_ratio: 2.0,
                            }),
                    )
                    .build();
                    let handle = physics.create_impulse_joint(parent_handle, child_handle, joint);
                    joint_handles.push(handle);
                    joint_pairs.push((parent_handle, child_handle));
                }
            }
        }

        let ragdoll = RagDoll::new(
            joint_to_body,
            body_handles,
            joint_handles,
            world_joint_transforms,
            bone_offsets,
            model.clone_scene_objects(),
            joint_pairs,
            spawn_positions,
        );
        self.ragdolls.insert(entity_id, ragdoll);
        true
    }

    /// Advance all ragdolls. Returns the corpse entity ids of any rigs that
    /// were despawned because a body transform went non-finite (the broad-
    /// phase BVH build would panic on their AABB next step) - the caller
    /// should delete those (otherwise-empty) corpse entities from the world.
    /// Losing a corpse is the rare last-resort outcome; the velocity clamp in
    /// `RagDoll::update` makes it rarer still.
    pub fn update(&mut self, physics: &mut PhysicsWorld) -> Vec<EntityId> {
        let mut poisoned = Vec::new();
        for (entity_id, ragdoll) in self.ragdolls.iter_mut() {
            if !ragdoll.update(physics) {
                poisoned.push(*entity_id);
            }
        }
        for entity_id in &poisoned {
            self.remove_entity(*entity_id, physics);
        }
        poisoned
    }

    /// Per-ragdoll quality metrics (keyed by the corpse entity id).
    pub fn debug_metrics(&self, physics: &PhysicsWorld) -> Vec<(EntityId, RagDollMetrics)> {
        self.ragdolls
            .iter()
            .map(|(id, ragdoll)| (*id, ragdoll.metrics(physics)))
            .collect()
    }

    pub fn render_scene_objects(&self) -> Vec<SceneObject> {
        let mut scene = Vec::new();
        for ragdoll in self.ragdolls.values() {
            scene.extend(ragdoll.renderables());
        }
        scene
    }

    /// Seed a just-spawned corpse with its killing blow: shove the struck limb
    /// (by skeleton joint id when the hitbox reported one, else the body
    /// nearest the hit point) along the blow's direction. The articulated rig
    /// propagates the reaction naturally.
    pub fn apply_impact(
        &mut self,
        entity_id: EntityId,
        impact: &crate::scripts::DamageImpact,
        physics: &mut PhysicsWorld,
    ) {
        let Some(ragdoll) = self.ragdolls.get(&entity_id) else {
            return;
        };
        let by_bone = impact
            .bone
            .and_then(|bone| ragdoll.joint_to_body.get(&bone).copied());
        let handle = by_bone.or_else(|| {
            // Nearest body to the hit point.
            ragdoll
                .physics_bodies
                .iter()
                .filter_map(|h| {
                    physics.get_body_transform(*h).map(|iso| {
                        let p =
                            Vector3::new(iso.translation.x, iso.translation.y, iso.translation.z);
                        (*h, (p - impact.point).magnitude2())
                    })
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h)
        });
        if let Some(handle) = handle {
            physics.apply_impulse_to_handle(handle, impact.direction * KILLING_BLOW_IMPULSE);
        }
    }

    pub fn remove_entity(&mut self, entity_id: EntityId, physics: &mut PhysicsWorld) {
        if let Some(ragdoll) = self.ragdolls.remove(&entity_id) {
            // Impulse joints must be removed explicitly; multibody joints are
            // removed automatically when their attached bodies are removed (see
            // PhysicsWorld::remove_rigid_body_handle), so for multibody rigs
            // joint_handles is empty and this loop is a no-op.
            for joint in ragdoll.joint_handles {
                physics.remove_impulse_joint(joint);
            }
            for body in ragdoll.physics_bodies {
                physics.remove_rigid_body_handle(body);
            }
        }
    }

    pub fn get_ragdoll_bodies(&self) -> Vec<RigidBodyHandle> {
        let mut all_bodies = Vec::new();
        for ragdoll in self.ragdolls.values() {
            all_bodies.extend(&ragdoll.physics_bodies);
        }
        all_bodies
    }

    pub fn get_first_ragdoll_body(&self) -> Option<RigidBodyHandle> {
        self.ragdolls
            .values()
            .next()
            .and_then(|ragdoll| ragdoll.physics_bodies.first())
            .copied()
    }
}

fn isometry_from_parts(position: Vector3<f32>, rotation: Quaternion<f32>) -> Isometry<f32> {
    Isometry::from_parts(
        Translation3::new(position.x, position.y, position.z),
        quat_to_nquat(rotation),
    )
}

fn matrix_from_isometry(iso: Isometry<f32>) -> Matrix4<f32> {
    let translation = Matrix4::from_translation(vec3(
        iso.translation.x,
        iso.translation.y,
        iso.translation.z,
    ));
    let rotation = Matrix4::from(Quaternion::new(
        iso.rotation.w,
        iso.rotation.i,
        iso.rotation.j,
        iso.rotation.k,
    ));
    translation * rotation
}
