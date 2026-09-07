//! Player interaction abstraction.
//!
//! One trait with two implementations - VR (two `VirtualHand`s) and flatscreen
//! (`FlatPlayerController`) - so `mission_core` drives interaction
//! polymorphically through a single `Box<dyn PlayerInteraction>` instead of
//! branching on `PresentationMode` and holding both sets of state.
//!
//! Both implementations speak the same `VirtualHandEffect` language, which
//! `mission_core` already processes in one place.

use std::{cell::RefCell, collections::HashMap};

use cgmath::{InnerSpace, One, Point3, Quaternion, Rotation, Vector3, Vector4};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, light::SpotLight},
};
use rapier3d::prelude::RigidBodyHandle;
use shipyard::{EntityId, World};

use crate::{
    GameOptions,
    flat_player_controller::FlatPlayerController,
    hand_glove::GloveRenderer,
    hud::create_arm_hud_panels,
    input_context::InputContext,
    physics::PhysicsWorld,
    virtual_hand::{VirtualHand, VirtualHandEffect, hand_world_position},
    vr_config::Handedness,
    vr_support::{GripPose, SupportProfile, solve_two_hand_pose},
};

/// Read-only per-frame inputs an interaction controller needs to update.
pub struct InteractionContext<'a> {
    pub physics: &'a PhysicsWorld,
    pub world: &'a World,
    pub input: &'a InputContext,
    pub player_pos: Vector3<f32>,
    pub player_rotation: Quaternion<f32>,
    pub head_rotation: Quaternion<f32>,
    /// Eye height above `player_pos` in SS2 units - crouch-aware, so the flat
    /// controller's shot/viewmodel origin follows the actual camera.
    pub eye_height: f32,
    pub step_dt: f32,
    pub support_enabled: bool,
}

/// Read-only per-frame inputs for the VR hand-climb resolve. Separate from
/// [`InteractionContext`] because it runs BEFORE the movement pass - its
/// result IS the movement - while the hands update after it.
pub struct ClimbContext<'a> {
    pub physics: &'a PhysicsWorld,
    pub world: &'a World,
    pub input: &'a InputContext,
    pub pawn_pos: Vector3<f32>,
    pub pawn_rotation: Quaternion<f32>,
    /// World height of the player's feet, for the ledge grip test.
    pub feet_y: f32,
    /// The fixed timestep the coming movement frame integrates with, for the
    /// release velocity (NOT the wall clock - see `vr_climb`).
    pub step_dt: f32,
}

/// How the player interacts with the world. The effects returned by `update`
/// (and `grab`/`wield`) are applied by `mission_core::process_virtual_hand_effects`.
pub trait PlayerInteraction {
    /// Per-frame update; returns effects to apply.
    fn update(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect>;

    /// Resolve presentation-specific item placement before applying hand effects.
    fn fit_held_items(
        &mut self,
        _world: &World,
        _assets: &mut AssetCache,
        _effects: &mut Vec<VirtualHandEffect>,
        _options: &GameOptions,
    ) {
    }

    /// Refresh glove attachment from the collision-resolved item transform.
    fn synchronize_held_visuals(&mut self, _world: &World) {}

    fn grip_diagnostics(&self) -> serde_json::Value {
        serde_json::json!([])
    }

    /// Resolve this frame's hand-climb intent: the body translation a gripping
    /// hand demands, and the velocity a release throws the body with. Flat
    /// climbs by pushing into a ladder instead and never grips.
    fn update_hand_climb(&mut self, _ctx: &ClimbContext) -> crate::vr_climb::ClimbFrame {
        crate::vr_climb::ClimbFrame::default()
    }

    /// Hand-climb state, for debug introspection (`/v1/info`).
    fn hand_climb(&self) -> Option<&crate::vr_climb::HandClimb> {
        None
    }

    /// Feed collision-resolved motion back into the held tracking reference.
    fn resolve_hand_climb(&mut self, _requested: Vector3<f32>, _applied: Vector3<f32>) {}

    /// Drop every climb hold without throwing the body - the vault took over
    /// (see [`crate::vr_climb::HandClimb::release_all`]).
    fn release_climb_grips(&mut self) {}

    /// Entities held in (left, right) - for `PlayerInfo`. Flat reports its
    /// wielded weapon as the "left".
    fn held_entities(&self) -> (Option<EntityId>, Option<EntityId>);

    /// Entities under the reticle/hands, for the hover-highlight overlay.
    fn highlighted_entities(&self) -> Vec<EntityId>;

    /// What one hand alone is aiming at - for readouts that must name a single
    /// object rather than highlight every pick. Flat aims with the reticle, so
    /// it reports that pick for either hand.
    fn highlighted_entity(&self, hand: Handedness) -> Option<EntityId>;

    /// The first-person viewmodel entity (drawn on top); `None` for VR.
    fn viewmodel_entity(&self) -> Option<EntityId> {
        None
    }

    /// 3D visuals owned by the controller (VR: hand models + forearm HUD
    /// panels). Flat draws nothing here; its weapon is drawn from
    /// `viewmodel_entity`.
    ///
    /// `use_mode` is the cyber interface's state: the forearm readouts go quiet
    /// while the interface carries them (issue #1268).
    fn render(
        &self,
        _asset_cache: &mut AssetCache,
        _world: &World,
        _use_mode: bool,
    ) -> Vec<SceneObject> {
        Vec::new()
    }

    /// Hand-mounted spotlights (VR enhanced-lighting experiment).
    fn hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    // --- external state changes driven by effect handlers in mission_core ---

    /// Grab `entity_id` into `hand` (VR) / wield it (flat). Returns any effects.
    fn grab(
        &mut self,
        world: &World,
        entity_id: EntityId,
        hand: Handedness,
    ) -> Vec<VirtualHandEffect>;

    /// A held entity was recreated as `new`; track the new id.
    fn replace_entity(&mut self, old: EntityId, new: EntityId, rigid_body: RigidBodyHandle);

    /// A held entity was destroyed; release it.
    fn on_entity_destroyed(&mut self, entity_id: EntityId);

    /// Which hand is holding `entity_id`, if any. A VR wield needs this to
    /// pick which way round to draw a first-person arm rig (they are all
    /// authored right-handed). Flat has one wield hand and reports it as the
    /// right, matching the right-hand trigger it fires with.
    fn holding_hand(&self, entity_id: EntityId) -> Option<Handedness>;

    /// Whether `entity_id` is currently held. Answered from
    /// [`Self::holding_hand`] so "is it held" and "which hand holds it" cannot
    /// give different answers.
    fn is_holding(&self, entity_id: EntityId) -> bool {
        self.holding_hand(entity_id).is_some()
    }

    /// Wield `entity_id` as the first-person weapon (flat); no-op for VR.
    fn wield(&mut self, _entity_id: EntityId) -> Vec<VirtualHandEffect> {
        Vec::new()
    }

    fn is_wielding(&self) -> bool {
        false
    }

    /// The flatscreen camera/crosshair fire ray (origin, forward), if this
    /// controller drives a crosshair. `None` for VR (which aims by hand pose).
    fn flat_aim_ray(&self) -> Option<(Point3<f32>, Vector3<f32>)> {
        None
    }
}

/// VR interaction: two motion-controller hands.
pub struct VrInteraction {
    left_hand: VirtualHand,
    right_hand: VirtualHand,
    /// Lazily initialized on first render (needs the asset cache). The outer
    /// `Option` is "have we tried yet" - a glove that failed to load stays
    /// `Some(None)` so we don't hit the asset cache's miss path every frame.
    glove_renderer: RefCell<Option<Option<GloveRenderer>>>,
    /// Which hands hold a climbing hold, and which one moves the body.
    hand_climb: crate::vr_climb::HandClimb,
    grip_kinematics: Option<[crate::vr_grip::GripKinematics; 2]>,
    grip_geometry: HashMap<String, Option<GripGeometry>>,
    kinematics_hashes: [String; 2],
    grip_library: Option<crate::vr_grip::GripLibrary>,
    grip_hints: HashMap<String, crate::vr_grip::GripHints>,
    grip_overlay: bool,
    fitted_grips: [Option<HeldGrip>; 2],
    support_profiles: HashMap<String, SupportProfile>,
    support: Option<SupportAttachment>,
    support_preview: Option<SupportCandidate>,
    support_pressed: [bool; 2],
    support_blocked: [bool; 2],
    visual_hands: [Option<GripPose>; 2],
    step_dt: f32,
}

struct SupportAttachment {
    entity: EntityId,
    primary: usize,
    active: bool,
    blend: f32,
    correction: Quaternion<f32>,
    anchor: Vector3<f32>, // Scaled model-space contact, fixed until release.
    primary_offset: Vector3<f32>, // Grab-time collision offset, in world space.
    player_rotation: Quaternion<f32>, // Rebase the offset on locomotion turns, never wrist twists.
}

#[derive(Clone)]
struct SupportCandidate {
    entity: EntityId,
    primary: usize,
    profile: SupportProfile,
    primary_palm: Vector3<f32>,
    visible_primary_palm: Vector3<f32>,
    control_primary_palm: Vector3<f32>,
    primary_anchor: Vector3<f32>,
    tracked_axis: Vector3<f32>,
    anchor: Vector3<f32>,
    region: Option<[Vector3<f32>; 2]>,
    model_pose: GripPose,
    hand_pose: GripPose,
}

struct GripGeometry {
    triangles: std::rc::Rc<Vec<[Point3<f32>; 3]>>,
    fingerprint: String,
    weapon_arms: Option<Vec<Point3<f32>>>,
}

struct HeldGrip {
    model_mirror: Option<cgmath::Matrix4<f32>>,
    entity: EntityId,
    model: String,
    resolved: Option<crate::vr_grip::ResolvedGrip>,
    solve_ms: f64,
    surface_hash: String,
    kinematics_hash: String,
    hints_hash: String,
    source: &'static str,
    authored: bool,
    item_bounds: Option<[[f32; 3]; 2]>,
}

impl HeldGrip {
    /// Undo the contact-origin split on the collision-resolved melee body.
    fn physical_model_pose(&self, world: &World) -> Option<GripPose> {
        use shipyard::{Get, View};
        let grip = self.resolved.as_ref()?;
        let (positions, offsets) = world
            .borrow::<(
                View<dark::properties::PropPosition>,
                View<crate::runtime_props::RuntimePropVrGripOffset>,
            )>()
            .ok()?;
        let position = positions.get(self.entity).ok()?;
        let contact = offsets.get(self.entity).ok()?.0;
        let pose = GripPose {
            position: position.position
                - position.rotation.rotate_vector(contact * grip.item_scale),
            rotation: position.rotation,
        };
        pose.is_tracked().then_some(pose)
    }
}

impl VrInteraction {
    pub fn new() -> Self {
        Self {
            left_hand: VirtualHand::new(Handedness::Left),
            right_hand: VirtualHand::new(Handedness::Right),
            glove_renderer: RefCell::new(None),
            hand_climb: crate::vr_climb::HandClimb::default(),
            grip_kinematics: None,
            grip_geometry: HashMap::new(),
            kinematics_hashes: [String::new(), String::new()],
            grip_library: None,
            grip_hints: HashMap::new(),
            grip_overlay: false,
            fitted_grips: [None, None],
            support_profiles: HashMap::new(),
            support: None,
            support_preview: None,
            support_pressed: [true; 2],
            support_blocked: [false; 2],
            visual_hands: [None, None],
            step_dt: 0.0,
        }
    }
}

impl Default for VrInteraction {
    fn default() -> Self {
        Self::new()
    }
}

impl VrInteraction {
    fn hand_poses(&self) -> [GripPose; 2] {
        [&self.left_hand, &self.right_hand].map(|hand| GripPose {
            position: hand.get_position(),
            rotation: hand.get_rotation(),
        })
    }

    fn support_candidate(
        &self,
        world: &World,
        poses: [GripPose; 2],
        prefer_locked_anchor: bool,
    ) -> Option<SupportCandidate> {
        let rig = self.grip_kinematics.as_ref()?;
        for (primary, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
            let Some(held) = self.fitted_grips[primary].as_ref() else {
                continue;
            };
            if !crate::vr_support::supports_model(&held.model)
                || hand.get_held_entity() != Some(held.entity)
                || !poses[primary].is_tracked()
            {
                continue;
            }
            let Some(grip) = held.resolved.as_ref() else {
                continue;
            };
            let Some(profile) = self.support_profiles.get(&held.model) else {
                continue;
            };
            let base_rotation = poses[primary].rotation.normalize() * grip.rotation;
            let primary_palm = poses[primary].point(rig[primary].palm);
            // Scaled model-space anchors, independent of the melee contact-origin split.
            let primary_anchor = grip
                .rotation
                .conjugate()
                .rotate_vector(rig[primary].palm - grip.offset);
            let handedness = if primary == 0 {
                Handedness::Left
            } else {
                Handedness::Right
            };
            let Some(model_mirror) = held.model_mirror else {
                continue;
            };
            let correction = self
                .support
                .as_ref()
                .filter(|s| s.entity == held.entity && s.primary == primary)
                .map_or(Quaternion::one(), |s| s.correction);
            let rotation = base_rotation * correction;
            let target_pose = GripPose {
                position: primary_palm - rotation.rotate_vector(primary_anchor),
                rotation,
            };
            // Melee physics may stop short of its target at a wall. Acquisition
            // and visible gloves belong on that actual weapon, not an unseen target.
            let model_pose = held.physical_model_pose(world).unwrap_or(target_pose);
            let anchor = self
                .support
                .as_ref()
                .filter(|s| prefer_locked_anchor && s.entity == held.entity && s.primary == primary)
                .map_or_else(
                    || {
                        if !poses[1 - primary].is_tracked() {
                            return profile.region_in_frame(handedness, model_mirror)[0]
                                * grip.item_scale;
                        }
                        profile.closest_anchor(
                            handedness,
                            model_mirror,
                            grip.item_scale,
                            model_pose,
                            poses[1 - primary].point(rig[1 - primary].palm),
                        )
                    },
                    |s| s.anchor,
                );
            let hand_pose =
                profile.glove_pose(handedness, model_pose, grip, &rig[1 - primary], anchor);
            return Some(SupportCandidate {
                entity: held.entity,
                primary,
                profile: profile.clone(),
                primary_palm,
                visible_primary_palm: model_pose.point(primary_anchor),
                // Calibrate the controller reference once when grabbing a blocked
                // weapon. Later collision motion is feedback, not a release gesture.
                control_primary_palm: self
                    .support
                    .as_ref()
                    .filter(|s| {
                        prefer_locked_anchor && s.entity == held.entity && s.primary == primary
                    })
                    .map_or(model_pose.point(primary_anchor), |s| {
                        primary_palm + s.primary_offset
                    }),
                primary_anchor,
                tracked_axis: base_rotation.rotate_vector(anchor - primary_anchor),
                anchor,
                region: profile.region.as_ref().map(|_| {
                    profile
                        .region_in_frame(handedness, model_mirror)
                        .map(|p| p * grip.item_scale)
                }),
                model_pose,
                hand_pose,
            });
        }
        None
    }

    fn update_support(&mut self, ctx: &InteractionContext) {
        self.step_dt = ctx.step_dt.max(0.0);
        if let Some(s) = self.support.as_mut() {
            let player_rotation = ctx.player_rotation.normalize();
            s.primary_offset =
                (player_rotation * s.player_rotation.conjugate()).rotate_vector(s.primary_offset);
            s.player_rotation = player_rotation;
        }
        let inputs = [&ctx.input.left_hand, &ctx.input.right_hand];
        let poses = inputs.map(|input| GripPose {
            position: crate::virtual_hand::hand_world_position(
                ctx.player_pos,
                ctx.player_rotation,
                input.position,
            ),
            rotation: ctx.player_rotation * input.rotation,
        });
        let pressed = inputs.map(|input| input.squeeze_value >= 0.5);
        for i in 0..2 {
            if !pressed[i] && inputs[i].trigger_value < 0.5 {
                self.support_blocked[i] = false;
            }
        }
        let candidate = self
            .support
            .as_ref()
            .and_then(|_| self.support_candidate(ctx.world, poses, true));
        let available = |primary: usize| {
            let other = 1 - primary;
            let hand = if other == 0 {
                &self.left_hand
            } else {
                &self.right_hand
            };
            hand.get_held_entity().is_none()
                && !self.hand_climb.grips().any(|(h, _)| {
                    h == if other == 0 {
                        Handedness::Left
                    } else {
                        Handedness::Right
                    }
                })
        };
        if let Some(support) = self.support.as_mut() {
            let valid = candidate.as_ref().is_some_and(|c| {
                let other = 1 - c.primary;
                let Some(rig) = self.grip_kinematics.as_ref() else {
                    return false;
                };
                let separation =
                    (poses[other].point(rig[other].palm) - c.control_primary_palm).magnitude();
                support.entity == c.entity
                    && support.primary == c.primary
                    && ctx.support_enabled
                    && available(c.primary)
                    && poses.iter().all(|p| p.is_tracked())
                    && pressed[c.primary]
                    && pressed[other]
                    && separation > 0.06
                    && c.profile.allows_swing(
                        c.tracked_axis,
                        poses[other].point(rig[other].palm) - c.control_primary_palm,
                    )
                    && (separation - (c.anchor - c.primary_anchor).magnitude()).abs()
                        <= c.profile.release_distance
            });
            if !valid {
                support.active = false;
            }
        }
        // Hand input still updates on render-only (zero-dt) frames, just like
        // VirtualHand. Consume the squeeze edge there too; only blending needs dt.
        if ctx.support_enabled && self.support.as_ref().is_none_or(|s| !s.active) {
            let candidate = self.support_candidate(ctx.world, poses, false);
            if let Some(c) = candidate.as_ref() {
                let other = 1 - c.primary;
                if let Some(rig) = self.grip_kinematics.as_ref() {
                    let palm = poses[other].point(rig[other].palm);
                    if available(c.primary)
                        && poses.iter().all(|p| p.is_tracked())
                        && pressed[c.primary]
                        && pressed[other]
                        && !self.support_pressed[other]
                        && !self.support_blocked[other]
                        && (palm - c.control_primary_palm).magnitude() > 0.08
                        && c.profile
                            .allows_swing(c.tracked_axis, palm - c.control_primary_palm)
                        && (palm - c.model_pose.point(c.anchor)).magnitude()
                            <= c.profile.grab_radius
                    {
                        let (blend, correction) = self
                            .support
                            .as_ref()
                            .filter(|s| s.entity == c.entity && s.primary == c.primary)
                            .map_or((0.0, Quaternion::one()), |s| (s.blend, s.correction));
                        self.support = Some(SupportAttachment {
                            entity: c.entity,
                            primary: c.primary,
                            active: true,
                            blend,
                            correction,
                            anchor: c.anchor,
                            // Lock this reference until release, including after a block
                            // clears. Following the body would turn physics recovery into
                            // a steering/release gesture with stationary controllers.
                            primary_offset: c.visible_primary_palm - c.primary_palm,
                            player_rotation: ctx.player_rotation.normalize(),
                        });
                        self.support_blocked[other] = true;
                    }
                }
            }
        }
        self.support_pressed = pressed;
    }

    fn apply_support(&mut self, world: &World, effects: &mut Vec<VirtualHandEffect>) {
        use shipyard::{Get, View};
        self.visual_hands = [None, None];
        let poses = self.hand_poses();
        let candidate = self.support_candidate(world, poses, true);
        let valid = self
            .support
            .as_ref()
            .zip(candidate.as_ref())
            .is_some_and(|(s, c)| {
                s.entity == c.entity
                    && s.primary == c.primary
                    && (!s.active
                        || [
                            self.left_hand.get_held_entity(),
                            self.right_hand.get_held_entity(),
                        ][1 - s.primary]
                            .is_none())
            });
        if !valid {
            self.support = None;
        }
        if let (Some(s), Some(c), Some(rig)) = (
            &mut self.support,
            candidate.as_ref(),
            self.grip_kinematics.as_ref(),
        ) {
            let primary = s.primary;
            let other = 1 - primary;
            let grip = self.fitted_grips[primary]
                .as_ref()
                .unwrap()
                .resolved
                .as_ref()
                .unwrap();
            let base_rotation = poses[primary].rotation.normalize() * grip.rotation;
            let target = if s.active && poses[other].is_tracked() {
                solve_two_hand_pose(
                    c.primary_palm,
                    // Use the grab-time calibrated controller reference for aim.
                    // Collision motion must not steer the target back into itself.
                    // Translation still follows the primary controller.
                    c.primary_palm + (poses[other].point(rig[other].palm) - c.control_primary_palm),
                    base_rotation,
                    c.primary_anchor,
                    c.anchor,
                    base_rotation * s.correction,
                )
                .rotation
            } else {
                base_rotation
            };
            let alpha = 1.0 - (-self.step_dt / 0.06).exp();
            s.correction = s
                .correction
                .slerp(base_rotation.conjugate() * target, alpha)
                .normalize();
            s.blend = (s.blend
                + if s.active {
                    self.step_dt / 0.08
                } else {
                    -self.step_dt / 0.08
                })
            .clamp(0.0, 1.0);
            let rotation = base_rotation * s.correction;
            let model_pose = GripPose {
                position: c.primary_palm - rotation.rotate_vector(c.primary_anchor),
                rotation,
            };
            let contact = world
                .borrow::<View<crate::runtime_props::RuntimePropVrGripOffset>>()
                .ok()
                .and_then(|v| v.get(c.entity).ok().map(|p| p.0))
                .unwrap_or(Vector3::new(0.0, 0.0, 0.0));
            effects.retain(|e| !matches!(e, VirtualHandEffect::SetPositionRotation { entity_id, .. } if *entity_id == c.entity));
            effects.push(VirtualHandEffect::SetPositionRotation {
                entity_id: c.entity,
                position: model_pose.point(contact * grip.item_scale),
                rotation,
                scale: Vector3::new(grip.item_scale, grip.item_scale, grip.item_scale),
            });
            if !s.active && s.blend == 0.0 && s.correction.s.abs() > 0.99999 {
                self.support = None;
                self.visual_hands = [None, None];
            }
        }
    }
}

impl PlayerInteraction for VrInteraction {
    fn fit_held_items(
        &mut self,
        world: &World,
        assets: &mut AssetCache,
        effects: &mut Vec<VirtualHandEffect>,
        options: &GameOptions,
    ) {
        let bake = options
            .experimental_features
            .contains("astra-bake-vr-grips");
        self.grip_overlay = options.experimental_features.contains("astra-grip-overlay");
        use dark::{importers::GRIP_SURFACE_IMPORTER, properties::PropModelName};
        use engine::assets::text_importer::TEXT_IMPORTER;
        use shipyard::{Get, View};
        if self.grip_library.is_none() {
            self.support_profiles = assets
                .get_opt(&TEXT_IMPORTER, "vr-support-grips.json")
                .and_then(|text| {
                    serde_json::from_str::<HashMap<String, SupportProfile>>(&text).ok()
                })
                .unwrap_or_default();
            self.support_profiles.retain(|model, profile| {
                crate::vr_support::supports_model(model) && profile.is_valid()
            });
            self.grip_library = Some(
                assets
                    .get_opt(&TEXT_IMPORTER, "vr-grips.json")
                    .and_then(|text| {
                        serde_json::from_str::<crate::vr_grip::GripLibrary>(&text).ok()
                    })
                    .filter(|library| {
                        library.version == 1
                            && library.solver_revision == crate::vr_grip::SOLVER_REVISION
                    })
                    .unwrap_or_default(),
            );
            if let Some(weapons) = assets
                .get_opt(&TEXT_IMPORTER, "vr-weapon-grips.json")
                .and_then(|text| serde_json::from_str::<crate::vr_grip::GripLibrary>(&text).ok())
                .filter(|lib| {
                    lib.version == 1 && lib.solver_revision == crate::vr_grip::SOLVER_REVISION
                })
            {
                // Explicit pickup-library edits take precedence over the shipped weapon defaults.
                let library = self.grip_library.as_mut().unwrap();
                // Lookup skips stale entries, so a stale custom override can
                // fall back to the current shipped default for the same hand.
                library.entries.extend(weapons.entries);
            }
            self.grip_hints = assets
                .get_opt(&TEXT_IMPORTER, "astra-vr-grip-hints.json")
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
        }
        let mut slot = self.glove_renderer.borrow_mut();
        let Some(renderer) = slot
            .get_or_insert_with(|| GloveRenderer::new(assets))
            .as_mut()
        else {
            return;
        };
        let kinematics = self.grip_kinematics.get_or_insert_with(|| {
            [
                renderer.grip_kinematics(Handedness::Left),
                renderer.grip_kinematics(Handedness::Right),
            ]
        });
        if self.kinematics_hashes[0].is_empty() {
            self.kinematics_hashes = std::array::from_fn(|i| kinematics[i].fingerprint());
        }
        for (index, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
            let Some(entity) = hand
                .get_held_entity()
                .filter(|_| crate::virtual_hand::shows_hand_visual(world, hand.get_held_entity()))
            else {
                self.fitted_grips[index] = None;
                continue;
            };
            let model = world
                .borrow::<View<PropModelName>>()
                .ok()
                .and_then(|v| v.get(entity).ok().map(|p| p.0.to_lowercase()));
            let Some(model) = model else {
                self.fitted_grips[index] = None;
                continue;
            };
            if self.fitted_grips[index]
                .as_ref()
                .is_none_or(|g| g.entity != entity || g.model != model)
            {
                let start = std::time::Instant::now();
                let hand_name = if index == 0 { "left" } else { "right" };
                let eligible = bake
                    || self
                        .grip_library
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .any(|entry| entry.model == model && entry.hand == hand_name);
                let geometry = if eligible {
                    self.grip_geometry
                        .entry(format!("{model}:{hand_name}"))
                        .or_insert_with(|| {
                            if crate::vr_weapon_grip::supports_model(&model) {
                                let (triangles, arms, fingerprint) = crate::vr_weapon_grip::inputs(
                                    assets,
                                    &format!("{model}.bin"),
                                    if index == 0 {
                                        Handedness::Left
                                    } else {
                                        Handedness::Right
                                    },
                                )?;
                                return Some(GripGeometry {
                                    triangles: std::rc::Rc::new(triangles),
                                    fingerprint,
                                    weapon_arms: Some(arms),
                                });
                            }
                            let triangles = assets
                                .get_opt(&GRIP_SURFACE_IMPORTER, &format!("{}.bin", model))?;
                            let fingerprint = crate::vr_grip::surface_fingerprint(&triangles);
                            Some(GripGeometry {
                                triangles,
                                fingerprint,
                                weapon_arms: None,
                            })
                        })
                        .as_ref()
                } else {
                    None
                };
                let triangles = geometry.map(|g| &g.triangles);
                let surface_hash = geometry.map(|g| g.fingerprint.clone()).unwrap_or_default();
                let kinematics_hash = self.kinematics_hashes[index].clone();
                let hints = self.grip_hints.entry(model.clone()).or_default();
                let hints_hash = hints.fingerprint();
                let resolved = if bake && geometry.is_some_and(|g| g.weapon_arms.is_some()) {
                    let geometry = geometry.unwrap();
                    crate::vr_weapon_grip::resolve(
                        &model,
                        if index == 0 {
                            Handedness::Left
                        } else {
                            Handedness::Right
                        },
                        &geometry.triangles,
                        geometry.weapon_arms.as_ref().unwrap(),
                        &kinematics[index],
                    )
                } else if bake {
                    triangles
                        .as_ref()
                        .and_then(|triangles| crate::vr_grip::GripSurface::new(triangles))
                        .and_then(|surface| surface.resolve(&kinematics[index], hints))
                } else {
                    self.grip_library
                        .as_ref()
                        .unwrap()
                        .lookup(
                            &model,
                            hand_name,
                            &surface_hash,
                            &kinematics_hash,
                            &hints_hash,
                        )
                        .cloned()
                };
                let source = if bake {
                    "bake"
                } else if resolved.is_some() {
                    "prepared"
                } else {
                    "missing_or_stale"
                };
                // The Quest runtime has no tracing subscriber; use its structured
                // stdout marker convention for on-device verification.
                let solve_ms = start.elapsed().as_secs_f64() * 1000.0;
                println!(
                    "SHOCK2QUEST_VR_GRIP model={model} hand={hand_name} source={source} elapsed_ms={solve_ms:.3}"
                );
                if resolved.is_none() {
                    println!(
                        "SHOCK2QUEST_VR_GRIP_REJECT model={model} hand={hand_name} geometry={} surface={surface_hash} kinematics={kinematics_hash} hints={hints_hash}",
                        geometry.is_some(),
                    );
                    for entry in self
                        .grip_library
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .filter(|e| e.model == model && e.hand == hand_name)
                    {
                        println!(
                            "SHOCK2QUEST_VR_GRIP_EXPECTED model={model} hand={hand_name} surface={} kinematics={} hints={} valid={}",
                            entry.surface_hash,
                            entry.kinematics_hash,
                            entry.hints_hash,
                            entry.grip.is_valid(),
                        );
                    }
                }
                let authored = !bake
                    && resolved.is_some()
                    && self.grip_library.as_ref().unwrap().entries.iter().any(|e| {
                        e.model == model
                            && e.hand == hand_name
                            && e.authored
                            && e.surface_hash == surface_hash
                            && e.kinematics_hash == kinematics_hash
                            && e.hints_hash == hints_hash
                            && resolved.as_ref() == Some(&e.grip)
                    });
                if crate::vr_weapon_grip::is_melee(&model) {
                    if let (Some(grip), Some(geometry), Some(contact)) = (
                        resolved.as_ref(),
                        geometry,
                        world
                            .borrow::<View<crate::runtime_props::RuntimePropVrGripOffset>>()
                            .ok()
                            .and_then(|v| v.get(entity).ok().map(|p| p.0)),
                    ) {
                        let mut min = cgmath::vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
                        let mut max = -min;
                        for p in geometry.triangles.iter().flatten() {
                            let p = (p.to_homogeneous().truncate() - contact) * grip.item_scale;
                            for axis in 0..3 {
                                min[axis] = min[axis].min(p[axis]);
                                max[axis] = max[axis].max(p[axis]);
                            }
                        }
                        effects.push(VirtualHandEffect::FitHeldMelee {
                            entity_id: entity,
                            size: max - min,
                            center: (min + max) * 0.5,
                        });
                    }
                }
                let item_bounds = resolved
                    .as_ref()
                    .zip(geometry)
                    .and_then(|(grip, geometry)| grip.item_bounds(&geometry.triangles));
                let model_mirror = if crate::vr_support::supports_model(&model) {
                    crate::vr_weapon_grip::model_mirror(assets, &format!("{model}.bin"))
                } else {
                    None
                };
                self.fitted_grips[index] = Some(HeldGrip {
                    model_mirror,
                    item_bounds,
                    entity,
                    model,
                    resolved,
                    solve_ms,
                    surface_hash,
                    kinematics_hash,
                    hints_hash,
                    source,
                    authored,
                });
            }
            if let Some(grip) = self.fitted_grips[index]
                .as_ref()
                .and_then(|g| g.resolved.as_ref())
            {
                // The acquisition frame may only contain HoldItem. Append its
                // initial placement too, in the same effect pipeline as movement.
                effects.retain(|effect| !matches!(effect, VirtualHandEffect::SetPositionRotation {entity_id,..} if *entity_id == entity));
                use cgmath::Rotation;
                let contact = world
                    .borrow::<View<crate::runtime_props::RuntimePropVrGripOffset>>()
                    .ok()
                    .and_then(|v| v.get(entity).ok().map(|p| p.0))
                    .unwrap_or(cgmath::vec3(0.0, 0.0, 0.0));
                let offset = grip.offset + grip.rotation.rotate_vector(contact * grip.item_scale);
                effects.push(VirtualHandEffect::SetPositionRotation {
                    entity_id: entity,
                    position: hand.get_position() + hand.get_rotation().rotate_vector(offset),
                    rotation: hand.get_rotation() * grip.rotation,
                    scale: cgmath::vec3(grip.item_scale, grip.item_scale, grip.item_scale),
                });
            }
        }
        drop(slot);
        self.apply_support(world, effects);
    }

    fn synchronize_held_visuals(&mut self, world: &World) {
        let poses = self.hand_poses();
        self.support_preview = self.support_candidate(world, poses, true);
        self.visual_hands = [None, None];
        // Physical melee follows its synchronized body even with one hand.
        // Supported guns follow their solved pose so both gloves stay seated.
        for (index, held) in self.fitted_grips.iter().enumerate() {
            let Some(held) = held else { continue };
            let model = held.physical_model_pose(world).or_else(|| {
                self.support.as_ref()?;
                self.support_preview
                    .as_ref()
                    .filter(|c| c.primary == index && c.entity == held.entity)
                    .map(|c| c.model_pose)
            });
            let (Some(grip), Some(model)) = (&held.resolved, model) else {
                continue;
            };
            let rotation = model.rotation * grip.rotation.conjugate();
            self.visual_hands[index] = Some(GripPose {
                position: model.position - rotation.rotate_vector(grip.offset),
                rotation,
            });
        }
        let (Some(support), Some(candidate)) = (&self.support, &self.support_preview) else {
            return;
        };
        let other = 1 - support.primary;
        if poses[other].is_tracked()
            && [
                self.left_hand.get_held_entity(),
                self.right_hand.get_held_entity(),
            ][other]
                .is_none()
        {
            self.visual_hands[other] = Some(GripPose {
                position: poses[other].position * (1.0 - support.blend)
                    + candidate.hand_pose.position * support.blend,
                rotation: poses[other]
                    .rotation
                    .normalize()
                    .slerp(candidate.hand_pose.rotation, support.blend),
            });
        }
    }

    fn grip_diagnostics(&self) -> serde_json::Value {
        let visual_triggers = [
            self.left_hand.get_trigger_value(),
            self.right_hand.get_trigger_value(),
        ];
        serde_json::Value::Array(self.fitted_grips.iter().enumerate().filter_map(|(i, grip)| {
            let grip=grip.as_ref()?;
            let support = self.support_preview.as_ref().filter(|c| c.primary == i && c.entity == grip.entity).map(|c| {
                let attachment = self.support.as_ref().filter(|s| s.primary == i && s.entity == grip.entity);
                serde_json::json!({
                    "hand": if i == 0 { "right" } else { "left" },
                    "attached": attachment.is_some_and(|s| s.active),
                    "blend": attachment.map_or(0.0, |s| s.blend),
                    "tracked_palm": self.grip_kinematics.as_ref().map(|rig| self.hand_poses()[1-i].point(rig[1-i].palm)),
                    "pressed": self.support_pressed, "blocked": self.support_blocked, "step_dt": self.step_dt,
                    "socket_position": c.model_pose.point(c.anchor),
                    "controller_position": c.hand_pose.position,
                    "controller_rotation": c.hand_pose.rotation,
                    "model_position": c.model_pose.position, "model_rotation": c.model_pose.rotation,
                    "primary_palm": c.primary_palm, "primary_anchor": c.primary_anchor,
                    "visible_primary_palm": c.visible_primary_palm,
                    "control_primary_palm": c.control_primary_palm,
                    "support_anchor": c.anchor, "grab_radius": c.profile.grab_radius,
                    "region_endpoints": c.region.map(|ends| ends.map(|p| c.model_pose.point(p))),
                    "visual_trigger": visual_triggers[1-i],
                    "finger_curls": crate::vr_grip::blended_curls(c.profile.curls, c.profile.trigger_curls, visual_triggers[1-i]),
                    "release_distance": c.profile.release_distance, "max_swing_degrees": c.profile.max_swing_degrees
                })
            });
            Some(serde_json::json!({"glove_pose": self.visual_hands[i], "support": support, "hand": if i == 0 {"left"} else {"right"}, "entity_id": grip.entity.inner() as i32,
                "visual_trigger": visual_triggers[i], "finger_curls": grip.resolved.as_ref().map(|g| g.curls_at(visual_triggers[i])),
                "model": grip.model, "item_bounds": grip.item_bounds, "solve_ms": grip.solve_ms, "grip": grip.resolved,
                "surface_hash": grip.surface_hash, "kinematics_hash": grip.kinematics_hash, "hints_hash": grip.hints_hash, "solver_revision": crate::vr_grip::SOLVER_REVISION, "source": grip.source, "authored": grip.authored,
                "palm": self.grip_kinematics.as_ref().map(|k| k[i].palm), "palm_normal": self.grip_kinematics.as_ref().map(|k| k[i].normal)}))
        }).collect())
    }

    fn update_hand_climb(&mut self, ctx: &ClimbContext) -> crate::vr_climb::ClimbFrame {
        use shipyard::EntitiesView;

        let hand_input = |index: usize, hand: &VirtualHand, input: &crate::input_context::Hand| {
            crate::vr_climb::ClimbHandInput {
                local_position: input.position,
                squeeze: input.squeeze_value,
                // A hand that is carrying something cannot also hold a ladder.
                is_empty: hand.get_held_entity().is_none() && !self.support_blocked[index],
            }
        };
        self.hand_climb.update(
            ctx.pawn_pos,
            ctx.pawn_rotation,
            ctx.step_dt,
            [
                hand_input(0, &self.left_hand, &ctx.input.left_hand),
                hand_input(1, &self.right_hand, &ctx.input.right_hand),
            ],
            |point, transferring| {
                if transferring {
                    ctx.physics
                        .climbable_grip_during_transfer_at(point, ctx.feet_y)
                } else {
                    ctx.physics.climbable_grip_at(
                        point,
                        crate::physics::CLIMB_GRIP_RADIUS,
                        ctx.feet_y,
                    )
                }
            },
            // "Cannot check" is not "gone" - a failed borrow keeps the hold.
            |entity_id| {
                ctx.world
                    .borrow::<EntitiesView>()
                    .map_or(true, |entities| entities.is_alive(entity_id))
            },
        )
    }

    fn hand_climb(&self) -> Option<&crate::vr_climb::HandClimb> {
        Some(&self.hand_climb)
    }

    fn resolve_hand_climb(&mut self, requested: Vector3<f32>, applied: Vector3<f32>) {
        self.hand_climb.resolve_translation(requested, applied);
    }

    fn release_climb_grips(&mut self) {
        self.hand_climb.release_all();
    }

    fn update(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect> {
        self.update_support(ctx);
        let left_held_entity = self.left_hand.get_held_entity();
        // Read both positions from this frame before either hand updates, so
        // native-toxin self-use has the same distance in either hand.
        let left_position = hand_world_position(
            ctx.player_pos,
            ctx.player_rotation,
            ctx.input.left_hand.position,
        );
        let right_position = hand_world_position(
            ctx.player_pos,
            ctx.player_rotation,
            ctx.input.right_hand.position,
        );
        let (right_hand, mut right_msgs) =
            if self.support_blocked[1] && self.right_hand.get_held_entity().is_none() {
                (
                    self.right_hand.update_suppressed(
                        ctx.player_pos,
                        ctx.player_rotation,
                        &ctx.input.right_hand,
                    ),
                    Vec::new(),
                )
            } else {
                VirtualHand::update(
                    &self.right_hand,
                    ctx.physics,
                    ctx.world,
                    ctx.player_pos,
                    ctx.player_rotation,
                    &ctx.input.right_hand,
                    left_held_entity,
                    left_position,
                )
            };
        self.right_hand = right_hand;

        // Right updates first, so a same-frame right-hand grab is visible to
        // the left hand and one physical item cannot enter both hand states.
        let right_held_entity = self.right_hand.get_held_entity();
        let (left_hand, mut left_msgs) =
            if self.support_blocked[0] && self.left_hand.get_held_entity().is_none() {
                (
                    self.left_hand.update_suppressed(
                        ctx.player_pos,
                        ctx.player_rotation,
                        &ctx.input.left_hand,
                    ),
                    Vec::new(),
                )
            } else {
                VirtualHand::update(
                    &self.left_hand,
                    ctx.physics,
                    ctx.world,
                    ctx.player_pos,
                    ctx.player_rotation,
                    &ctx.input.left_hand,
                    right_held_entity,
                    right_position,
                )
            };
        self.left_hand = left_hand;

        left_msgs.append(&mut right_msgs);
        left_msgs
    }

    fn held_entities(&self) -> (Option<EntityId>, Option<EntityId>) {
        (
            self.left_hand.get_held_entity(),
            self.right_hand.get_held_entity(),
        )
    }

    fn highlighted_entities(&self) -> Vec<EntityId> {
        [
            self.left_hand.get_raytraced_entity(),
            self.right_hand.get_raytraced_entity(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn highlighted_entity(&self, hand: Handedness) -> Option<EntityId> {
        match hand {
            Handedness::Left => self.left_hand.get_raytraced_entity(),
            Handedness::Right => self.right_hand.get_raytraced_entity(),
        }
    }

    fn render(
        &self,
        asset_cache: &mut AssetCache,
        world: &World,
        use_mode: bool,
    ) -> Vec<SceneObject> {
        let mut glove_slot = self.glove_renderer.borrow_mut();
        let mut glove_renderer = glove_slot
            .get_or_insert_with(|| GloveRenderer::new(asset_cache))
            .as_mut();

        let mut objs = Vec::new();
        let support_grip = self.support.as_ref().and_then(|support| {
            let mut grip = self.fitted_grips[support.primary]
                .as_ref()?
                .resolved
                .clone()?;
            grip.curls = self.support_preview.as_ref()?.profile.curls;
            grip.trigger_curls = self.support_preview.as_ref()?.profile.trigger_curls;
            Some((1 - support.primary, grip))
        });
        for (index, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
            let grip = self.fitted_grips[index]
                .as_ref()
                .and_then(|g| g.resolved.as_ref())
                .or_else(|| {
                    support_grip
                        .as_ref()
                        .filter(|(i, _)| *i == index)
                        .map(|(_, g)| g)
                });
            objs.extend(hand.render(
                world,
                glove_renderer.as_deref_mut(),
                grip.map(|grip| {
                    (
                        grip,
                        self.support
                            .as_ref()
                            .filter(|s| 1 - s.primary == index && hand.get_held_entity().is_none())
                            .map_or(1.0, |s| s.blend),
                    )
                }),
                self.visual_hands[index],
            ));
        }
        if self.grip_overlay {
            use cgmath::{Matrix4, Rotation, vec3};
            for (index, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
                let Some(grip) = self.fitted_grips[index]
                    .as_ref()
                    .and_then(|g| g.resolved.as_ref())
                else {
                    continue;
                };
                for contact in grip.contacts.iter().flatten() {
                    let local = grip.offset
                        + grip
                            .rotation
                            .rotate_vector(vec3(contact[0], contact[1], contact[2]));
                    let point = hand.get_position() + hand.get_rotation().rotate_vector(local);
                    let mut marker = SceneObject::new(
                        engine::scene::color_material::create(vec3(0.0, 1.0, 1.0)),
                        Box::new(engine::scene::cube::create()),
                    );
                    marker.set_transform(
                        Matrix4::from_translation(point) * Matrix4::from_scale(0.008),
                    );
                    objs.push(marker);
                }
            }
        }
        // Feedback comes from the resolved holds, never a second proximity
        // query: a blocked pull still shows a catch; release/break removes it.
        // Float just above the fist so the glove cannot hide the marker.
        for (handedness, _) in self.hand_climb.grips() {
            let hand = match handedness {
                Handedness::Left => &self.left_hand,
                Handedness::Right => &self.right_hand,
            };
            let mut marker = SceneObject::new(
                engine::scene::color_material::create(cgmath::vec3(0.0, 1.0, 1.0)),
                Box::new(engine::scene::cube::create()),
            );
            marker.set_transform(
                cgmath::Matrix4::from_translation(
                    hand.get_position() + cgmath::vec3(0.0, 0.18, 0.0),
                ) * cgmath::Matrix4::from_scale(0.03),
            );
            objs.push(marker);
        }
        // Labelled as the player's hands: that is what `Game` drops while the
        // pause menu is up (issue #1018), and what `/v1/scene` reports. The
        // forearm panels carry their own label from `create_arm_hud_panels`,
        // which the `debug_hud` scene emits without going through here.
        crate::util::tag_render_source(&mut objs, crate::util::render_source::PLAYER_HANDS);
        if crate::dev_params::get_bool(crate::dev_params::VR_SUPPORT_GRIPS) {
            // Draw the same candidate geometry used for acquisition, including
            // model mirroring, scale and collision-resolved weapon transforms.
            if let Some(c) = &self.support_preview {
                use cgmath::{Matrix4, vec3};
                use engine::scene::{VertexPosition, color_material, lines_mesh};
                let socket = c.model_pose.point(c.anchor);
                let [a, b] = c
                    .region
                    .map_or([socket; 2], |ends| ends.map(|p| c.model_pose.point(p)));
                let mut vertices = Vec::new();
                dark::hit_box::append_capsule_lines(
                    &mut vertices,
                    &Matrix4::one(),
                    a,
                    b,
                    c.profile.grab_radius,
                );
                dark::hit_box::append_capsule_lines(
                    &mut vertices,
                    &Matrix4::one(),
                    socket,
                    socket,
                    0.008,
                );
                let attached = self
                    .support
                    .as_ref()
                    .is_some_and(|s| s.active && s.entity == c.entity && s.primary == c.primary);
                let mut overlay = vec![SceneObject::new(
                    color_material::create(if attached {
                        vec3(0.1, 1.0, 0.2)
                    } else {
                        vec3(0.1, 0.9, 1.0)
                    }),
                    Box::new(lines_mesh::create(vertices)),
                )];
                if let Some(rig) = &self.grip_kinematics {
                    let pose = self.hand_poses()[1 - c.primary];
                    if pose.is_tracked() {
                        let palm = pose.point(rig[1 - c.primary].palm);
                        let mut vertices = vec![
                            VertexPosition { position: palm },
                            VertexPosition { position: socket },
                        ];
                        dark::hit_box::append_capsule_lines(
                            &mut vertices,
                            &Matrix4::one(),
                            palm,
                            palm,
                            0.012,
                        );
                        overlay.push(SceneObject::new(
                            color_material::create(vec3(1.0, 0.65, 0.1)),
                            Box::new(lines_mesh::create(vertices)),
                        ));
                    }
                }
                crate::util::tag_render_source(&mut overlay, "vr_support_grip");
                objs.extend(overlay);
            }
        }
        objs.append(&mut create_arm_hud_panels(
            asset_cache,
            world,
            use_mode,
            self.left_hand.get_position(),
            self.left_hand.get_rotation(),
            self.right_hand.get_position(),
            self.right_hand.get_rotation(),
        ));
        objs
    }

    fn hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        let mut lights = Vec::new();
        if options.experimental_features.contains("enhanced_lighting") {
            for hand in [&self.right_hand, &self.left_hand] {
                let dir = hand.get_rotation() * Vector3::new(0.0, 0.0, -1.0);
                lights.push(SpotLight {
                    position: hand.get_position(),
                    direction: dir.normalize(),
                    color_intensity: Vector4::new(1.0, 1.0, 0.8, 2.0),
                    inner_cone_angle: 15.0_f32.to_radians(),
                    outer_cone_angle: 30.0_f32.to_radians(),
                    range: 10.0,
                });
            }
        }
        lights
    }

    fn grab(
        &mut self,
        world: &World,
        entity_id: EntityId,
        hand: Handedness,
    ) -> Vec<VirtualHandEffect> {
        // `Effect::GrabEntity` and save/load restoration enter through this
        // path instead of the per-frame hand ray, so enforce the same single-
        // owner invariant here too.
        if self.left_hand.is_holding(entity_id) || self.right_hand.is_holding(entity_id) {
            return Vec::new();
        }
        let index = if hand == Handedness::Left { 0 } else { 1 };
        if let Some(support) = self.support.as_mut() {
            if support.primary == index {
                self.support = None;
            } else {
                // A new offhand item must not snap the primary out of its decay.
                support.active = false;
            }
        }
        // Consumed squeeze/trigger input remains blocked until both release.
        self.support_preview = None;
        self.visual_hands = [None, None];
        if hand == Handedness::Left {
            self.left_hand = self.left_hand.grab_entity(world, entity_id);
        } else {
            self.right_hand = self.right_hand.grab_entity(world, entity_id);
        }
        Vec::new()
    }

    fn replace_entity(&mut self, old: EntityId, new: EntityId, rigid_body: RigidBodyHandle) {
        if self.support.as_ref().is_some_and(|s| s.entity == old) {
            self.support = None;
            self.support_preview = None;
            self.visual_hands = [None, None];
        }
        self.left_hand = self.left_hand.replace_entity(old, new, rigid_body);
        self.right_hand = self.right_hand.replace_entity(old, new, rigid_body);
    }

    fn on_entity_destroyed(&mut self, entity_id: EntityId) {
        if self.support.as_ref().is_some_and(|s| s.entity == entity_id) {
            self.support = None;
            self.support_preview = None;
            self.visual_hands = [None, None];
        }
        self.left_hand = self.left_hand.destroy_entity(entity_id);
        self.right_hand = self.right_hand.destroy_entity(entity_id);
    }

    fn holding_hand(&self, entity_id: EntityId) -> Option<Handedness> {
        if self.left_hand.is_holding(entity_id) {
            Some(Handedness::Left)
        } else if self.right_hand.is_holding(entity_id) {
            Some(Handedness::Right)
        } else {
            None
        }
    }
}

/// Flatscreen interaction: a single first-person weapon controller.
pub struct FlatInteraction {
    controller: FlatPlayerController,
    highlighted: Option<EntityId>,
}

impl FlatInteraction {
    pub fn new() -> Self {
        Self {
            controller: FlatPlayerController::new(),
            highlighted: None,
        }
    }
}

impl Default for FlatInteraction {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerInteraction for FlatInteraction {
    fn update(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect> {
        let (msgs, highlighted) = self.controller.update(
            &ctx.input.right_hand,
            ctx.player_pos,
            ctx.player_rotation,
            ctx.head_rotation,
            ctx.eye_height,
            ctx.world,
            ctx.physics,
        );
        self.highlighted = highlighted;
        msgs
    }

    fn held_entities(&self) -> (Option<EntityId>, Option<EntityId>) {
        (self.controller.wielded_entity(), None)
    }

    fn highlighted_entities(&self) -> Vec<EntityId> {
        self.highlighted.into_iter().collect()
    }

    fn highlighted_entity(&self, _hand: Handedness) -> Option<EntityId> {
        self.highlighted
    }

    fn viewmodel_entity(&self) -> Option<EntityId> {
        self.controller.wielded_entity()
    }

    fn grab(
        &mut self,
        _world: &World,
        entity_id: EntityId,
        _hand: Handedness,
    ) -> Vec<VirtualHandEffect> {
        self.controller.wield(entity_id)
    }

    fn replace_entity(&mut self, old: EntityId, new: EntityId, _rigid_body: RigidBodyHandle) {
        self.controller.replace_wielded(old, new);
    }

    fn on_entity_destroyed(&mut self, entity_id: EntityId) {
        self.controller.on_entity_destroyed(entity_id);
    }

    fn holding_hand(&self, entity_id: EntityId) -> Option<Handedness> {
        (self.controller.wielded_entity() == Some(entity_id)).then_some(Handedness::Right)
    }

    fn wield(&mut self, entity_id: EntityId) -> Vec<VirtualHandEffect> {
        self.controller.wield(entity_id)
    }

    fn is_wielding(&self) -> bool {
        self.controller.is_wielding()
    }

    fn flat_aim_ray(&self) -> Option<(Point3<f32>, Vector3<f32>)> {
        self.controller.aim_ray()
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{FrobFlag, PropFrobInfo, PropModelName};

    use super::*;
    use crate::{
        input_context::InputContext,
        physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape},
        scripts::MessagePayload,
    };

    fn identity() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }

    fn grabbable(world: &mut World) -> EntityId {
        world.add_entity((
            PropFrobInfo {
                world_action: FrobFlag::MOVE,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
            PropModelName("test_item".to_owned()),
        ))
    }

    fn context<'a>(
        world: &'a World,
        physics: &'a PhysicsWorld,
        input: &'a InputContext,
    ) -> InteractionContext<'a> {
        InteractionContext {
            physics,
            world,
            input,
            player_pos: vec3(0.0, 0.0, 0.0),
            player_rotation: identity(),
            head_rotation: identity(),
            eye_height: 1.04,
            step_dt: 1.0 / 60.0,
            support_enabled: true,
        }
    }

    fn step_physics(physics: &mut PhysicsWorld) {
        let player = EntityId::from_inner(10_000).unwrap();
        let mut player = physics.create_player(vec3(10.0, 10.0, 10.0), player);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
    }

    fn wrench_support_fixture() -> (World, EntityId, PhysicsWorld, VrInteraction, InputContext) {
        use crate::vr_grip::{GripKinematics, ResolvedGrip};
        let mut world = World::new();
        let entity = grabbable(&mut world);
        let physics = PhysicsWorld::new();
        let mut interaction = VrInteraction::new();
        interaction.grab(&world, entity, Handedness::Right);
        interaction.grip_kinematics = Some(std::array::from_fn(|_| GripKinematics {
            fingers: std::array::from_fn(|_| Vec::new()),
            palm: vec3(0.0, 0.0, 0.0),
            normal: Vector3::unit_x(),
        }));
        interaction.support_profiles.insert(
            "wrench_h".into(),
            SupportProfile {
                palm_anchor: [0.0, 0.2, 0.0],
                region: None,
                rotation_degrees: [0.0; 3],
                curls: [0.5; 5],
                trigger_curls: None,
                grab_radius: 0.07,
                release_distance: 0.12,
                max_swing_degrees: 75.0,
            },
        );
        interaction.fitted_grips[1] = Some(HeldGrip {
            model_mirror: Some(Handedness::Left.mirror()),
            entity,
            model: "wrench_h".into(),
            resolved: Some(ResolvedGrip {
                item_scale: 1.0,
                pose_family: "cylindrical".into(),
                offset: vec3(0.0, 0.0, 0.0),
                rotation: identity(),
                curls: [0.5; 5],
                trigger_curls: None,
                contacts: [None; 5],
                anchor: [0.0; 3],
                score: 0.0,
            }),
            solve_ms: 0.0,
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            source: "prepared",
            authored: false,
            item_bounds: None,
        });
        let mut input = InputContext::default();
        input.right_hand.position = vec3(0.0, 1.0, 0.0);
        input.right_hand.rotation = identity();
        input.right_hand.squeeze_value = 1.0;
        input.left_hand.position = vec3(0.0, 1.2, 0.0);
        input.left_hand.rotation = identity();
        input.left_hand.squeeze_value = 0.0;
        (world, entity, physics, interaction, input)
    }

    #[test]
    fn support_accepts_zero_dt_input_edges_and_requires_release_after_break() {
        let (mut world, entity, physics, mut interaction, mut input) = wrench_support_fixture();
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        let mut ctx = context(&world, &physics, &input);
        ctx.step_dt = 0.0;
        interaction.update_support(&ctx);
        assert!(interaction.support.as_ref().unwrap().active);
        // Supporting hand input is swallowed even on render-only frames.
        let effects = interaction.update(&ctx);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, VirtualHandEffect::HoldItem { .. }))
        );
        assert_eq!(interaction.held_entities(), (None, Some(entity)));
        // Simulate collision blocking the physical wrench away from its target.
        world.add_component(
            entity,
            (
                dark::properties::PropPosition {
                    position: vec3(0.0, 1.0, 0.3),
                    rotation: identity(),
                    cell: 0,
                },
                crate::runtime_props::RuntimePropVrGripOffset(vec3(0.0, 0.0, 0.1)),
            ),
        );
        interaction.support.as_mut().unwrap().blend = 1.0;
        interaction.synchronize_held_visuals(&world);
        let actual = interaction.support_preview.as_ref().unwrap();
        assert!((actual.model_pose.position - vec3(0.0, 1.0, 0.2)).magnitude() < 1e-5);
        assert!(
            (interaction.visual_hands[0].unwrap().position - vec3(0.0, 1.2, 0.2)).magnitude()
                < 1e-5
        );
        assert!(
            (interaction.visual_hands[1].unwrap().position - vec3(0.0, 1.0, 0.2)).magnitude()
                < 1e-5
        );
        let attachment = interaction.support.take();
        interaction.synchronize_held_visuals(&world);
        assert!(
            (interaction.visual_hands[1].unwrap().position - vec3(0.0, 1.0, 0.2)).magnitude()
                < 1e-5,
            "single-hand melee also follows the physical body"
        );
        interaction.support = attachment;
        world.remove::<crate::runtime_props::RuntimePropVrGripOffset>(entity);
        // A trigger consumed while supporting must not leak on squeeze release.
        input.left_hand.trigger_value = 1.0;
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support_blocked[0]);
        input.left_hand.trigger_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support_blocked[0]);
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        input.left_hand.position.y = 2.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        input.left_hand.position.y = 1.2;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        input.left_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        input.left_hand.rotation = identity();
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        // A broad region chooses a fresh nearest point only on a new squeeze.
        interaction.support = None;
        interaction
            .support_profiles
            .get_mut("wrench_h")
            .unwrap()
            .region = Some(crate::vr_support::SupportRegion {
            start: [0.0, 0.2, 0.0],
            end: [0.0, 0.35, 0.0],
        });
        input.left_hand.squeeze_value = 0.0;
        let mut untracked = interaction.hand_poses();
        untracked[0].position.x = f32::NAN;
        let preview = interaction
            .support_candidate(&world, untracked, false)
            .unwrap();
        assert_eq!(preview.anchor, vec3(0.0, 0.2, 0.0));
        assert!(
            preview.hand_pose.is_tracked(),
            "untracked support input keeps a finite region preview"
        );
        input.left_hand.position.y = 1.22;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        let locked = interaction.support.as_ref().unwrap().anchor;
        assert!((locked.y - 0.22).abs() < 1e-5);
        input.left_hand.position.y = 1.28;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        assert_eq!(interaction.support.as_ref().unwrap().anchor, locked);
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!((interaction.support.as_ref().unwrap().anchor.y - 0.28).abs() < 1e-5);
        interaction.support.as_mut().unwrap().blend = 1.0;
        let mut ctx = context(&world, &physics, &input);
        ctx.support_enabled = false;
        interaction.update_support(&ctx);
        assert!(!interaction.support.as_ref().unwrap().active);
        let other_item = grabbable(&mut world);
        interaction.grab(&world, other_item, Handedness::Left);
        interaction.apply_support(&world, &mut Vec::new());
        // The owning wrench keeps its release blend while the new item gets
        // its own grip; there is no support-hand visual attached to the wrench.
        assert!(interaction.support.as_ref().is_some_and(|s| !s.active));
        assert_eq!(
            interaction.held_entities(),
            (Some(other_item), Some(entity))
        );
    }

    #[test]
    fn physical_support_calibration_ignores_body_recovery_and_wrist_twists() {
        use cgmath::{Deg, Rotation3};
        let (mut world, entity, physics, mut interaction, mut input) = wrench_support_fixture();
        world.add_component(
            entity,
            (
                dark::properties::PropPosition {
                    position: vec3(0.8, 1.0, 0.3),
                    rotation: identity(),
                    cell: 0,
                },
                crate::runtime_props::RuntimePropVrGripOffset(vec3(0.0, 0.0, 0.1)),
            ),
        );
        input.left_hand.position = vec3(0.8, 1.2, 0.2);
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        let offset = vec3(0.8, 0.0, 0.2);
        assert!(interaction.support.as_ref().unwrap().active);
        assert!((interaction.support.as_ref().unwrap().primary_offset - offset).magnitude() < 1e-5);
        // The collision clears all the way back to the primary controller.
        world.add_component(
            entity,
            dark::properties::PropPosition {
                position: vec3(0.0, 1.0, 0.1),
                rotation: identity(),
                cell: 0,
            },
        );
        interaction.update_support(&context(&world, &physics, &input));
        assert!(
            interaction.support.as_ref().unwrap().active,
            "body recovery cannot release unchanged controllers"
        );
        // Twist around the handle axis: the calibrated pivot must not orbit the wrist.
        input.right_hand.rotation = Quaternion::from_angle_y(Deg(90.0));
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        assert!((interaction.support.as_ref().unwrap().primary_offset - offset).magnitude() < 1e-5);
        // A player turn, unlike a wrist twist, rotates both controller references.
        let yaw = Quaternion::from_angle_y(Deg(90.0));
        let mut ctx = context(&world, &physics, &input);
        ctx.player_rotation = yaw;
        interaction.update_support(&ctx);
        assert!(interaction.support.as_ref().unwrap().active);
        assert!(
            (interaction.support.as_ref().unwrap().primary_offset - yaw.rotate_vector(offset))
                .magnitude()
                < 1e-5
        );
        // A new squeeze at the now-unblocked socket discards the old calibration.
        input.right_hand.rotation = identity();
        input.left_hand.position = vec3(0.0, 1.2, 0.0);
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        assert!(
            interaction
                .support
                .as_ref()
                .unwrap()
                .primary_offset
                .magnitude()
                < 1e-5
        );
    }

    #[test]
    fn one_item_cannot_be_grabbed_by_both_hands_on_the_same_frame() {
        let mut world = World::new();
        let item = grabbable(&mut world);
        let mut physics = PhysicsWorld::new();
        physics.add_dynamic(
            item,
            vec3(0.0, 0.0, -0.5),
            identity(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.5, 0.5, 0.5)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        step_physics(&mut physics);
        let mut input = InputContext::default();
        input.left_hand.squeeze_value = 1.0;
        input.right_hand.squeeze_value = 1.0;
        let mut interaction = VrInteraction::new();

        let effects = interaction.update(&context(&world, &physics, &input));

        assert_eq!(
            interaction.held_entities(),
            (None, Some(item)),
            "right-hand update wins ties; the left hand must see that the item is already held"
        );
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(effect, VirtualHandEffect::HoldItem { entity_id } if *entity_id == item))
                .count(),
            1,
            "one physical item must produce exactly one hold transition"
        );
    }

    #[test]
    fn a_held_item_does_not_receive_its_own_hands_hover() {
        let mut world = World::new();
        let item = grabbable(&mut world);
        let mut physics = PhysicsWorld::new();
        physics.add_dynamic(
            item,
            vec3(0.0, 0.0, -0.5),
            identity(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.5, 0.5, 0.5)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        step_physics(&mut physics);
        let mut input = InputContext::default();
        input.left_hand.position = vec3(10.0, 0.0, 0.0);
        input.right_hand.squeeze_value = 1.0;
        let mut interaction = VrInteraction::new();
        interaction.grab(&world, item, Handedness::Right);

        let effects = interaction.update(&context(&world, &physics, &input));

        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::OutMessage { message }
                    if message.to == item && matches!(message.payload, MessagePayload::Hover { .. })
            )),
            "a holding hand's ray must pass through its own item"
        );
    }

    #[test]
    fn externally_requested_grab_does_not_duplicate_an_existing_hold() {
        let mut world = World::new();
        let item = grabbable(&mut world);
        let mut interaction = VrInteraction::new();

        interaction.grab(&world, item, Handedness::Left);
        interaction.grab(&world, item, Handedness::Right);

        assert_eq!(interaction.held_entities(), (Some(item), None));
    }
}
