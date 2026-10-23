//! Player interaction abstraction.
//!
//! One trait with two implementations - VR (two `VirtualHand`s) and flatscreen
//! (`FlatPlayerController`) - so `mission_core` drives interaction
//! polymorphically through a single `Box<dyn PlayerInteraction>` instead of
//! branching on `PresentationMode` and holding both sets of state.
//!
//! Both implementations speak the same `VirtualHandEffect` language, which
//! `mission_core` already processes in one place.

mod climb_visual;
mod pose_tool;
mod slide;

use std::{cell::RefCell, collections::HashMap};

use cgmath::{InnerSpace, Matrix4, One, Point3, Quaternion, Rotation, Vector3, Vector4};
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
    hud::create_wrist_hud_panels,
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
    /// Shared pawn-space eye for flat rendering, aiming and weapon placement.
    pub flat_eye: crate::death_camera::EyePose,
    pub step_dt: f32,
    pub support_enabled: bool,
    /// Releases already claimed by inventory or body storage cannot be handed off.
    pub reserved_releases: &'a [EntityId],
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
    /// Standing on something ends a ladder departure's grace grabs.
    pub grounded: bool,
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

    /// Refresh contacts on moving parts after their deferred joint poses apply.
    fn synchronize_held_articulation(&mut self, _world: &World) {}

    fn maintenance_previews(&self, _world: &World) -> Vec<String> {
        Vec::new()
    }

    /// Per hand (left, right): the held item and what releasing it now would
    /// offer it to. Flatscreen has no hand release.
    fn release_offers(&self) -> [Option<(EntityId, EntityId)>; 2] {
        [None; 2]
    }

    fn hand_feedback_diagnostics(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

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

    /// Which free (left, right) gloves glow in reach of a hold they would take.
    fn climb_reach(&self) -> [bool; 2] {
        [false; 2]
    }

    /// Feed collision-resolved motion back into the held tracking reference.
    fn resolve_hand_climb(&mut self, _requested: Vector3<f32>, _applied: Vector3<f32>) {}

    /// Drop every climb hold without throwing the body - the vault took over
    /// (see [`crate::vr_climb::HandClimb::release_all`]).
    fn release_climb_grips(&mut self) {}

    /// A climb exit owns the body: take no hold, but keep tracking squeezes
    /// (see [`crate::vr_climb::HandClimb::track_squeezes_only`]).
    fn suspend_hand_climb(&mut self, _input: &InputContext) {}

    /// Whether letting go of every hold now lands safely, for the held gloves
    /// (see [`crate::climb_release`]).
    fn set_climb_release_safe(&mut self, _safe: bool) {}

    /// Entities held in (left, right) - for `PlayerInfo`. Flat reports its
    /// wielded weapon as the "left".
    fn held_entities(&self) -> (Option<EntityId>, Option<EntityId>);

    /// Start or stop authoring the held item's grip with tracked hands
    /// (VR only; see `interaction::pose_tool`).
    fn toggle_grip_pose_tool(&mut self) {}

    /// Whether a hand holds an item the grip pose tool can pose.
    fn can_pose_held_item(&self) -> bool {
        false
    }

    /// Whether the grip pose tool is open: its workbench holds the mission
    /// still and owns the hands.
    fn pose_workbench_open(&self) -> bool {
        false
    }

    /// Drive the open workbench with this frame's (rig-shifted) input and the
    /// world height of the player's real floor; the mission is not updated
    /// meanwhile.
    fn update_pose_workbench(
        &mut self,
        _world: &World,
        _input: &InputContext,
        _player_pos: Vector3<f32>,
        _player_rotation: Quaternion<f32>,
        _floor_y: f32,
        _dt: f32,
    ) {
    }

    /// The placed workbench, for the mission to draw the item on it.
    fn pose_workbench(&self) -> Option<PoseWorkbench> {
        None
    }

    /// A face button pressed on the open workbench: its control hand saves
    /// (lower) or discards (upper) the staged capture.
    fn pose_tool_button(&mut self, _hand: Handedness, _button: crate::hand_buttons::HandButton) {}

    /// A frame the open workbench is skipped (the pause menu is over it).
    fn hold_pose_workbench(&mut self) {}

    /// Controller pulses the workbench asked for since the last call (its
    /// clicks and saves), for the mission to request.
    fn take_pose_tool_pulses(&mut self) -> Vec<(Handedness, crate::haptics::HapticPulse)> {
        Vec::new()
    }

    /// A latched support grip, rather than proximity or the visual blend-out.
    fn is_supported(&self, _entity: EntityId) -> bool {
        false
    }

    /// Recoil's deflection of the flat fire ray off the camera axis, in
    /// radians (+x right, +y up) - what the reticle shifts by. VR aims with the
    /// gun itself and has no crosshair to deflect.
    fn flat_aim_bias(&self) -> cgmath::Vector2<f32> {
        cgmath::vec2(0.0, 0.0)
    }

    /// Kick a firing weapon's presentation-owned recoil spring. VR kicks the
    /// held rigid body instead (`PhysicsWorld::kick_held_gun`), so only the
    /// flat viewmodel implements this.
    fn kick_viewmodel(&mut self, _entity: EntityId, _impulse: crate::weapon_recoil::RecoilImpulse) {
    }

    /// Primary palm anchor in scaled held-model coordinates, when fitted.
    fn held_grip_anchor(&self, _entity: EntityId) -> Option<Vector3<f32>> {
        None
    }

    /// Entities under the reticle/hands, for the hover-highlight overlay.
    fn highlighted_entities(&self) -> Vec<EntityId>;

    /// Flat's reach-bounded crosshair target, where a cursor tool is applied.
    /// VR applies tools by touch instead.
    fn frob_target(&self) -> Option<EntityId> {
        None
    }

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
        _lighting: Option<&crate::object_lighting::ObjectLighting<'_>>,
    ) -> Vec<SceneObject> {
        Vec::new()
    }

    /// Forearm sockets use the same calibrated, collision-resolved wrists as the HUD.
    fn implant_socket_frames(&self, _world: &World) -> [Option<Matrix4<f32>>; 2] {
        [None; 2]
    }

    /// Hand-mounted spotlights (`hand_spotlights` dev param).
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

    /// Restore saved ownership without treating neutral startup input as a release.
    fn restore_held(
        &mut self,
        world: &World,
        entity_id: EntityId,
        hand: Handedness,
    ) -> Vec<VirtualHandEffect> {
        self.grab(world, entity_id, hand)
    }

    /// A held entity was recreated as `new`; track the new id.
    fn replace_entity(&mut self, old: EntityId, new: EntityId, rigid_body: RigidBodyHandle);

    /// A held entity was destroyed; release it.
    fn on_entity_destroyed(&mut self, entity_id: EntityId);

    /// Knock `entity_id` out of the VR hands holding or supporting it; they
    /// ignore trigger and grip until released. The holding hand, or `None`
    /// when no hand holds it.
    fn fumble(&mut self, _entity_id: EntityId) -> Option<Handedness> {
        None
    }

    /// Which hand is holding `entity_id`, if any. A VR wield needs this to
    /// pick which way round to draw a first-person arm rig (they are all
    /// authored right-handed). Flat has one wield hand and reports it as the
    /// right, matching the right-hand trigger it fires with.
    fn holding_hand(&self, entity_id: EntityId) -> Option<Handedness>;

    /// Primary and actively attached support hand for controller feedback.
    /// Flatscreen and unheld weapons produce no controller output.
    fn haptic_hands(&self, _entity_id: EntityId) -> [Option<Handedness>; 2] {
        [None; 2]
    }

    /// Tracked palm on the firing side of a held weapon's muzzle offset.
    fn held_launch_origin(&self, _entity_id: EntityId) -> Option<Point3<f32>> {
        None
    }

    /// Whether `entity_id` is currently held. Answered from
    /// [`Self::holding_hand`] so "is it held" and "which hand holds it" cannot
    /// give different answers.
    fn is_holding(&self, entity_id: EntityId) -> bool {
        self.holding_hand(entity_id).is_some()
    }

    /// Body-slot retrieval cannot share a hand with support or climbing.
    fn hand_available_for_body_slot(&self, _hand: Handedness) -> bool {
        false
    }

    /// Available body-slot contents, resolved by the same pass that owns retrieval.
    fn set_body_anticipation(&mut self, _targets: [Option<EntityId>; 2]) {}

    /// Hands a VR UI panel is listening to this frame; they ease into a point.
    fn set_ui_point(&mut self, _hands: [bool; 2]) {}

    fn set_body_tool_device(&mut self, _device: bool) {}

    fn personal_card_grip(&self, _hand: usize) -> Option<crate::vr_grip::ResolvedGrip> {
        None
    }

    /// Permanent body tools reserve hands without inventing inventory entities.
    fn reserve_body_tool_hands(&mut self, _reserved: [bool; 2]) {}

    /// Current tracked palms in pawn space, using the same calibrated glove
    /// kinematics as fitted items and support grips. Missing rigs are disarmed.
    fn body_palm_positions(&self, _input: &InputContext) -> [Option<Vector3<f32>>; 2] {
        [None; 2]
    }

    /// Final visible glove cuffs, in world space (left, right).
    fn body_wrist_frames(&self) -> [Option<Matrix4<f32>>; 2] {
        [None; 2]
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
    body_hand_tracking: [bool; 2],
    left_hand: VirtualHand,
    right_hand: VirtualHand,
    /// Lazily initialized on first render (needs the asset cache). The outer
    /// `Option` is "have we tried yet" - a glove that failed to load stays
    /// `Some(None)` so we don't hit the asset cache's miss path every frame.
    glove_renderer: RefCell<Option<Option<GloveRenderer>>>,
    /// Which hands hold a climbing hold, and which one moves the body.
    hand_climb: crate::vr_climb::HandClimb,
    climb_members: [Option<climb_visual::HeldMember>; 2],
    climb_visuals: [climb_visual::ClimbVisual; 2],
    grip_kinematics: Option<[crate::vr_grip::GripKinematics; 2]>,
    grip_geometry: HashMap<String, Option<GripGeometry>>,
    magazine_grips: HashMap<String, Option<crate::vr_grip::ResolvedGrip>>,
    kinematics_hashes: [String; 2],
    grip_library: Option<crate::vr_grip::GripLibrary>,
    grip_hints: HashMap<String, crate::vr_grip::GripHints>,
    grip_overlay: bool,
    fitted_grips: [Option<HeldGrip>; 2],
    world_scale: crate::world_scale::WorldScale,
    support_profiles: HashMap<String, SupportProfile>,
    support: Option<SupportAttachment>,
    slide: Option<slide::SlideAttachment>,
    slide_reserved: Option<usize>,
    slide_preview: Option<slide::SlideContact>,
    support_preview: Option<SupportCandidate>,
    support_pressed: [bool; 2],
    support_blocked: [bool; 2],
    body_tool_hands: [bool; 2],
    body_tool_device: bool,
    visual_hands: [Option<GripPose>; 2],
    step_dt: f32,
    anticipation: [crate::hand_anticipation::HandAnticipation; 2],
    body_anticipation: [Option<EntityId>; 2],
    climb_anticipation: [bool; 2],
    /// Last anticipation pass's climb reach, kept for the glove glow.
    climb_reach: [bool; 2],
    /// Seconds each hand has held its climb hold: a fresh hold flashes cyan
    /// before the glove shows the release's green/red.
    climb_held_seconds: [f32; 2],
    climb_release_safe: bool,
    ui_point: [bool; 2],
    /// Developer grip authoring with tracked hands (`toggle_grip_pose_tool`).
    pose_tool: Option<pose_tool::GripPoseTool>,
    /// Its controller pulses, until the mission takes them.
    pose_tool_pulses: Vec<(Handedness, crate::haptics::HapticPulse)>,
}

/// A held item's per-model grip contact offset (`RuntimePropVrGripOffset`).
pub(crate) fn grip_contact(world: &World, entity: EntityId) -> Vector3<f32> {
    world
        .borrow::<shipyard::View<crate::runtime_props::RuntimePropVrGripOffset>>()
        .ok()
        .and_then(|v| shipyard::Get::get(&v, entity).ok().map(|p| p.0))
        .unwrap_or(Vector3::new(0.0, 0.0, 0.0))
}

struct SupportAttachment {
    entity: EntityId,
    primary: usize,
    active: bool,
    blend: f32,
    correction: Quaternion<f32>,
    anchor: Vector3<f32>,         // Closed-pose contact, fixed until release.
    pump_separation_offset: f32,  // Controller distance minus rail distance at acquisition.
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
    closed_anchor: Vector3<f32>,
    pump: Option<(crate::vr_support::SupportMotion, Vector3<f32>, f32)>,
    region: Option<[Vector3<f32>; 2]>,
    model_pose: GripPose,
    hand_pose: GripPose,
}

struct GripGeometry {
    triangles: std::rc::Rc<Vec<[Point3<f32>; 3]>>,
    fingerprint: String,
    weapon_arms: Option<Vec<Point3<f32>>>,
}

/// What the mission draws for the grip pose workbench instead of itself: the
/// held item's world pose and grip scale there, its model frame for the posed
/// hand if mirrored from the holder's, how the bench is turned for viewing,
/// each extra view, the workbench's lights and its floor.
pub struct PoseWorkbench {
    pub entity: EntityId,
    pub item: GripPose,
    pub item_scale: f32,
    pub mirror: Option<Matrix4<f32>>,
    pub turn: Matrix4<f32>,
    pub views: Option<[Matrix4<f32>; 3]>,
    pub lights: Option<std::rc::Rc<engine::scene::light::LightArray>>,
    pub environment: Vec<SceneObject>,
}

/// A pose-tool capture applied live but not yet saved, and what the first
/// unsaved capture replaced, so discarding restores it: a grip, or in
/// support mode the item's support pose.
enum StagedGrip {
    Grip {
        entry: crate::vr_grip::BakedGripEntry,
        previous: Option<HeldGrip>,
        hand_pose: GripPose,
    },
    Support {
        model: String,
        pose: crate::vr_support::SupportPose,
        previous: SupportProfile,
    },
}

#[derive(Clone)]
struct HeldGrip {
    /// A glove weapon's right-hand model frame into its left's
    /// (`vr_weapon_grip::model_mirror`); `None` for an item drawn alike in
    /// either hand.
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
    fn primary_anchor(&self, palm: Vector3<f32>) -> Option<Vector3<f32>> {
        let grip = self.resolved.as_ref()?;
        Some(grip.rotation.conjugate().rotate_vector(palm - grip.offset))
    }

    /// Resolve the physical weapon pose, undoing melee's contact-origin split.
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
        let contact = match offsets.get(self.entity) {
            Ok(offset) => offset.0,
            Err(_) => {
                crate::mission::mission_core::held_item_collision_group(world, self.entity)?;
                Vector3::new(0.0, 0.0, 0.0)
            }
        };
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
            body_hand_tracking: [false; 2],
            hand_climb: crate::vr_climb::HandClimb::default(),
            climb_members: [None; 2],
            climb_visuals: Default::default(),
            grip_kinematics: None,
            grip_geometry: HashMap::new(),
            magazine_grips: HashMap::new(),
            kinematics_hashes: [String::new(), String::new()],
            grip_library: None,
            grip_hints: HashMap::new(),
            grip_overlay: false,
            fitted_grips: [None, None],
            world_scale: crate::world_scale::WorldScale::active(),
            support_profiles: HashMap::new(),
            support: None,
            slide: None,
            slide_reserved: None,
            slide_preview: None,
            support_preview: None,
            support_pressed: [true; 2],
            support_blocked: [false; 2],
            body_tool_hands: [false; 2],
            body_tool_device: false,
            visual_hands: [None, None],
            step_dt: 0.0,
            anticipation: Default::default(),
            body_anticipation: [None; 2],
            climb_anticipation: [false; 2],
            climb_reach: [false; 2],
            climb_held_seconds: [0.0; 2],
            climb_release_safe: false,
            ui_point: [false; 2],
            pose_tool: None,
            pose_tool_pulses: Vec::new(),
        }
    }
}

/// How close an empty, squeezing hand must be to catch a released item.
/// Matches the nearby tool-use reach.
const HANDOFF_REACH: f32 = 0.3;

impl Default for VrInteraction {
    fn default() -> Self {
        Self::new()
    }
}

impl VrInteraction {
    /// Resolve a deliberate nearby catch before either hand can drop or ray-grab.
    fn try_handoff(&mut self, ctx: &InteractionContext) -> Option<Vec<VirtualHandEffect>> {
        // Use this frame's controller poses, not last frame's hands or a
        // weapon displaced by world collision.
        if ctx
            .input
            .pose_tracking
            .is_some_and(|p| !p.head || !p.hands[0] || !p.hands[1])
        {
            return None;
        }
        let inputs = [&ctx.input.left_hand, &ctx.input.right_hand];
        let poses = inputs.map(|input| GripPose {
            position: hand_world_position(ctx.player_pos, ctx.player_rotation, input.position),
            rotation: ctx.player_rotation * input.rotation,
        });
        if poses.iter().any(|pose| !pose.is_tracked())
            || (poses[0].position - poses[1].position).magnitude() > HANDOFF_REACH
        {
            return None;
        }
        let hands = [&self.left_hand, &self.right_hand];
        let giver = (0..2).find(|&i| {
            hands[i].released_entity(inputs[i]).is_some_and(|entity| {
                !ctx.reserved_releases.contains(&entity)
                    && hands[1 - i].get_held_entity().is_none()
                    && self.slide_reserved != Some(1 - i)
                    && inputs[1 - i].squeeze_value > 0.5
                    && !self
                        .hand_climb
                        .grips()
                        .any(|(hand, _)| crate::vr_config::hand_slot(hand) == 1 - i)
            })
        })?;
        let receiver = 1 - giver;
        let entity = hands[giver].get_held_entity()?;
        // Refresh both inputs without world interactions: an already-held
        // trigger must not fire/frob the newly received object. Reset motion so
        // a later throw belongs to the receiving hand, not its approach.
        let emptied = hands[giver].destroy_entity(entity).update_suppressed(
            ctx.player_pos,
            ctx.player_rotation,
            inputs[giver],
        );
        let received = hands[receiver]
            .update_suppressed(ctx.player_pos, ctx.player_rotation, inputs[receiver])
            .grab_entity(ctx.world, entity);
        let (received, mut pose_effects) = VirtualHand::update(
            &received,
            ctx.physics,
            ctx.world,
            ctx.player_pos,
            ctx.player_rotation,
            inputs[receiver],
            None,
            poses[giver].position,
            ctx.step_dt,
        );
        if giver == 0 {
            self.left_hand = emptied;
            self.right_hand = received;
        } else {
            self.right_hand = emptied;
            self.left_hand = received;
        }
        self.support = None;
        self.support_preview = None;
        self.support_blocked = [false; 2];
        self.visual_hands = [None, None];

        // Keep script release/acquisition semantics, but never create a loose
        // body, seed a throw, or offer this object for consumption elsewhere.
        let mut effects = vec![
            VirtualHandEffect::OutMessage {
                message: crate::scripts::Message {
                    to: entity,
                    payload: crate::scripts::MessagePayload::Drop,
                },
            },
            VirtualHandEffect::HoldItem { entity_id: entity },
        ];
        effects.append(&mut pose_effects);
        Some(effects)
    }

    fn update_anticipation(&mut self, ctx: &InteractionContext) {
        use crate::{hand_anticipation::Target, hand_feedback::HandAffordance};
        use shipyard::{Get, View};
        // Paused updates skip the climb prepass; keep the last resolved reach
        // for rendering. A simulated frame consumes the fresh target (or clears
        // it when a mantle skips the prepass).
        let climb = if ctx.step_dt > 0.0 {
            std::mem::take(&mut self.climb_anticipation)
        } else {
            self.climb_reach
        };
        self.climb_reach = climb;
        for (i, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
            let tracked = ctx.input.pose_tracking.is_none_or(|p| p.head && p.hands[i])
                && (GripPose {
                    position: hand.get_position(),
                    rotation: hand.get_rotation(),
                })
                .is_tracked();
            if !tracked
                || hand.get_held_entity().is_some()
                || self.support_blocked[i]
                || self.body_tool_hands[i]
            {
                self.anticipation[i] = Default::default();
                self.climb_reach[i] = false;
                continue;
            }
            let object = self.body_anticipation[i].or_else(|| hand.nearby_grab_target());
            // Climbing resolves before ordinary pickup rays, so a nearby rung
            // wins even when that hand points through it at a loose item.
            let target = if self.ui_point[i] {
                Target::UiPoint
            } else if climb[i] {
                Target::Grab([1.0; 5])
            } else if let Some(entity) = object {
                // Weapons change from their world mesh to the authored hand mesh
                // on pickup; prepare the same grip that will be visible then.
                let model = ctx
                    .world
                    .borrow::<View<dark::properties::PropPlayerGun>>()
                    .ok()
                    .and_then(|guns| guns.get(entity).ok().map(|g| g.hand_model.to_lowercase()))
                    .or_else(|| {
                        ctx.world
                            .borrow::<View<dark::properties::PropLimbModel>>()
                            .ok()
                            .and_then(|models| models.get(entity).ok().map(|m| m.0.to_lowercase()))
                    })
                    .or_else(|| {
                        ctx.world
                            .borrow::<View<dark::properties::PropModelName>>()
                            .ok()
                            .and_then(|models| models.get(entity).ok().map(|m| m.0.to_lowercase()))
                    });
                let curls = model
                    .and_then(|model| {
                        // The grip the hand will take; fingers are alike mirrored.
                        self.grip_library
                            .as_ref()?
                            .select(
                                &model,
                                [Handedness::Left, Handedness::Right][i],
                                crate::vr_weapon_grip::supports_model(&model),
                            )
                            // Bone poses pre-curl with the generic grab shape.
                            .and_then(|(grip, _)| grip.fingers.rest_curls().copied())
                    })
                    .unwrap_or([0.85, 0.6, 0.9, 0.9, 0.9]);
                Target::Grab(curls)
            } else if hand.affordance() == HandAffordance::Frobbable {
                Target::Point
            } else {
                Target::None
            };
            self.anticipation[i].update(target, ctx.step_dt);
        }
    }

    fn hand_poses(&self) -> [GripPose; 2] {
        [&self.left_hand, &self.right_hand].map(|hand| GripPose {
            position: hand.get_position(),
            rotation: hand.get_rotation(),
        })
    }

    fn pump_motion(
        &self,
        world: &World,
        primary: usize,
    ) -> Option<(crate::vr_support::SupportMotion, Vector3<f32>)> {
        use cgmath::Transform;
        use shipyard::{Get, View};
        let held = self.fitted_grips[primary].as_ref()?;
        if held.model != "sg_h" {
            return None;
        }
        world
            .borrow::<View<crate::runtime_props::RuntimePropGloveWeapon>>()
            .ok()?
            .get(held.entity)
            .ok()?;
        let motion = self.support_profiles.get(&held.model)?.motion?;
        let rigs = world
            .borrow::<View<crate::runtime_props::RuntimePropObjectArticulation>>()
            .ok()?;
        let mut travel = motion.travel(&rigs.get(held.entity).ok()?.0)?;
        if primary == 0 {
            travel = held.model_mirror?.transform_vector(travel);
        }
        Some((motion, travel * held.resolved.as_ref()?.item_scale))
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
            let primary_palm = poses[primary].point(rig[primary].palm_in_world());
            // Scaled model-space anchors, independent of the melee contact-origin split.
            let primary_anchor = held.primary_anchor(rig[primary].palm_in_world())?;
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
            let control_primary_palm = self
                .support
                .as_ref()
                .filter(|s| prefer_locked_anchor && s.entity == held.entity && s.primary == primary)
                .map_or(model_pose.point(primary_anchor), |s| {
                    primary_palm + s.primary_offset
                });
            let pump = self.pump_motion(world, primary);
            let old_fraction = crate::vr_shotgun_pump::state(world, held.entity).fraction;
            let contact_pose = GripPose {
                position: model_pose.point(
                    pump.map_or(Vector3::new(0.0, 0.0, 0.0), |(_, travel)| {
                        travel * old_fraction
                    }),
                ),
                ..model_pose
            };
            let closed_anchor = self
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
                            contact_pose,
                            poses[1 - primary].point(rig[1 - primary].palm_in_world()),
                        )
                    },
                    |s| s.anchor,
                );
            let pump = pump.and_then(|(motion, travel)| {
                let separation = (poses[1 - primary].point(rig[1 - primary].palm_in_world())
                    - control_primary_palm)
                    .magnitude();
                let fraction = crate::vr_support::pump_fraction_from_separation(
                    closed_anchor - primary_anchor,
                    travel,
                    separation
                        - self
                            .support
                            .as_ref()
                            .filter(|s| {
                                prefer_locked_anchor
                                    && s.entity == held.entity
                                    && s.primary == primary
                            })
                            .map_or(0.0, |s| s.pump_separation_offset),
                )?;
                let moving = prefer_locked_anchor
                    && self.step_dt > 0.0
                    && poses.iter().all(|p| p.is_tracked())
                    && self.support.as_ref().is_some_and(|s| {
                        s.active && s.entity == held.entity && s.primary == primary
                    });
                Some((motion, travel, if moving { fraction } else { old_fraction }))
            });
            let anchor = closed_anchor
                + pump.map_or(Vector3::new(0.0, 0.0, 0.0), |(_, travel, fraction)| {
                    travel * fraction
                });
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
                control_primary_palm,
                primary_anchor,
                tracked_axis: base_rotation.rotate_vector(anchor - primary_anchor),
                anchor,
                closed_anchor,
                pump,
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
        let tracking = ctx
            .input
            .pose_tracking
            .is_none_or(|p| p.head && p.hands.iter().all(|v| *v));
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
                let separation = (poses[other].point(rig[other].palm_in_world())
                    - c.control_primary_palm)
                    .magnitude();
                // Input can change during a paused redraw. Test retention
                // against the whole rail then, but only advance the part on a
                // simulation step: a full stroke must not look like a release.
                let anchor = c
                    .pump
                    .and_then(|(_, travel, _)| {
                        crate::vr_support::pump_fraction_from_separation(
                            c.closed_anchor - c.primary_anchor,
                            travel,
                            separation - support.pump_separation_offset,
                        )
                        .map(|f| c.closed_anchor + travel * f)
                    })
                    .unwrap_or(c.anchor);
                support.entity == c.entity
                    && support.primary == c.primary
                    && ctx.support_enabled
                    && available(c.primary)
                    && tracking
                    && poses.iter().all(|p| p.is_tracked())
                    && pressed[c.primary]
                    && pressed[other]
                    && separation > 0.06
                    && c.profile.allows_swing(
                        c.tracked_axis,
                        poses[other].point(rig[other].palm_in_world()) - c.control_primary_palm,
                    )
                    && (separation
                        - support.pump_separation_offset
                        - (anchor - c.primary_anchor).magnitude())
                    .abs()
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
                    let palm = poses[other].point(rig[other].palm_in_world());
                    if available(c.primary)
                        && tracking
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
                            anchor: c.closed_anchor,
                            pump_separation_offset: if c.pump.is_some() {
                                (palm - c.control_primary_palm).magnitude()
                                    - (c.anchor - c.primary_anchor).magnitude()
                            } else {
                                0.0
                            },
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
                    c.primary_palm
                        + (poses[other].point(rig[other].palm_in_world()) - c.control_primary_palm),
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
            let contact = grip_contact(world, c.entity);
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

impl VrInteraction {
    /// One workbench frame: place it on the first tracked frame, track the
    /// hands, scale the item and stage a queued capture. Input is pawn-local
    /// (rig-shifted); the frozen pawn maps it into the world.
    fn update_pose_tool(
        &mut self,
        world: &World,
        input: &InputContext,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        floor_y: f32,
        dt: f32,
    ) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let holding = [&self.left_hand, &self.right_hand][tool.holder].get_held_entity();
        if holding != Some(tool.entity) {
            println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL active=false reason=item_released");
            self.close_pose_tool();
            return;
        }
        tool.switch = pose_hand_switch(
            tool,
            self.fitted_grips[tool.hand].as_ref(),
            self.grip_library.as_ref(),
        )
        .map(|_| ());
        let frame = pose_frame(tool, self.fitted_grips[tool.hand].as_ref());
        tool.support_mode = support_mode(
            tool,
            self.fitted_grips[tool.hand].as_ref(),
            &self.support_profiles,
        );
        let Some(grip) = self.fitted_grips[tool.hand]
            .as_mut()
            .and_then(|held| held.resolved.as_mut())
        else {
            return;
        };
        let posing = tool.posing();
        let tracking = input.pose_tracking;
        let head = crate::util::tracked_rotation(input.head.rotation)
            .filter(|_| tracking.is_none_or(|t| t.head))
            .map(|rotation| {
                (
                    hand_world_position(player_pos, player_rotation, input.head.position),
                    player_rotation * rotation,
                )
            });
        let contact = grip_contact(world, tool.entity);
        if !tool.placed {
            // Placed once, from a tracked head; until then nothing changes.
            let Some((eye, gaze)) = head else {
                return;
            };
            tool.place(
                eye,
                gaze,
                pose_tool::item_pose(tool.hand_pose, grip, contact).position,
            );
        }
        if let Some((eye, _)) = head {
            tool.eye = eye;
        }
        tool.floor_y = floor_y;
        tool.muzzle = crate::weapon_muzzle::known(world, tool.entity).map(|muzzle| match frame {
            Some(frame) => {
                use cgmath::Transform;
                crate::weapon_muzzle::MuzzleFallback {
                    point: frame.transform_point(muzzle.point),
                    axis: frame.transform_vector(muzzle.axis),
                }
            }
            None => muzzle,
        });
        // Test hold: the posing controller holds the item, posed as
        // `VirtualHand` poses it in play. An untracked one (zero rotation, or
        // the runtime says so) places nothing.
        let test_hold = tool.test_hold.is_some();
        if let Some(hold) = tool.test_hold.as_mut() {
            let hand = [&input.left_hand, &input.right_hand][posing];
            let pose = GripPose {
                position: hand_world_position(player_pos, player_rotation, hand.position),
                rotation: player_rotation * hand.rotation,
            };
            hold.follow(
                (pose.is_tracked() && tracking.is_none_or(|t| t.hands[posing])).then_some(pose),
                hand.trigger_value,
            );
        }
        // The input in world space, for the panel. Only a controller in hand
        // aims a ray (`pose_tool::controller_in_hand`); any other gets the
        // zero rotation, which aims none. A test-holding controller aims the
        // item instead.
        let mut pointing = input.clone();
        for (index, hand) in [&mut pointing.left_hand, &mut pointing.right_hand]
            .into_iter()
            .enumerate()
        {
            let in_hand = pose_tool::controller_in_hand(
                index == posing,
                hand.joints.is_some(),
                tracking.is_none_or(|t| t.hands[index]),
            ) && !(test_hold && index == posing);
            hand.joints = hand
                .joints
                .map(|joints| joints.map(|p| hand_world_position(player_pos, player_rotation, p)));
            hand.position = hand_world_position(player_pos, player_rotation, hand.position);
            hand.rotation = crate::util::tracked_rotation(hand.rotation)
                .filter(|_| in_hand)
                .map_or(Quaternion::new(0.0, 0.0, 0.0, 0.0), |rotation| {
                    player_rotation * rotation
                });
        }
        let (event, snapshot) = tool.point(&pointing);
        // The other hand's controller (held while the posing hand is
        // tracked) drives the tool; its trigger snapshots unless the press
        // began on the panel, and its squeeze turns the bench. Only while it
        // is in hand (it aims a ray): its other controls disarm, like the
        // trigger, until let go in hand.
        let free = [Handedness::Left, Handedness::Right][1 - posing];
        let control = [&input.left_hand, &input.right_hand][1 - posing];
        let in_hand = crate::util::tracked_rotation(
            [&pointing.left_hand, &pointing.right_hand][1 - posing].rotation,
        );
        // Test hold leaves the bench: no turning or scaling it (disarmed, as
        // out of hand, until let go), and no posing-hand glove but the one
        // holding the item.
        tool.turn_with(
            in_hand.filter(|_| !test_hold),
            control.squeeze_value > crate::ui::VR_TRIGGER_THRESHOLD,
        );
        // The posing hand into the unturned bench, which the tool works in.
        // The free hand pinches to snapshot only with no controller in it:
        // a hand round its controller can read as a pinch.
        let mut tracked = [pointing.left_hand.joints, pointing.right_hand.joints];
        tracked[posing] = tracked[posing]
            .filter(|_| !test_hold)
            .map(|joints| joints.map(|p| tool.to_bench(p)));
        tracked[1 - posing] = tracked[1 - posing].filter(|_| in_hand.is_none());
        tool.observe(tracked, snapshot, dt);
        // A support pose leaves the item's scale to its grip.
        let stick = tool.scale_input(
            control.thumbstick.y,
            in_hand.is_some() && !tool.support && !test_hold,
        );
        let scaled = pose_tool::GripPoseTool::scale(grip.item_scale, stick, dt);
        // The controllers in hand, which `pointing` gave a rotation.
        tool.controllers = std::array::from_fn(|i| {
            let hand = [&pointing.left_hand, &pointing.right_hand][i];
            let pose = GripPose {
                position: hand.position,
                rotation: hand.rotation,
            };
            pose.is_tracked()
                .then_some((pose, hand.trigger_value, hand.squeeze_value))
        });
        // The mapping's error for the live hand and for a queued capture, and
        // the open glove to match that capture against.
        let handedness = [Handedness::Left, Handedness::Right][posing];
        let capture = tool.capture.take();
        let mut glove_slot = self.glove_renderer.borrow_mut();
        let capture = match glove_slot.as_mut().and_then(|renderer| renderer.as_mut()) {
            Some(renderer) => {
                let live = tool.tracked[posing]
                    .and_then(|joints| pose_tool::tracked_glove(renderer, &joints, handedness));
                tool.tip_error_mm = live.as_ref().map(|glove| glove.tip_error_mm());
                tool.hand_size_ratio = live.as_ref().map(|glove| glove.fit.size_ratio);
                capture.map(|joints| {
                    let fitted = pose_tool::tracked_glove(renderer, &joints, handedness);
                    (joints, fitted)
                })
            }
            None => None,
        };
        drop(glove_slot);
        if let Some((joints, fitted)) = capture {
            let staged = if self.pose_tool.as_ref().is_some_and(|tool| tool.support) {
                self.capture_support(fitted.as_ref().map(|glove| {
                    let fit = &glove.fit;
                    let pose = GripPose {
                        position: fit.position,
                        rotation: fit.rotation,
                    };
                    (pose, fit.bones)
                }))
            } else {
                self.capture_grip(world, fitted.as_ref())
            };
            if staged {
                let fitted = fitted.map(|glove| glove.fit.joints);
                self.log_capture_joints(&joints, fitted.as_ref(), player_pos, player_rotation);
            }
        }
        self.scale_pose_tool_item(scaled);
        self.place_pose_tool_item(contact);
        let Some(event) = event else {
            return;
        };
        self.pose_tool_pulses
            .push((free, crate::haptics::GRIP_POSE_CLICK));
        println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL panel={event:?}");
        match event {
            pose_tool::panel::PanelEvent::Save => self.save_staged(),
            pose_tool::panel::PanelEvent::Discard => self.discard_staged(),
            pose_tool::panel::PanelEvent::TestHold => self.toggle_test_hold(),
            pose_tool::panel::PanelEvent::AlignAim => self.align_aim(contact),
            pose_tool::panel::PanelEvent::SwitchHand => self.switch_pose_hand(contact),
            pose_tool::panel::PanelEvent::SupportMode => self.toggle_support_mode(),
            pose_tool::panel::PanelEvent::Slot => {
                if let Some(tool) = self.pose_tool.as_mut()
                    && tool.toggle_slot()
                {
                    println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL slot={}", tool.slot.name());
                }
            }
            pose_tool::panel::PanelEvent::ResetView => {
                if let Some(tool) = self.pose_tool.as_mut() {
                    tool.reset_view();
                }
            }
            pose_tool::panel::PanelEvent::Exit => self.exit_pose_tool(),
        }
        // Drawn as the event left it (test hold ended, aim aligned), not a
        // frame late.
        self.place_pose_tool_item(contact);
    }

    /// Draw the posed item at `scale`. Resizing is an edit like a capture:
    /// with nothing staged it stages the current grip, so Save, Discard and
    /// the exit auto-save treat it alike.
    fn scale_pose_tool_item(&mut self, scale: f32) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let Some(held) = self.fitted_grips[tool.hand].as_ref() else {
            return;
        };
        let Some(current) = held.resolved.as_ref() else {
            return;
        };
        if current.item_scale == scale {
            return;
        }
        if tool.staged.is_none() {
            println!(
                "SHOCK2QUEST_VR_GRIP_POSE_TOOL staged=true reason=scale model={} hand={}",
                held.model,
                ["left", "right"][tool.hand]
            );
            let grip = current
                .clone()
                .rescaled(1.0 / crate::world_scale::WorldScale::active().personal());
            self.stage_grip(grip);
        }
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        tool.saved = false;
        if let Some(grip) = self.fitted_grips[tool.hand]
            .as_mut()
            .and_then(|held| held.resolved.as_mut())
        {
            grip.item_scale = scale;
        }
    }

    /// The hand the grip pose tool would pose, and its fitted item. The right
    /// hand first: it is the usual gun hand. Overrides are keyed like the
    /// libraries, which a magazine's fit key is not.
    fn poseable_held_item(&self) -> Option<(usize, &HeldGrip)> {
        [1, 0].into_iter().find_map(|i| {
            let held = self.fitted_grips[i]
                .as_ref()
                .filter(|held| held.resolved.is_some() && !held.model.starts_with("magazine:"))?;
            Some((i, held))
        })
    }

    /// Pose the other hand: its grip for the same item, from the library,
    /// holding the item where it is on the bench.
    fn switch_pose_hand(&mut self, contact: Vector3<f32>) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let held = self.fitted_grips[tool.hand].as_ref();
        let (Ok(grip), Some(held), Some(item)) = (
            pose_hand_switch(tool, held, self.grip_library.as_ref())
                .map(crate::vr_grip::ResolvedGrip::for_world_scale),
            held.cloned(),
            tool.item_placed,
        ) else {
            return;
        };
        let hand_pose = pose_tool::hand_for_item(item, &grip, contact);
        let other = 1 - tool.hand;
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL posing={} model={}",
            ["left", "right"][other],
            held.model
        );
        tool.retarget(other, hand_pose);
        // The other hand holds nothing of this in the mission; the tool's
        // copy is dropped as the workbench closes and the mission refits.
        self.fitted_grips[other] = Some(HeldGrip {
            resolved: Some(grip),
            source: "prepared",
            ..held
        });
    }

    /// Leave the workbench, saving a staged capture first. One that fails to
    /// save keeps the workbench open, with the error, to retry or discard.
    fn exit_pose_tool(&mut self) {
        if self
            .pose_tool
            .as_ref()
            .is_some_and(|tool| tool.staged.is_some())
        {
            self.save_staged();
            if self
                .pose_tool
                .as_ref()
                .is_some_and(|tool| tool.last_save_error.is_some())
            {
                println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL active=true reason=save_failed");
                return;
            }
        }
        self.close_pose_tool();
        println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL active=false");
    }

    /// Where the posed item is drawn, for the mission and the extra views:
    /// on the bench, or test-held by its controller (placed as play places
    /// a held item), with a gun's aim lines.
    fn place_pose_tool_item(&mut self, contact: Vector3<f32>) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let Some(held) = self.fitted_grips[tool.hand].as_ref() else {
            return;
        };
        if let Some(grip) = held.resolved.as_ref() {
            let hand = tool.holding();
            let item = pose_tool::item_pose(hand, grip, contact);
            tool.item_placed = Some(item);
            // The views turn about the model origin, which a contact offset
            // does not move.
            let model = pose_tool::item_pose(hand, grip, Vector3::new(0.0, 0.0, 0.0));
            tool.item = Some((model, grip.item_scale));
            tool.aim = tool
                .test_hold
                .filter(|hold| hold.tracked)
                .zip(tool.muzzle)
                .map(|(_, muzzle)| pose_tool::AimLines::new(model, grip.item_scale, muzzle, hand));
        }
    }

    /// Start or end test hold, from the free controller's lower button or
    /// the panel.
    fn toggle_test_hold(&mut self) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        if tool.toggle_test_hold() {
            println!(
                "SHOCK2QUEST_VR_GRIP_POSE_TOOL test_hold={}",
                tool.test_hold.is_some()
            );
        }
    }

    /// Turn the test-held gun's grip so its barrel fires down the controller
    /// (`pose_tool::aimed`), keeping its roll, hand placement and fingers;
    /// staged like any edit. The bench item stays put: its glove turns.
    fn align_aim(&mut self, contact: Vector3<f32>) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let (Ok(()), Some(muzzle)) = (tool.align_entry(), tool.muzzle) else {
            return;
        };
        let Some(held) = self.fitted_grips[tool.hand].as_ref() else {
            return;
        };
        let Some(current) = held.resolved.clone() else {
            return;
        };
        let bench = pose_tool::item_pose(tool.hand_pose, &current, contact);
        let grip = pose_tool::aimed(current, muzzle.axis, pose_tool::CONTROLLER_AIM);
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL aligned=true staged=true model={} hand={}",
            held.model,
            ["left", "right"][tool.hand]
        );
        let personal = crate::world_scale::WorldScale::active().personal();
        let Some(entry) = self.stage_grip(grip.rescaled(1.0 / personal)) else {
            return;
        };
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let grip = entry.grip.for_world_scale();
        tool.hand_pose = pose_tool::hand_for_item(bench, &grip, contact);
        if let Some(held) = self.fitted_grips[tool.hand].as_mut() {
            held.resolved = Some(grip);
            held.source = "prepared";
            held.authored = true;
        }
    }

    /// Leave the workbench, throwing away an unsaved capture or scale. The
    /// frozen hands resume "already pressed": a held trigger or grip neither
    /// fires, grabs nor (for the item still held) releases until let go, and
    /// the jump from where they froze is no throw. A held item does get a
    /// trigger release, as if the trigger were let go on the workbench.
    fn close_pose_tool(&mut self) {
        self.discard_staged();
        if self.pose_tool.take().is_none() {
            return;
        }
        // The mission refits from the library as it resumes: the saved grip
        // (unsaved scale changes are discarded too), with its collider sized.
        // Both hands: a save can also change the other hand's mirrored grip.
        self.fitted_grips = [None, None];
        for hand in [&mut self.left_hand, &mut self.right_hand] {
            *hand = if hand.get_held_entity().is_some() {
                hand.suspend_maintenance()
            } else {
                hand.spent_until_release()
            };
        }
    }

    /// Stage the queued tracked-hand capture as the held item's grip: apply it
    /// now as a preview, unsaved, and re-seat the held hand so the item does
    /// not move. Whether it staged.
    fn capture_grip(&mut self, world: &World, fitted: Option<&pose_tool::TrackedGlove>) -> bool {
        let Some(tool) = self.pose_tool.as_mut() else {
            return false;
        };
        let Some(held) = self.fitted_grips[tool.hand].as_ref() else {
            return false;
        };
        let Some(current) = held.resolved.as_ref() else {
            return false;
        };
        let Some(fitted) = fitted else {
            println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL captured=false reason=degenerate_joints");
            tool.capture_failure = Some("degenerate_joints".to_owned());
            return false;
        };
        let personal = crate::world_scale::WorldScale::active().personal();
        let fingers = pose_tool::slot_fingers(tool.slot, &current.fingers, fitted.fit.bones);
        // How far a rest capture's own aim was turned away (its fingers were
        // shaped for that aim), logged so a capture swung far off shows.
        let mut discarded_deg = None;
        let (grip, hand_pose) = match tool.slot {
            // Fingers only: the wrist and item stay where the rest grip has them.
            pose_tool::Slot::Trigger => (
                crate::vr_grip::ResolvedGrip {
                    fingers,
                    ..current.clone()
                }
                .rescaled(1.0 / personal),
                tool.hand_pose,
            ),
            pose_tool::Slot::Rest => {
                let contact = grip_contact(world, held.entity);
                let item = pose_tool::item_pose(tool.hand_pose, current, contact);
                let (grip, hand_pose) = pose_tool::captured_grip(
                    &fitted.fit,
                    item,
                    contact,
                    current.item_scale,
                    personal,
                );
                let grip = crate::vr_grip::ResolvedGrip { fingers, ..grip };
                // A gun keeps the current grip's aim: the mimed wrist is not the
                // controller-holding one, so only its roll about the barrel, where
                // the hand sits and the fingers are taken. The item stays put on the
                // bench; the glove turns to hold it so.
                match tool.muzzle {
                    Some(muzzle) => {
                        let barrel = pose_tool::firing_direction(current, muzzle.axis);
                        let swing = pose_tool::firing_direction(&grip, muzzle.axis).angle(barrel);
                        discarded_deg = Some(cgmath::Deg::from(swing).0);
                        let kept = pose_tool::aimed(grip, muzzle.axis, barrel);
                        let hand_pose = pose_tool::hand_for_item(
                            item,
                            &kept.clone().rescaled(personal),
                            contact,
                        );
                        (kept, hand_pose)
                    }
                    None => (grip, hand_pose),
                }
            }
        };
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL captured=true staged=true model={} hand={} slot={} aim_kept_swing_deg={}",
            held.model,
            ["left", "right"][tool.hand],
            tool.slot.name(),
            discarded_deg.map_or("none".to_owned(), |deg| format!("{deg:.1}"))
        );
        // The metric the mapping is tuned against: glove vs tracked
        // fingertips, and how much larger the real hand is than the glove.
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL tip_error_mm=[{}] hand_size_ratio={:.3}",
            fitted.tip_error_mm().map(|e| format!("{e:.1}")).join(","),
            fitted.fit.size_ratio
        );
        let Some(entry) = self.stage_grip(grip) else {
            return false;
        };
        let Some(tool) = self.pose_tool.as_mut() else {
            return false;
        };
        // The glove now sits where the tracked hand held the item. The grip
        // previews in place while the mission is frozen; it refits from the
        // library when the workbench closes.
        tool.hand_pose = hand_pose;
        if let Some(held) = self.fitted_grips[tool.hand].as_mut() {
            held.resolved = Some(entry.grip.for_world_scale());
            held.source = "prepared";
            held.authored = true;
        }
        true
    }

    /// Stage `grip` (authored units, like every library and overrides
    /// entry) as the posing hand's entry for its item, ahead of the library.
    /// It replaces a staged entry but keeps the grip from before the first,
    /// which is what discarding returns to.
    fn stage_grip(
        &mut self,
        grip: crate::vr_grip::ResolvedGrip,
    ) -> Option<crate::vr_grip::BakedGripEntry> {
        let tool = self.pose_tool.as_mut()?;
        let held = self.fitted_grips[tool.hand].as_ref()?;
        let entry = crate::vr_grip::BakedGripEntry {
            authored: true,
            model: held.model.clone(),
            hand: ["left", "right"][tool.hand].to_owned(),
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            grip,
        };
        let staged = match tool.staged.take() {
            Some(StagedGrip::Grip {
                entry: old,
                previous,
                hand_pose,
            }) => {
                if let Some(library) = &mut self.grip_library {
                    unstage_library_entry(library, &old);
                }
                StagedGrip::Grip {
                    entry: entry.clone(),
                    previous,
                    hand_pose,
                }
            }
            // Outside support mode only a grip is ever staged.
            _ => StagedGrip::Grip {
                entry: entry.clone(),
                previous: Some(held.clone()),
                hand_pose: tool.hand_pose,
            },
        };
        tool.staged = Some(staged);
        tool.last_save_error = None;
        tool.saved = false;
        if let Some(library) = &mut self.grip_library {
            library.prepend(crate::vr_grip::GripLibrary {
                entries: vec![entry.clone()],
                ..Default::default()
            });
        }
        Some(entry)
    }

    /// Log a staged capture's tracked joints and the glove fitted over them
    /// (`raw`/`fitted`, in the unturned bench) as pawn-local input, the
    /// form hand recordings keep, for mapping work without a recording.
    fn log_capture_joints(
        &self,
        raw: &crate::hand_capture::HandJoints,
        fitted: Option<&crate::hand_capture::HandJoints>,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
    ) {
        let (Some(tool), Some(fitted)) = (self.pose_tool.as_ref(), fitted) else {
            return;
        };
        let Some(held) = self.fitted_grips[tool.hand].as_ref() else {
            return;
        };
        let local = |joints: &crate::hand_capture::HandJoints| {
            joints.map(|p| {
                crate::virtual_hand::hand_local_position(
                    player_pos,
                    player_rotation,
                    tool.from_bench(p),
                )
            })
        };
        println!(
            "{}",
            crate::hand_capture::joints_log_line(
                [Handedness::Left, Handedness::Right][tool.posing()],
                &held.model,
                crate::world_scale::WorldScale::active().factor(),
                &local(raw),
                &local(fitted),
            )
        );
    }

    /// Write the staged capture (at its current item scale) to the overrides
    /// file. A failed save stays staged, so it can be retried.
    fn save_staged(&mut self) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let saved = match tool.staged.as_mut() {
            None => {
                println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL saved=false reason=nothing_staged");
                return;
            }
            Some(StagedGrip::Grip { entry, .. }) => {
                if let Some(grip) = self.fitted_grips[tool.hand]
                    .as_ref()
                    .and_then(|held| held.resolved.as_ref())
                {
                    // Authored, as `capture_grip` stores the entry.
                    entry.grip.item_scale =
                        grip.item_scale / crate::world_scale::WorldScale::active().personal();
                }
                pose_tool::save_override(&tool.overrides_path, entry).map(|()| {
                    // Keep the in-memory entry in step with the file (its scale).
                    if let Some(kept) = self.grip_library.as_mut().and_then(|library| {
                        library
                            .entries
                            .iter_mut()
                            .find(|e| e.model == entry.model && e.hand == entry.hand)
                    }) {
                        *kept = entry.clone();
                    }
                    format!("model={} hand={}", entry.model, entry.hand)
                })
            }
            Some(StagedGrip::Support { model, pose, .. }) => {
                pose_tool::save_support_override(&tool.overrides_path, model, pose)
                    .map(|()| format!("model={model} support=true"))
            }
        };
        match saved {
            Ok(saved) => println!(
                "SHOCK2QUEST_VR_GRIP_POSE_TOOL saved=true {saved} path={}",
                tool.overrides_path.display()
            ),
            Err(error) => {
                println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL saved=false error=\"{error}\"");
                tool.last_save_error = Some(error.to_string());
                return;
            }
        }
        tool.staged = None;
        tool.last_save_error = None;
        tool.capture_failure = None;
        tool.saved = true;
        let free = [Handedness::Left, Handedness::Right][1 - tool.posing()];
        self.pose_tool_pulses
            .push((free, crate::haptics::GRIP_POSE_SAVED));
    }

    /// Undo an unsaved capture: the grip, glove placement and library entry
    /// return to what they were before it.
    fn discard_staged(&mut self) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        let Some(staged) = tool.staged.take() else {
            return;
        };
        tool.last_save_error = None;
        tool.capture_failure = None;
        match staged {
            StagedGrip::Grip {
                entry,
                previous,
                hand_pose,
            } => {
                tool.hand_pose = hand_pose;
                self.fitted_grips[tool.hand] = previous;
                if let Some(library) = &mut self.grip_library {
                    unstage_library_entry(library, &entry);
                }
                println!(
                    "SHOCK2QUEST_VR_GRIP_POSE_TOOL discarded=true model={} hand={}",
                    entry.model, entry.hand
                );
            }
            StagedGrip::Support {
                model, previous, ..
            } => {
                println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL discarded=true model={model} support=true");
                self.support_profiles.insert(model, previous);
            }
        }
    }

    /// Stage the queued tracked-hand capture as the held item's support pose:
    /// the posing hand's glove relative to the grip hand's, frozen at its
    /// grip. `glove` is where the glove fitted over the tracked hand is, and
    /// its bones; `None` for a fit that failed. Previewed live, unsaved.
    /// Whether it staged.
    fn capture_support(
        &mut self,
        glove: Option<(GripPose, crate::hand_pose::FingerBones)>,
    ) -> bool {
        let Some(tool) = self.pose_tool.as_mut() else {
            return false;
        };
        let (Some(held), Some(rig)) = (
            self.fitted_grips[tool.hand].as_ref(),
            self.grip_kinematics.as_ref(),
        ) else {
            return false;
        };
        let (Some(grip), Some(mirror)) = (held.resolved.as_ref(), held.model_mirror) else {
            return false;
        };
        // A failed fit, or a pose its profile rejects as invalid.
        let posed = glove.ok_or("degenerate_joints").and_then(|(glove, bones)| {
            let pose = SupportProfile::captured_pose(
                [Handedness::Left, Handedness::Right][tool.hand],
                pose_tool::item_pose(tool.hand_pose, grip, Vector3::new(0.0, 0.0, 0.0)),
                grip,
                &rig[tool.posing()],
                mirror,
                glove,
                crate::vr_grip::FingerPose::Bones {
                    bones,
                    trigger_bones: None,
                },
            )
            .ok_or("degenerate_joints")?;
            self.support_profiles
                .get(&held.model)
                .is_some_and(|profile| profile.with_pose(&pose).is_valid())
                .then_some(pose)
                .ok_or("invalid_support_pose")
        });
        let pose = match posed {
            Ok(pose) => pose,
            Err(reason) => {
                println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL captured=false reason={reason}");
                tool.capture_failure = Some(reason.to_owned());
                return false;
            }
        };
        let model = held.model.clone();
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL captured=true staged=true model={model} support=true palm_anchor={:?} rotation_degrees={:?}",
            pose.palm_anchor, pose.rotation_degrees
        );
        // Discarding returns to the profile from before the first capture.
        let previous = match tool.staged.take() {
            Some(StagedGrip::Support { previous, .. }) => previous,
            _ => self.support_profiles[&model].clone(),
        };
        let posed = previous.with_pose(&pose);
        self.support_profiles.insert(model.clone(), posed);
        tool.staged = Some(StagedGrip::Support {
            model,
            pose,
            previous,
        });
        tool.last_save_error = None;
        tool.saved = false;
        true
    }

    /// Pose the support grip, or the primary again. Not while a capture is
    /// staged, nor for an item with no support grip (`support_mode`).
    fn toggle_support_mode(&mut self) {
        let Some(tool) = self.pose_tool.as_mut() else {
            return;
        };
        // `support_mode` is from the frame's start: a capture staged since
        // (the same frame) must be saved or discarded first too.
        if tool.support_mode.is_err() || tool.staged.is_some() {
            return;
        }
        tool.set_support(!tool.support);
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL mode={} posing={}",
            if tool.support { "support" } else { "primary" },
            ["left", "right"][tool.posing()]
        );
    }

    /// The support glove the workbench draws in support mode: the item's
    /// support profile (a staged capture included) placed against the grip
    /// hand's glove, as play places it.
    fn pose_tool_support_glove(
        &self,
        tool: &pose_tool::GripPoseTool,
    ) -> Option<(GripPose, crate::hand_pose::HandShape)> {
        let held = self.fitted_grips[tool.hand].as_ref()?;
        let grip = held.resolved.as_ref()?;
        let profile = self.support_profiles.get(&held.model)?;
        let rig = self.grip_kinematics.as_ref()?;
        let primary = [Handedness::Left, Handedness::Right][tool.hand];
        let pose = profile.glove_pose(
            primary,
            pose_tool::item_pose(tool.hand_pose, grip, Vector3::new(0.0, 0.0, 0.0)),
            grip,
            &rig[tool.posing()],
            profile.anchor_in_frame(primary, held.model_mirror?) * grip.item_scale,
        );
        Some((pose, profile.fingers.hand_shape_at(0.0)))
    }
}

/// Whether the pose tool can switch between the primary and the support
/// grip, or why not: only for an item with a support profile, and not with
/// a capture staged.
fn support_mode(
    tool: &pose_tool::GripPoseTool,
    held: Option<&HeldGrip>,
    profiles: &HashMap<String, SupportProfile>,
) -> Result<(), String> {
    if !held.is_some_and(|held| {
        held.model_mirror.is_some()
            && crate::vr_support::supports_model(&held.model)
            && profiles.contains_key(&held.model)
    }) {
        return Err("one-handed".to_owned());
    }
    if tool.test_hold.is_some() {
        return Err("end test hold first".to_owned());
    }
    if tool.staged.is_some() {
        return Err("save or discard first".to_owned());
    }
    Ok(())
}

/// The library grip (authored size) the pose tool's other hand would start
/// from for the same item, mirrored as play would hold it, or why it cannot
/// switch to it.
fn pose_hand_switch(
    tool: &pose_tool::GripPoseTool,
    held: Option<&HeldGrip>,
    library: Option<&crate::vr_grip::GripLibrary>,
) -> Result<crate::vr_grip::ResolvedGrip, String> {
    // The bench can only show a glove weapon to the other hand mirrored.
    if held.is_some_and(|held| {
        crate::vr_weapon_grip::supports_model(&held.model) && held.model_mirror.is_none()
    }) {
        return Err("hold it in that hand".to_owned());
    }
    if tool.support {
        return Err("posing the support grip".to_owned());
    }
    if tool.test_hold.is_some() {
        return Err("end test hold first".to_owned());
    }
    if tool.staged.is_some() {
        return Err("save or discard first".to_owned());
    }
    let other = 1 - tool.hand;
    held.zip(library)
        .and_then(|(held, library)| {
            let hand = [Handedness::Left, Handedness::Right][other];
            library.lookup_for(&held.model, hand, held.model_mirror)
        })
        .map(|(grip, _)| grip)
        .ok_or_else(|| format!("no {} grip to start from", ["left", "right"][other]))
}

/// The posed hand's item frame from its holder's, when the grip hand is not
/// the holder and the item is drawn mirrored between hands: the bench and its
/// muzzle show the item as that hand would hold it.
fn pose_frame(tool: &pose_tool::GripPoseTool, held: Option<&HeldGrip>) -> Option<Matrix4<f32>> {
    if tool.hand == tool.holder {
        return None;
    }
    let hand = [Handedness::Left, Handedness::Right][tool.hand];
    crate::vr_grip::hand_model_mirror(held?.model_mirror?, hand)
}

/// Remove a staged entry from the library: always the first for its model
/// and hand, since staging prepends it.
fn unstage_library_entry(
    library: &mut crate::vr_grip::GripLibrary,
    entry: &crate::vr_grip::BakedGripEntry,
) {
    library.remove_first(&entry.model, &entry.hand);
}

impl PlayerInteraction for VrInteraction {
    fn body_wrist_frames(&self) -> [Option<Matrix4<f32>>; 2] {
        let mut glove = self.glove_renderer.borrow_mut();
        let Some(Some(renderer)) = glove.as_mut() else {
            return [None; 2];
        };
        let tracked = self.hand_poses();
        std::array::from_fn(|i| {
            let pose = self.visual_hands[i].unwrap_or(tracked[i]);
            (self.body_hand_tracking[i] && pose.is_tracked()).then(|| {
                Matrix4::from_translation(pose.position)
                    * Matrix4::from(pose.rotation)
                    * renderer.wrist_frame(if i == 0 {
                        Handedness::Left
                    } else {
                        Handedness::Right
                    })
            })
        })
    }

    fn implant_socket_frames(&self, world: &World) -> [Option<Matrix4<f32>>; 2] {
        use crate::hud::virtual_arms::{
            BIO_BAND_RADIUS, glove_band_frame, glove_palm_lift, implant_socket_frame,
        };
        let mut glove = self.glove_renderer.borrow_mut();
        let Some(Some(renderer)) = glove.as_mut() else {
            return [None; 2];
        };
        let tracked = self.hand_poses();
        std::array::from_fn(|i| {
            let pose = self.visual_hands[i].unwrap_or(tracked[i]);
            if !pose.is_tracked() {
                return None;
            }
            let hand = if i == 0 {
                Handedness::Left
            } else {
                Handedness::Right
            };
            let held = crate::wielded_weapon::held_by_hand(world, hand);
            let (root, lift) = if crate::virtual_hand::shows_hand_visual(world, held) {
                (
                    glove_band_frame(
                        Matrix4::from_translation(pose.position)
                            * Matrix4::from(pose.rotation)
                            * renderer.wrist_frame(hand),
                    ),
                    glove_palm_lift(),
                )
            } else {
                let (wrist, radius) = held
                    .filter(|&amp| crate::wielded_weapon::is_psi_amp(world, amp))
                    .and_then(|amp| crate::psi_amp_readout::wrist(world, amp, hand))?;
                (wrist * Matrix4::from_scale(radius / BIO_BAND_RADIUS), 0.0)
            };
            Some(implant_socket_frame(root, hand, lift))
        })
    }

    fn body_palm_positions(&self, input: &InputContext) -> [Option<Vector3<f32>>; 2] {
        let Some(rig) = &self.grip_kinematics else {
            return [None; 2];
        };
        let hands = [&input.left_hand, &input.right_hand];
        std::array::from_fn(|i| {
            let pose = GripPose {
                position: hands[i].position,
                rotation: hands[i].rotation,
            };
            (pose.is_tracked() && input.pose_tracking.is_none_or(|p| p.head && p.hands[i]))
                .then(|| pose.point(rig[i].palm_in_world()))
        })
    }

    fn set_body_tool_device(&mut self, device: bool) {
        self.body_tool_device = device;
    }

    fn personal_card_grip(&self, hand: usize) -> Option<crate::vr_grip::ResolvedGrip> {
        self.grip_library
            .as_ref()?
            .lookup(
                if self.body_tool_device {
                    "tricorder"
                } else {
                    "scipass"
                },
                if hand == 0 { "left" } else { "right" },
            )
            .cloned()
            .map(crate::vr_grip::ResolvedGrip::for_world_scale)
    }

    fn reserve_body_tool_hands(&mut self, reserved: [bool; 2]) {
        self.body_tool_hands = reserved;
    }

    fn hand_available_for_body_slot(&self, hand: Handedness) -> bool {
        let i = crate::vr_config::hand_slot(hand);
        let held = if i == 0 {
            self.left_hand.get_held_entity()
        } else {
            self.right_hand.get_held_entity()
        };
        held.is_none()
            && !self.body_tool_hands[i]
            && !self.support_blocked[i]
            && !self.hand_climb.grips().any(|(h, _)| h == hand)
    }
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
                    .filter(|library| library.version == 1)
                    .unwrap_or_default(),
            );
            if let Some(weapons) = assets
                .get_opt(&TEXT_IMPORTER, "vr-weapon-grips.json")
                .and_then(|text| serde_json::from_str::<crate::vr_grip::GripLibrary>(&text).ok())
                .filter(|lib| lib.version == 1)
            {
                // Explicit pickup-library edits take precedence over the shipped weapon defaults.
                let library = self.grip_library.as_mut().unwrap();
                // The first valid model/hand entry wins; explicit pickup-library
                // overrides remain ahead of shipped weapon defaults.
                library.entries.extend(weapons.entries);
            }
            let tricorder = assets
                .get_opt(&TEXT_IMPORTER, "vr-tricorder-grips.json")
                .and_then(|text| serde_json::from_str::<crate::vr_grip::GripLibrary>(&text).ok())
                .filter(|lib| lib.version == 1)
                .unwrap_or_else(|| {
                    serde_json::from_str(include_str!("../../assets/vr-tricorder-grips.json"))
                        .expect("shipped tricorder grips")
                });
            self.grip_library
                .as_mut()
                .unwrap()
                .entries
                .extend(tricorder.entries);
            if let Some(magazines) = assets
                .get_opt(&TEXT_IMPORTER, "vr-magazine-grips.json")
                .and_then(|text| serde_json::from_str::<crate::vr_grip::GripLibrary>(&text).ok())
                .filter(|lib| lib.version == 1)
            {
                self.grip_library
                    .as_mut()
                    .unwrap()
                    .entries
                    .extend(magazines.entries);
            }
            // Read from the data root directly: a loose file there is shadowed
            // by the bundled assets when going through the asset cache.
            if let Some(overrides) = crate::vr_grip::GripOverrides::load(
                &crate::paths::data_root().join(crate::vr_grip::GRIP_OVERRIDES_FILE),
            ) {
                self.grip_library.as_mut().unwrap().prepend(overrides.grips);
                crate::vr_support::apply_overrides(&mut self.support_profiles, &overrides.support);
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
            let magazine = crate::vr_magazine::appearance(world, entity);
            let model = world
                .borrow::<View<PropModelName>>()
                .ok()
                .and_then(|v| v.get(entity).ok().map(|p| p.0.to_lowercase()));
            let model = magazine
                .as_ref()
                .map(|m| format!("magazine:{}:{}", m.source, m.item_scale.to_bits()))
                .or(model);
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
                let handedness = [Handedness::Left, Handedness::Right][index];
                // A glove weapon is drawn mirrored in the left hand; one hand's
                // grip, mirrored, holds it in the other.
                let model_mirror = if crate::vr_weapon_grip::supports_model(&model) {
                    crate::vr_weapon_grip::model_mirror(assets, &format!("{model}.bin"))
                } else {
                    None
                };
                let eligible = magazine.is_some()
                    || bake
                    || self
                        .grip_library
                        .as_ref()
                        .unwrap()
                        .select(&model, handedness, model_mirror.is_some())
                        .is_some();
                let geometry = if eligible {
                    self.grip_geometry
                        .entry(format!("{model}:{hand_name}"))
                        .or_insert_with(|| {
                            if let Some(magazine) = magazine.as_ref() {
                                let source = crate::vr_magazine::load(assets, &magazine.source)?;
                                let source = source.as_ref().as_ref()?;
                                return Some(GripGeometry {
                                    triangles: std::rc::Rc::new(source.triangles.clone()),
                                    fingerprint: crate::vr_grip::surface_fingerprint(
                                        &source.triangles,
                                    ),
                                    weapon_arms: None,
                                });
                            }
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
                let prepared_magazine = magazine.as_ref().and_then(|magazine| {
                    crate::vr_magazine::prepared_grip(
                        self.grip_library.as_ref().unwrap(),
                        &magazine.source,
                        hand_name,
                        magazine.item_scale,
                    )
                });
                let has_prepared_magazine = prepared_magazine.is_some();
                let mut mirrored = false;
                let resolved = if has_prepared_magazine {
                    prepared_magazine
                } else if let Some(magazine) = magazine.as_ref() {
                    self.magazine_grips
                        .entry(format!("{model}:{hand_name}"))
                        .or_insert_with(|| {
                            triangles.and_then(|triangles| {
                                crate::vr_magazine::resolve_grip(
                                    triangles,
                                    magazine.item_scale,
                                    &kinematics[index],
                                )
                            })
                        })
                        .clone()
                } else if bake && geometry.is_some_and(|g| g.weapon_arms.is_some()) {
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
                        .lookup_for(&model, handedness, model_mirror)
                        .map(|(grip, other)| {
                            mirrored = other;
                            grip
                        })
                };
                let source = if has_prepared_magazine {
                    "prepared"
                } else if bake || magazine.is_some() {
                    "bake"
                } else if mirrored {
                    "mirrored"
                } else if resolved.is_some() {
                    "prepared"
                } else {
                    "missing_or_invalid"
                };
                // The Quest runtime has no tracing subscriber; use its structured
                // stdout marker convention for on-device verification.
                let solve_ms = start.elapsed().as_secs_f64() * 1000.0;
                println!(
                    "SHOCK2QUEST_VR_GRIP model={model} hand={hand_name} source={source} elapsed_ms={solve_ms:.3}"
                );
                if resolved.is_none() {
                    let matching = self
                        .grip_library
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .filter(|e| e.model == model && e.hand == hand_name)
                        .count();
                    println!(
                        "SHOCK2QUEST_VR_GRIP_REJECT model={model} hand={hand_name} matching_entries={matching} reason=missing_or_invalid_pose"
                    );
                }
                let authored = !bake
                    && resolved.is_some()
                    && self.grip_library.as_ref().unwrap().entries.iter().any(|e| {
                        let mut expected = e.grip.clone();
                        let key = if let Some(magazine) = magazine.as_ref() {
                            expected.item_scale = magazine.item_scale;
                            crate::vr_magazine::grip_model(&magazine.source).unwrap_or(&model)
                        } else {
                            &model
                        };
                        e.model == key
                            && e.hand == hand_name
                            && e.authored
                            && resolved.as_ref() == Some(&expected)
                    });
                let resolved = resolved.map(crate::vr_grip::ResolvedGrip::for_world_scale);
                if crate::mission::mission_core::held_item_collision_group(world, entity).is_some()
                    || crate::pipe_melee::is_ranged_guard(world, entity)
                {
                    if let (Some(grip), Some(geometry), Some(contact)) = (
                        resolved.as_ref(),
                        geometry,
                        Some(grip_contact(world, entity)),
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
                        effects.push(VirtualHandEffect::FitHeldItem {
                            entity_id: entity,
                            size: max - min,
                            center: (min + max) * 0.5,
                            scale: grip.item_scale,
                        });
                    }
                }
                let item_bounds = resolved
                    .as_ref()
                    .zip(geometry)
                    .and_then(|(grip, geometry)| grip.item_bounds(&geometry.triangles));
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
            let hand_pose = GripPose {
                position: hand.get_position(),
                rotation: hand.get_rotation(),
            };
            // An untracked hand (zero quaternion) places nothing: the item
            // stays where it is.
            if let Some(grip) = self.fitted_grips[index]
                .as_ref()
                .and_then(|g| g.resolved.as_ref())
                .filter(|_| hand_pose.is_tracked())
            {
                // The acquisition frame may only contain HoldItem. Append its
                // initial placement too, in the same effect pipeline as movement.
                effects.retain(|effect| !matches!(effect, VirtualHandEffect::SetPositionRotation {entity_id,..} if *entity_id == entity));
                let item = pose_tool::item_pose(hand_pose, grip, grip_contact(world, entity));
                effects.push(VirtualHandEffect::SetPositionRotation {
                    entity_id: entity,
                    position: item.position,
                    rotation: item.rotation,
                    scale: cgmath::vec3(grip.item_scale, grip.item_scale, grip.item_scale),
                });
            }
        }
        drop(slot);
        self.apply_support(world, effects);
        let candidate = self.support_candidate(world, self.hand_poses(), true);
        for primary in 0..2 {
            if let Some((motion, _)) = self.pump_motion(world, primary) {
                let entity = self.fitted_grips[primary].as_ref().unwrap().entity;
                let fraction = candidate
                    .as_ref()
                    .filter(|c| c.entity == entity)
                    .and_then(|c| c.pump.map(|(_, _, fraction)| fraction))
                    .unwrap_or_else(|| crate::vr_shotgun_pump::state(world, entity).fraction);
                effects.push(VirtualHandEffect::MovePump {
                    entity_id: entity,
                    motion,
                    fraction,
                });
            }
        }
    }

    fn synchronize_held_visuals(&mut self, world: &World) {
        let poses = self.hand_poses();
        self.support_preview = self.support_candidate(world, poses, true);
        self.visual_hands = [None, None];
        for i in 0..2 {
            if !self.body_hand_tracking[i]
                || !poses[i].is_tracked()
                || self.body_tool_hands[i]
                || [&self.left_hand, &self.right_hand][i]
                    .get_held_entity()
                    .is_some()
            {
                self.climb_visuals[i] = Default::default();
                continue;
            }
            let target = self.climb_members[i].and_then(|held| {
                climb_visual::attached_pose(
                    &self.grip_kinematics.as_ref()?[i],
                    [Handedness::Left, Handedness::Right][i],
                    held.grip,
                    held.member,
                    held.wrist_rotation,
                )
            });
            self.visual_hands[i] = self.climb_visuals[i].update(poses[i], target, self.step_dt);
        }
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
        self.synchronize_slide(world);
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

    fn synchronize_held_articulation(&mut self, world: &World) {
        self.synchronize_slide(world);
    }

    fn set_ui_point(&mut self, hands: [bool; 2]) {
        self.ui_point = hands;
    }

    fn set_body_anticipation(&mut self, targets: [Option<EntityId>; 2]) {
        self.body_anticipation = targets;
    }

    fn maintenance_previews(&self, world: &World) -> Vec<String> {
        [&self.left_hand, &self.right_hand]
            .into_iter()
            .filter_map(|hand| hand.maintenance_preview(world))
            .collect()
    }

    fn release_offers(&self) -> [Option<(EntityId, EntityId)>; 2] {
        let hands = [&self.left_hand, &self.right_hand];
        std::array::from_fn(|i| {
            // A squeezing empty hand within reach catches the item instead
            // (`try_handoff`), so nothing is offered to the target.
            let other = hands[1 - i];
            let handoff = other.get_held_entity().is_none()
                && other.squeeze_value() > 0.5
                && (hands[i].get_position() - other.get_position()).magnitude() <= HANDOFF_REACH;
            hands[i].release_offer().filter(|_| !handoff)
        })
    }

    fn hand_feedback_diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"left": self.left_hand.feedback_diagnostics(), "right": self.right_hand.feedback_diagnostics(), "anticipation": self.anticipation})
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
                    "tracked_palm": self.grip_kinematics.as_ref().map(|rig| self.hand_poses()[1-i].point(rig[1-i].palm_in_world())),
                    "pressed": self.support_pressed, "blocked": self.support_blocked, "step_dt": self.step_dt,
                    "socket_position": c.model_pose.point(c.anchor),
                    // The support solver returns a calibrated hand origin. Tools
                    // placing raw tracked input need the inverse calibration.
                    "controller_position": c.hand_pose.position - crate::glove_fit::forward_translation(
                        c.hand_pose.rotation, crate::dev_params::get(crate::dev_params::GLOVE_FORWARD_CM)),
                    "controller_rotation": c.hand_pose.rotation,
                    "model_position": c.model_pose.position, "model_rotation": c.model_pose.rotation,
                    "primary_palm": c.primary_palm, "primary_anchor": c.primary_anchor,
                    "visible_primary_palm": c.visible_primary_palm,
                    "control_primary_palm": c.control_primary_palm,
                    "pump": c.pump.map(|(motion, travel, fraction)| serde_json::json!({"parameter": motion.parameter, "value": motion.value(fraction), "fraction": fraction, "travel": travel, "world_travel": c.model_pose.rotation.rotate_vector(travel)})),
                    "support_anchor": c.anchor, "grab_radius": c.profile.grab_radius,
                    "region_endpoints": c.region.map(|ends| ends.map(|p| c.model_pose.point(p))),
                    "visual_trigger": visual_triggers[1-i],
                    "finger_curls": c.profile.fingers.curls_at(visual_triggers[1-i]),
                    // The captured part of the profile in use (bones: posed in VR).
                    "palm_anchor": c.profile.palm_anchor, "rotation_degrees": c.profile.rotation_degrees,
                    "finger_pose": if c.profile.fingers.rest_curls().is_some() { "curls" } else { "bones" },
                    "release_distance": c.profile.release_distance, "max_swing_degrees": c.profile.max_swing_degrees
                })
            });
            Some(serde_json::json!({"slide": self.slide_diagnostics(grip.entity), "glove_pose": self.visual_hands[i], "support": support, "hand": if i == 0 {"left"} else {"right"}, "entity_id": grip.entity.inner() as i32,
                "visual_trigger": visual_triggers[i], "finger_curls": grip.resolved.as_ref().and_then(|g| g.fingers.curls_at(visual_triggers[i])),
                // The bone pose drawn at that trigger (bone grips; null for curls).
                "finger_bones": grip.resolved.as_ref().and_then(|g| match g.fingers.at(visual_triggers[i]) { crate::vr_grip::FingerPose::Bones { bones, .. } => Some(bones), crate::vr_grip::FingerPose::Curls { .. } => None }),
                "model": grip.model, "item_bounds": grip.item_bounds, "solve_ms": grip.solve_ms, "grip": grip.resolved,
                "surface_hash": grip.surface_hash, "kinematics_hash": grip.kinematics_hash, "hints_hash": grip.hints_hash, "solver_revision": crate::vr_grip::SOLVER_REVISION, "source": grip.source, "authored": grip.authored,
                "palm": self.grip_kinematics.as_ref().map(|k| k[i].palm_in_world()), "palm_normal": self.grip_kinematics.as_ref().map(|k| k[i].normal),
                "pose_tool": self.pose_tool.as_ref().filter(|tool| tool.hand == i).map(|tool| serde_json::json!({"mode": if tool.support { "support" } else { "primary" }, "posing": (["left", "right"][tool.posing()]), "slot": tool.slot.name(), "has_trigger_pose": grip.resolved.as_ref().is_some_and(|g| g.fingers.has_trigger_pose()), "staged": tool.staged.is_some(), "saved": tool.saved, "last_save_error": tool.last_save_error, "tip_error_mm": tool.tip_error_mm, "hand_size_ratio": tool.hand_size_ratio, "capture_failure": tool.capture_failure, "test_hold": tool.test_hold.map(|hold| serde_json::json!({"tracked": hold.tracked})), "aim_error_deg": tool.aim.map(|aim| aim.error_deg()), "panel": tool.panel_diagnostics()}))}))
        }).collect())
    }

    fn update_hand_climb(&mut self, ctx: &ClimbContext) -> crate::vr_climb::ClimbFrame {
        use shipyard::EntitiesView;

        let hand_input = |index: usize, hand: &VirtualHand, input: &crate::input_context::Hand| {
            crate::vr_climb::ClimbHandInput {
                local_position: input.position,
                squeeze: input.squeeze_value,
                // A hand that is carrying something cannot also hold a ladder.
                is_empty: hand.get_held_entity().is_none()
                    && !self.support_blocked[index]
                    && !self.body_tool_hands[index],
            }
        };
        let probe = |point: Vector3<f32>, rule: crate::vr_climb::GripProbe| match rule {
            crate::vr_climb::GripProbe::Ordinary => {
                ctx.physics
                    .climbable_grip_at(point, crate::physics::CLIMB_GRIP_RADIUS, ctx.feet_y)
            }
            crate::vr_climb::GripProbe::Transfer => ctx
                .physics
                .climbable_grip_during_transfer_at(point, ctx.feet_y),
            crate::vr_climb::GripProbe::Surface => ctx.physics.surface_grip_at(point, ctx.pawn_pos),
        };
        self.hand_climb.set_grounded(ctx.grounded);
        let frame = self.hand_climb.update(
            ctx.pawn_pos,
            ctx.pawn_rotation,
            ctx.step_dt,
            [
                hand_input(0, &self.left_hand, &ctx.input.left_hand),
                hand_input(1, &self.right_hand, &ctx.input.right_hand),
            ],
            probe,
            // "Cannot check" is not "gone" - a failed borrow keeps the hold.
            |entity_id| {
                ctx.world
                    .borrow::<EntitiesView>()
                    .map_or(true, |entities| entities.is_alive(entity_id))
            },
        );
        // After the update, so the glow answers with the holds the next grab
        // will be judged against - by the same rule acquisition uses.
        self.climb_anticipation = std::array::from_fn(|i| {
            let hand = [&self.left_hand, &self.right_hand][i];
            let input = [&ctx.input.left_hand, &ctx.input.right_hand][i];
            hand_input(i, hand, input).is_empty
                && ctx.input.pose_tracking.is_none_or(|p| p.head && p.hands[i])
                && (GripPose {
                    position: input.position,
                    rotation: input.rotation,
                })
                .is_tracked()
                && self
                    .hand_climb
                    .probe_grip(
                        crate::vr_climb::HANDS[i],
                        hand_world_position(ctx.pawn_pos, ctx.pawn_rotation, input.position),
                        probe,
                    )
                    .is_some()
        });
        let held: [bool; 2] =
            std::array::from_fn(|i| self.hand_climb.grips().any(|(h, _)| h as usize == i));
        for (seconds, held) in self.climb_held_seconds.iter_mut().zip(held) {
            *seconds = if held { *seconds + ctx.step_dt } else { 0.0 };
        }
        let previous_members = self.climb_members;
        self.climb_members = [None; 2];
        for (hand, anchor) in self.hand_climb.grips() {
            self.climb_members[hand as usize] =
                ctx.physics.ladder_member(&anchor.grip).map(|member| {
                    let mut grip = anchor.grip;
                    // A cap contact has a vertical normal. Choose its ladder
                    // face from the acquisition point, never the drifting hand.
                    if member.face_normal.dot(grip.normal).abs() <= 0.1 {
                        grip.normal = anchor.hand_world_at_grab - grip.point;
                    }
                    climb_visual::HeldMember::capture(
                        previous_members[hand as usize],
                        grip,
                        member,
                        ctx.pawn_rotation
                            * [&ctx.input.left_hand, &ctx.input.right_hand][hand as usize].rotation,
                    )
                });
        }
        frame
    }

    fn hand_climb(&self) -> Option<&crate::vr_climb::HandClimb> {
        Some(&self.hand_climb)
    }

    fn climb_reach(&self) -> [bool; 2] {
        self.climb_reach
    }

    fn resolve_hand_climb(&mut self, requested: Vector3<f32>, applied: Vector3<f32>) {
        self.hand_climb.resolve_translation(requested, applied);
    }

    fn release_climb_grips(&mut self) {
        self.hand_climb.release_all();
        self.climb_members = [None; 2];
    }

    fn suspend_hand_climb(&mut self, input: &InputContext) {
        self.hand_climb.track_squeezes_only([
            input.left_hand.squeeze_value,
            input.right_hand.squeeze_value,
        ]);
    }

    fn set_climb_release_safe(&mut self, safe: bool) {
        self.climb_release_safe = safe;
    }

    fn update(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect> {
        let scale = crate::world_scale::WorldScale::active();
        if scale != self.world_scale {
            // Reuse the authored fit data, rebuilding runtime offsets and contact
            // shapes at the new conversion without rebaking or saving grip edits.
            self.fitted_grips = [None, None];
            self.support = None;
            self.support_preview = None;
            self.slide = None;
            self.slide_preview = None;
            self.visual_hands = [None, None];
            self.left_hand.reset_throw_motion();
            self.right_hand.reset_throw_motion();
            self.world_scale = scale;
        }
        self.body_hand_tracking =
            std::array::from_fn(|i| ctx.input.pose_tracking.is_none_or(|p| p.head && p.hands[i]));
        let mut slide_effects = self.update_slide(ctx);
        self.update_support(ctx);
        if let Some(effects) = self.try_handoff(ctx) {
            return effects;
        }
        for (i, hand) in [&mut self.left_hand, &mut self.right_hand]
            .into_iter()
            .enumerate()
        {
            if ctx
                .input
                .pose_tracking
                .is_some_and(|p| !p.head || !p.hands[i])
            {
                hand.reset_throw_motion();
            }
        }
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
        let tracked =
            [(&ctx.input.left_hand, 0), (&ctx.input.right_hand, 1)].map(|(input, slot)| {
                self.body_hand_tracking[slot]
                    && (GripPose {
                        position: input.position,
                        rotation: input.rotation,
                    })
                    .is_tracked()
            });
        let maintenance_hand = [&self.left_hand, &self.right_hand].map(|hand| {
            hand.get_held_entity().is_some_and(|item| {
                crate::scripts::maintenance::is_maintenance_tool(ctx.world, item)
            })
        });
        let right_held_before = self.right_hand.get_held_entity();
        let (right_hand, mut right_msgs) = if maintenance_hand[1] && !tracked[1] {
            (self.right_hand.suspend_maintenance(), Vec::new())
        } else if self.support_blocked[1] && self.right_hand.get_held_entity().is_none() {
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
                left_held_entity.filter(|_| !maintenance_hand[1] || tracked[0]),
                left_position,
                ctx.step_dt,
            )
        };
        self.right_hand = right_hand;

        // Right updates first, so a same-frame right-hand grab is visible to
        // the left hand and one physical item cannot enter both hand states.
        // A release is still owned until its effects run (it may be a backpack
        // deposit), so the left hand must not acquire that same entity either.
        let right_held_entity = self.right_hand.get_held_entity().or(right_held_before);
        let (left_hand, mut left_msgs) = if maintenance_hand[0] && !tracked[0] {
            (self.left_hand.suspend_maintenance(), Vec::new())
        } else if self.support_blocked[0] && self.left_hand.get_held_entity().is_none() {
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
                right_held_entity.filter(|_| !maintenance_hand[0] || tracked[1]),
                right_position,
                ctx.step_dt,
            )
        };
        self.left_hand = left_hand;

        self.update_anticipation(ctx);
        left_msgs.append(&mut right_msgs);
        left_msgs.append(&mut slide_effects);
        left_msgs
    }

    fn toggle_grip_pose_tool(&mut self) {
        if self.pose_tool.is_some() {
            self.exit_pose_tool();
            return;
        }
        let Some((hand, held)) = self.poseable_held_item() else {
            println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL active=false reason=no_fitted_held_item");
            return;
        };
        println!(
            "SHOCK2QUEST_VR_GRIP_POSE_TOOL active=true model={} hand={hand}",
            held.model
        );
        let entity = held.entity;
        let virtual_hand = [&self.left_hand, &self.right_hand][hand];
        let hand_pose = GripPose {
            position: virtual_hand.get_position(),
            rotation: virtual_hand.get_rotation(),
        };
        self.support = None;
        self.slide = None;
        self.pose_tool_pulses.clear();
        self.pose_tool = Some(pose_tool::GripPoseTool::new(
            hand,
            entity,
            hand_pose,
            crate::paths::data_root().join(crate::vr_grip::GRIP_OVERRIDES_FILE),
        ));
    }

    fn can_pose_held_item(&self) -> bool {
        self.poseable_held_item().is_some()
    }

    fn pose_workbench_open(&self) -> bool {
        self.pose_tool.is_some()
    }

    fn update_pose_workbench(
        &mut self,
        world: &World,
        input: &InputContext,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        floor_y: f32,
        dt: f32,
    ) {
        self.update_pose_tool(world, input, player_pos, player_rotation, floor_y, dt);
    }

    fn pose_tool_button(&mut self, hand: Handedness, button: crate::hand_buttons::HandButton) {
        let pressed = crate::vr_config::hand_slot(hand);
        if self
            .pose_tool
            .as_ref()
            .is_none_or(|tool| tool.posing() == pressed)
        {
            return;
        }
        // Saving is on the panel: the lower button is test hold's, which
        // the hand on that controller toggles without looking.
        match button {
            crate::hand_buttons::HandButton::Lower => self.toggle_test_hold(),
            crate::hand_buttons::HandButton::Upper => self.discard_staged(),
        }
    }

    fn take_pose_tool_pulses(&mut self) -> Vec<(Handedness, crate::haptics::HapticPulse)> {
        std::mem::take(&mut self.pose_tool_pulses)
    }

    fn hold_pose_workbench(&mut self) {
        if let Some(tool) = self.pose_tool.as_mut() {
            tool.rearm();
        }
    }

    fn pose_workbench(&self) -> Option<PoseWorkbench> {
        let tool = self.pose_tool.as_ref().filter(|tool| tool.placed)?;
        Some(PoseWorkbench {
            entity: tool.entity,
            item: tool.item_placed?,
            item_scale: tool.item.map_or(1.0, |(_, scale)| scale),
            mirror: pose_frame(tool, self.fitted_grips[tool.hand].as_ref()),
            turn: tool.turn_matrix(),
            views: tool.views(),
            lights: tool.lights(),
            environment: tool.environment(),
        })
    }

    fn held_grip_anchor(&self, entity: EntityId) -> Option<Vector3<f32>> {
        let rig = self.grip_kinematics.as_ref()?;
        for (index, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
            if hand.get_held_entity() == Some(entity) {
                let held = self.fitted_grips[index].as_ref()?;
                return (held.entity == entity)
                    .then(|| held.primary_anchor(rig[index].palm_in_world()))
                    .flatten();
            }
        }
        None
    }

    fn is_supported(&self, entity: EntityId) -> bool {
        self.support
            .as_ref()
            .is_some_and(|s| s.active && s.entity == entity)
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
        lighting: Option<&crate::object_lighting::ObjectLighting<'_>>,
    ) -> Vec<SceneObject> {
        let mut glove_slot = self.glove_renderer.borrow_mut();
        let mut glove_renderer = glove_slot
            .get_or_insert_with(|| GloveRenderer::new(asset_cache))
            .as_mut();

        // The grip pose workbench draws its own hands; the frozen ones are not
        // where the player's hands are.
        if let Some(tool) = self.pose_tool.as_ref().filter(|tool| tool.placed) {
            let grip = self.fitted_grips[tool.hand]
                .as_ref()
                .and_then(|held| held.resolved.as_ref());
            let support = tool
                .support
                .then(|| self.pose_tool_support_glove(tool))
                .flatten();
            let mut objs = glove_renderer
                .map(|renderer| pose_tool::render(tool, renderer, grip, support, tool.lights()))
                .unwrap_or_default();
            crate::util::tag_render_source(&mut objs, crate::util::render_source::PLAYER_HANDS);
            drop(glove_slot);
            objs.extend(pose_tool::render_panel(tool, asset_cache));
            objs.extend(pose_tool::render_aim(tool));
            return objs;
        }
        let mut objs = Vec::new();
        let support_grip = self.slide_grip().or_else(|| {
            self.support.as_ref().and_then(|support| {
                let mut grip = self.fitted_grips[support.primary]
                    .as_ref()?
                    .resolved
                    .clone()?;
                grip.fingers = self.support_preview.as_ref()?.profile.fingers.clone();
                Some((1 - support.primary, grip))
            })
        });
        // Free gloves glow half cyan in reach of a hold. Held gloves flash cyan
        // as they catch, then both show what letting go of everything does:
        // green for a safe landing, red otherwise. Holding comes from the
        // resolved holds, so a blocked pull still shows a catch.
        let mut climb_lights = self
            .climb_reach
            .map(|reach| reach.then_some(crate::hand_glove::HandLight::ClimbReach));
        for (handedness, _) in self.hand_climb.grips() {
            let index = handedness as usize;
            climb_lights[index] = Some(crate::climb_release::held_glove_light(
                self.climb_held_seconds[index],
                self.climb_release_safe,
            ));
        }
        for (index, hand) in [&self.left_hand, &self.right_hand].into_iter().enumerate() {
            let card_grip = self.body_tool_hands[index]
                .then(|| self.personal_card_grip(index))
                .flatten();
            let grip = card_grip.as_ref().or_else(|| {
                self.fitted_grips[index]
                    .as_ref()
                    .and_then(|g| g.resolved.as_ref())
                    .or_else(|| {
                        support_grip
                            .as_ref()
                            .filter(|(i, _)| *i == index)
                            .map(|(_, g)| g)
                    })
            });
            objs.extend(
                hand.render(
                    world,
                    glove_renderer.as_deref_mut(),
                    grip.map(|grip| {
                        (
                            grip.fingers.hand_shape_at(hand.get_trigger_value()),
                            self.support
                                .as_ref()
                                .filter(|s| {
                                    1 - s.primary == index && hand.get_held_entity().is_none()
                                })
                                .map_or(1.0, |s| s.blend),
                        )
                    })
                    .or_else(|| self.climb_visuals[index].fingers()),
                    self.visual_hands[index],
                    Some(&self.anticipation[index]),
                    climb_lights[index],
                    lighting,
                ),
            );
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
        // Labelled as the player's hands: that is what `Game` drops while the
        // pause menu is up (issue #1018), and what `/v1/scene` reports. The
        // forearm panels carry their own label from `create_wrist_hud_panels`,
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
                        let palm = pose.point(rig[1 - c.primary].palm_in_world());
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
        if let Some(renderer) = glove_renderer.as_deref_mut() {
            let frames = [
                renderer.wrist_frame(Handedness::Left),
                renderer.wrist_frame(Handedness::Right),
            ];
            let tracked = self.hand_poses();
            let visible = std::array::from_fn(|i| self.visual_hands[i].unwrap_or(tracked[i]));
            let mut supported = [None, None];
            if let Some(s) = self.support.as_ref().filter(|s| s.active) {
                supported[1 - s.primary] = Some(s.entity);
            }
            objs.append(&mut create_wrist_hud_panels(
                asset_cache,
                world,
                use_mode,
                visible,
                frames,
                supported,
            ));
        }
        objs
    }

    fn hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        let mut lights = Vec::new();
        if crate::dev_params::get_bool(crate::dev_params::HAND_SPOTLIGHTS) {
            let cone = crate::dev_params::get(crate::dev_params::SPOTLIGHT_CONE);
            for hand in [&self.right_hand, &self.left_hand] {
                let dir = hand.get_rotation() * Vector3::new(0.0, 0.0, -1.0);
                lights.push(SpotLight {
                    position: hand.get_position(),
                    direction: dir.normalize(),
                    color_intensity: Vector4::new(
                        1.0,
                        1.0,
                        0.8,
                        crate::dev_params::get(crate::dev_params::SPOTLIGHT_INTENSITY),
                    ),
                    inner_cone_angle: (cone / 2.0).to_radians(),
                    outer_cone_angle: cone.to_radians(),
                    range: crate::dev_params::get(crate::dev_params::SPOTLIGHT_RANGE),
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

    fn restore_held(
        &mut self,
        world: &World,
        entity_id: EntityId,
        hand: Handedness,
    ) -> Vec<VirtualHandEffect> {
        let effects = self.grab(world, entity_id, hand);
        let restored_hand = if hand == Handedness::Left {
            &mut self.left_hand
        } else {
            &mut self.right_hand
        };
        if restored_hand.is_holding(entity_id) {
            restored_hand.preserve_restored_grip();
        }
        effects
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

    fn held_launch_origin(&self, entity_id: EntityId) -> Option<Point3<f32>> {
        let index = [&self.left_hand, &self.right_hand]
            .iter()
            .position(|hand| hand.get_held_entity() == Some(entity_id))?;
        let pose = self.hand_poses()[index];
        let palm = self
            .grip_kinematics
            .as_ref()
            .map_or(Vector3::new(0.0, 0.0, 0.0), |rig| {
                rig[index].palm_in_world()
            });
        let point = pose.point(palm);
        Some(Point3::new(point.x, point.y, point.z))
    }

    fn haptic_hands(&self, entity_id: EntityId) -> [Option<Handedness>; 2] {
        let primary = self.holding_hand(entity_id);
        let support = self
            .support
            .as_ref()
            .filter(|s| s.active && s.entity == entity_id && primary.is_some())
            .map(|s| {
                if s.primary == 0 {
                    Handedness::Right
                } else {
                    Handedness::Left
                }
            });
        [primary, support]
    }

    fn fumble(&mut self, entity_id: EntityId) -> Option<Handedness> {
        let hands = self.haptic_hands(entity_id);
        let primary = hands[0]?;
        self.on_entity_destroyed(entity_id);
        for hand in hands.into_iter().flatten() {
            let slot = match hand {
                Handedness::Left => &mut self.left_hand,
                Handedness::Right => &mut self.right_hand,
            };
            *slot = slot.spent_until_release();
        }
        Some(primary)
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
            ctx.flat_eye,
            ctx.step_dt,
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

    fn frob_target(&self) -> Option<EntityId> {
        self.controller.frob_target()
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

    fn kick_viewmodel(&mut self, entity: EntityId, impulse: crate::weapon_recoil::RecoilImpulse) {
        self.controller.kick(entity, impulse);
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

    fn flat_aim_bias(&self) -> cgmath::Vector2<f32> {
        self.controller.aim_bias()
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{FrobFlag, PropFrobInfo, PropModelName};

    use super::*;
    #[test]
    fn body_contacts_rotate_the_calibrated_palm_and_reject_missing_tracking() {
        use cgmath::{Deg, Rotation3};
        let mut interaction = VrInteraction::new();
        let mut input = InputContext::default();
        assert_eq!(interaction.body_palm_positions(&input), [None; 2]);
        interaction.grip_kinematics =
            Some(std::array::from_fn(|_| crate::vr_grip::GripKinematics {
                fingers: std::array::from_fn(|_| Vec::new()),
                palm: vec3(0.0, 0.0, -0.08),
                normal: Vector3::unit_x(),
            }));
        input.right_hand.position = vec3(1.0, 2.0, 3.0);
        input.right_hand.rotation = Quaternion::from_angle_y(Deg(90.0));
        let palm = interaction.body_palm_positions(&input)[1].unwrap();
        assert!((palm - vec3(0.92, 2.0, 3.0)).magnitude() < 0.0001);
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [true, false],
        });
        assert!(interaction.body_palm_positions(&input)[1].is_none());
        input.pose_tracking = None;
        input.right_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        assert!(interaction.body_palm_positions(&input)[1].is_none());
    }

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
            flat_eye: crate::death_camera::EyePose::flat(1.04, identity()),
            step_dt: 1.0 / 60.0,
            support_enabled: true,
            reserved_releases: &[],
        }
    }

    #[test]
    fn body_anticipation_is_visual_only_and_clears_on_tracking_loss() {
        let mut world = World::new();
        let entity = grabbable(&mut world);
        let physics = PhysicsWorld::new();
        let mut input = InputContext::default();
        input.left_hand.rotation = identity();
        input.right_hand.rotation = identity();
        let mut interaction = VrInteraction::new();
        // Updating suppressed hands sets the tracked transform without inventing a target.
        interaction.right_hand = interaction.right_hand.update_suppressed(
            vec3(0.0, 0.0, 0.0),
            identity(),
            &input.right_hand,
        );
        interaction.set_body_anticipation([None, Some(entity)]);
        for _ in 0..30 {
            interaction.update_anticipation(&context(&world, &physics, &input));
        }
        let feedback = interaction.hand_feedback_diagnostics();
        assert!(feedback["anticipation"][1]["curls"][2].as_f64().unwrap() > 0.45);
        assert_eq!(interaction.held_entities(), (None, None));
        assert_eq!(input.right_hand.squeeze_value, 0.0);
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [true, false],
        });
        interaction.update_anticipation(&context(&world, &physics, &input));
        assert_eq!(
            interaction.hand_feedback_diagnostics()["anticipation"][1]["curls"],
            serde_json::json!([0.0, 0.0, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn climb_reach_survives_paused_updates_but_clears_when_resuming_or_untracked() {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let mut input = InputContext::default();
        input.right_hand.rotation = identity();
        let mut interaction = VrInteraction::new();
        interaction.right_hand = interaction.right_hand.update_suppressed(
            vec3(0.0, 0.0, 0.0),
            identity(),
            &input.right_hand,
        );
        interaction.climb_anticipation = [false, true];
        interaction.update_anticipation(&context(&world, &physics, &input));
        assert_eq!(interaction.climb_reach, [false, true]);

        // The paused runtime pumps interaction without a new climb prepass.
        let mut paused = context(&world, &physics, &input);
        paused.step_dt = 0.0;
        for _ in 0..3 {
            interaction.update_anticipation(&paused);
            assert_eq!(interaction.climb_reach, [false, true]);
        }

        // A resumed frame with no target (including a mantle) clears the glow.
        interaction.update_anticipation(&context(&world, &physics, &input));
        assert_eq!(interaction.climb_reach, [false; 2]);
        interaction.climb_anticipation = [false, true];
        interaction.update_anticipation(&context(&world, &physics, &input));
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [true, false],
        });
        let mut paused = context(&world, &physics, &input);
        paused.step_dt = 0.0;
        interaction.update_anticipation(&paused);
        assert_eq!(interaction.climb_reach, [false; 2]);
    }

    #[test]
    fn the_hand_a_ui_panel_reads_eases_into_a_full_point() {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let mut input = InputContext::default();
        input.left_hand.rotation = identity();
        input.right_hand.rotation = identity();
        let mut interaction = VrInteraction::new();
        for hand in [&mut interaction.left_hand, &mut interaction.right_hand] {
            *hand = hand.update_suppressed(vec3(0.0, 0.0, 0.0), identity(), &input.right_hand);
        }
        interaction.set_ui_point([false, true]);
        for _ in 0..60 {
            interaction.update_anticipation(&context(&world, &physics, &input));
        }
        let point = |interaction: &VrInteraction, hand: usize| {
            interaction.hand_feedback_diagnostics()["anticipation"][hand]["point"]
                .as_f64()
                .unwrap()
        };
        assert!(point(&interaction, 1) > 0.99);
        assert!(point(&interaction, 0) < 0.01);
        // Leaving the panel eases back out.
        interaction.set_ui_point([false; 2]);
        for _ in 0..60 {
            interaction.update_anticipation(&context(&world, &physics, &input));
        }
        assert!(point(&interaction, 1) < 0.01);
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
                motion: None,
                rotation_degrees: [0.0; 3],
                fingers: crate::vr_grip::FingerPose::curls([0.5; 5]),
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
                fingers: crate::vr_grip::FingerPose::curls([0.5; 5]),
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

    /// The grip pose workbench open on the fixture's wrench (right hand) with
    /// a capture staged in the library, saving overrides to `path`.
    fn staged_workbench(path: std::path::PathBuf) -> VrInteraction {
        let (_world, entity, _physics, mut interaction, _input) = wrench_support_fixture();
        let entry = crate::vr_grip::BakedGripEntry {
            authored: true,
            model: "wrench_h".into(),
            hand: "right".into(),
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            grip: interaction.fitted_grips[1]
                .as_ref()
                .unwrap()
                .resolved
                .clone()
                .unwrap(),
        };
        interaction.grip_library = Some(crate::vr_grip::GripLibrary {
            entries: vec![entry.clone()],
            ..Default::default()
        });
        let hand_pose = GripPose {
            position: vec3(0.0, 1.0, 0.0),
            rotation: identity(),
        };
        let mut tool = pose_tool::GripPoseTool::new(1, entity, hand_pose, path);
        tool.staged = Some(StagedGrip::Grip {
            entry,
            previous: None,
            hand_pose,
        });
        interaction.pose_tool = Some(tool);
        interaction
    }

    #[test]
    fn exiting_the_workbench_saves_a_staged_pose_and_clicks_the_free_controller() {
        let dir = crate::test_support::TempDir::new("pose-exit-save");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        let mut interaction = staged_workbench(path.clone());
        interaction.toggle_grip_pose_tool();
        assert!(!interaction.pose_workbench_open());
        let saved = crate::vr_grip::GripOverrides::load(&path)
            .expect("saved on exit")
            .grips;
        assert!(saved.lookup("wrench_h", "right").is_some());
        assert!(
            interaction
                .grip_library
                .as_ref()
                .unwrap()
                .lookup("wrench_h", "right")
                .is_some(),
            "and applied"
        );
        assert_eq!(
            interaction.take_pose_tool_pulses(),
            vec![(Handedness::Left, crate::haptics::GRIP_POSE_SAVED)]
        );
    }

    /// A right-hand save also changes a left-held twin's mirrored grip, so
    /// leaving refits both hands.
    #[test]
    fn leaving_the_workbench_refits_the_other_hands_item_too() {
        let dir = crate::test_support::TempDir::new("pose-exit-refit");
        let mut interaction = staged_workbench(dir.path().join("o.json"));
        interaction.fitted_grips[0] = interaction.fitted_grips[1].clone();
        interaction.toggle_grip_pose_tool();
        assert!(!interaction.pose_workbench_open());
        assert!(interaction.fitted_grips.iter().all(Option::is_none));
    }

    /// Device crash: the posing hand's controller lies on the table, which
    /// Quest reports as the zero quaternion. Leaving the workbench must not
    /// place the still-held item with it (NaN -> parry broad-phase panic).
    #[test]
    fn leaving_the_workbench_with_the_posing_controller_set_down_places_nothing() {
        let (world, entity, physics, mut interaction, mut input) = wrench_support_fixture();
        let dir = crate::test_support::TempDir::new("pose-exit-untracked");
        let hand_pose = GripPose {
            position: input.right_hand.position,
            rotation: identity(),
        };
        interaction.pose_tool = Some(pose_tool::GripPoseTool::new(
            1,
            entity,
            hand_pose,
            dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE),
        ));
        interaction.toggle_grip_pose_tool();
        assert!(!interaction.pose_workbench_open());
        input.right_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        input.right_hand.squeeze_value = 0.0;
        for _ in 0..3 {
            let effects = interaction.update(&context(&world, &physics, &input));
            assert_eq!(interaction.right_hand.get_held_entity(), Some(entity));
            assert!(
                !effects.iter().any(|e| matches!(
                    e,
                    VirtualHandEffect::SetPositionRotation { entity_id, .. } if *entity_id == entity
                )),
                "the item is not placed"
            );
            // `fit_held_items` reads this pose: still lost, so it places nothing.
            let pose = GripPose {
                position: interaction.right_hand.get_position(),
                rotation: interaction.right_hand.get_rotation(),
            };
            assert!(!pose.is_tracked());
        }
    }

    #[test]
    fn a_staged_pose_that_fails_to_save_on_exit_stays_open_to_retry() {
        let dir = crate::test_support::TempDir::new("pose-exit-save-fails");
        // The overrides path is a directory: the save fails.
        let mut interaction = staged_workbench(dir.path().to_path_buf());
        interaction.toggle_grip_pose_tool();
        assert!(interaction.pose_workbench_open(), "kept open");
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert!(tool.staged.is_some(), "with the pose");
        assert!(tool.last_save_error.is_some(), "and why it failed");
        assert!(interaction.take_pose_tool_pulses().is_empty());
        // Repaired, exiting again saves and closes.
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        interaction.pose_tool.as_mut().unwrap().overrides_path = path.clone();
        interaction.toggle_grip_pose_tool();
        assert!(!interaction.pose_workbench_open());
        assert!(
            crate::vr_grip::GripOverrides::load(&path)
                .expect("saved")
                .grips
                .lookup("wrench_h", "right")
                .is_some()
        );
    }

    #[test]
    fn a_scale_edit_after_saving_is_unsaved_and_saves_on_exit() {
        let dir = crate::test_support::TempDir::new("pose-scale-save");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        let mut interaction = staged_workbench(path.clone());
        interaction.save_staged();
        assert!(interaction.pose_tool.as_ref().unwrap().saved);
        let before = interaction.fitted_grips[1]
            .as_ref()
            .unwrap()
            .resolved
            .as_ref()
            .unwrap()
            .item_scale;
        interaction.scale_pose_tool_item(before * 1.5);
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert!(
            tool.staged.is_some() && !tool.saved,
            "Unsaved, Save enabled"
        );
        interaction.toggle_grip_pose_tool();
        assert!(!interaction.pose_workbench_open());
        let saved = crate::vr_grip::GripOverrides::load(&path)
            .expect("saved on exit")
            .grips;
        let authored = before / crate::world_scale::WorldScale::active().personal();
        let scale = saved.lookup("wrench_h", "right").unwrap().item_scale;
        assert!((scale - authored * 1.5).abs() < 1e-5, "{scale}");
    }

    /// A glove fitted over a tracked right hand, its index curled `curl`.
    fn tracked_glove(curl: f32) -> pose_tool::TrackedGlove {
        let joints = crate::hand_capture::tests::openxr_right_hand(curl);
        let rig = crate::hand_capture::tests::pose_rig(Handedness::Right);
        let fit = crate::hand_capture::fit_hand(&joints, &rig).unwrap();
        pose_tool::TrackedGlove {
            pose: crate::hand_pose::Pose::from_finger_bones(&fit.bones),
            fit,
        }
    }

    /// A Trigger-slot snapshot sets only the pressed fingers, over the curl
    /// grip's rest converted to bones; a later rest snapshot keeps them; Save
    /// writes both and Discard puts back what was there before.
    #[test]
    fn a_trigger_slot_capture_sets_only_the_pressed_fingers_and_saves_with_the_rest() {
        use crate::vr_grip::FingerPose;
        let dir = crate::test_support::TempDir::new("pose-trigger-slot");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        let mut interaction = staged_workbench(path.clone());
        interaction.pose_tool.as_mut().unwrap().staged = None;
        let world = World::new();
        let grip = |interaction: &VrInteraction| {
            interaction.fitted_grips[1]
                .as_ref()
                .unwrap()
                .resolved
                .clone()
                .unwrap()
        };
        let before = grip(&interaction);
        assert!(
            !before.fingers.has_trigger_pose(),
            "a curl grip, at rest only"
        );
        let hand_pose = interaction.pose_tool.as_ref().unwrap().hand_pose;
        assert!(interaction.pose_tool.as_mut().unwrap().toggle_slot());

        let pulled = tracked_glove(1.0);
        assert!(interaction.capture_grip(&world, Some(&pulled)));
        let posed = grip(&interaction);
        assert!((posed.offset - before.offset).magnitude() < 1e-6);
        assert_eq!(posed.rotation, before.rotation);
        assert!((posed.item_scale - before.item_scale).abs() < 1e-6);
        assert_eq!(
            posed.fingers,
            FingerPose::Bones {
                bones: before.fingers.rest_bones(),
                trigger_bones: Some(pulled.fit.bones),
            },
            "the curl rest as bones, the pressed fingers captured"
        );
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert_eq!(
            (tool.hand_pose.position, tool.hand_pose.rotation),
            (hand_pose.position, hand_pose.rotation),
            "the glove's wrist stays put"
        );
        assert!(tool.staged.is_some());

        // A rest snapshot now keeps the pressed fingers.
        assert!(interaction.pose_tool.as_mut().unwrap().toggle_slot());
        let open = tracked_glove(0.0);
        assert!(interaction.capture_grip(&world, Some(&open)));
        let both = grip(&interaction);
        assert_eq!(
            both.fingers,
            FingerPose::Bones {
                bones: open.fit.bones,
                trigger_bones: Some(pulled.fit.bones),
            }
        );

        interaction.save_staged();
        let saved = crate::vr_grip::GripOverrides::load(&path).expect("saved");
        let saved = saved.grips.lookup("wrench_h", "right").unwrap();
        assert_eq!(saved.fingers, both.fingers, "Save writes both poses");

        // Discarding another trigger snapshot returns to the saved pair.
        assert!(interaction.pose_tool.as_mut().unwrap().toggle_slot());
        assert!(interaction.capture_grip(&world, Some(&tracked_glove(0.5))));
        assert_ne!(grip(&interaction).fingers, both.fingers);
        interaction.discard_staged();
        assert_eq!(grip(&interaction).fingers, both.fingers);
    }

    /// Test hold on a gun (the fixture's item given the shotgun's model):
    /// the posing controller holds it where play would, a lost controller
    /// leaves it be, Align aim fires it down the controller, and leaving
    /// the workbench mid-hold still saves.
    #[test]
    fn test_hold_holds_a_gun_as_play_does_and_align_aims_it_down_the_controller() {
        use cgmath::{Deg, Rotation3};
        let (mut world, entity, _physics, mut interaction, mut input) = wrench_support_fixture();
        world.add_component(entity, dark::properties::PropModelName("sg_h".to_owned()));
        let barrel = vec3(-1.0, 0.0, 0.0);
        let skewed = Quaternion::from_angle_y(Deg(28.0)) * Quaternion::from_angle_y(Deg(-90.0));
        let grip = |interaction: &VrInteraction| {
            interaction.fitted_grips[1]
                .as_ref()
                .unwrap()
                .resolved
                .clone()
                .unwrap()
        };
        interaction.fitted_grips[1]
            .as_mut()
            .unwrap()
            .resolved
            .as_mut()
            .unwrap()
            .rotation = skewed;
        let dir = crate::test_support::TempDir::new("pose-test-hold");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        interaction.toggle_grip_pose_tool();
        interaction.pose_tool.as_mut().unwrap().overrides_path = path.clone();
        input.head.rotation = identity();
        let origin = vec3(0.0, 0.0, 0.0);
        let step = |interaction: &mut VrInteraction, input: &InputContext| {
            interaction.update_pose_tool(&world, input, origin, identity(), 0.0, 1.0 / 60.0);
        };
        step(&mut interaction, &input);
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert!(tool.muzzle.is_some(), "a gun");
        assert!(tool.placed);
        let bench = tool.item_placed.unwrap();

        // Nothing staged: the lower button does nothing.
        interaction.pose_tool_button(Handedness::Left, crate::hand_buttons::HandButton::Lower);
        assert!(interaction.pose_tool.as_ref().unwrap().test_hold.is_none());
        let personal = crate::world_scale::WorldScale::active().personal();
        interaction.stage_grip(grip(&interaction).rescaled(1.0 / personal));
        // The posing hand's own button is not the tool's.
        interaction.pose_tool_button(Handedness::Right, crate::hand_buttons::HandButton::Lower);
        assert!(interaction.pose_tool.as_ref().unwrap().test_hold.is_none());
        interaction.pose_tool_button(Handedness::Left, crate::hand_buttons::HandButton::Lower);
        assert!(interaction.pose_tool.as_ref().unwrap().test_hold.is_some());

        // Picked up: held as `fit_held_items` holds it for this controller.
        input.right_hand.position = vec3(0.2, 1.1, -0.3);
        input.right_hand.rotation = Quaternion::from_angle_x(Deg(-20.0));
        step(&mut interaction, &input);
        let controller = GripPose {
            position: hand_world_position(origin, identity(), input.right_hand.position),
            rotation: input.right_hand.rotation,
        };
        let tool = interaction.pose_tool.as_ref().unwrap();
        let expected = pose_tool::item_pose(controller, &grip(&interaction), vec3(0.0, 0.0, 0.0));
        let placed = tool.item_placed.unwrap();
        assert!((placed.position - expected.position).magnitude() < 1e-5);
        assert!((placed.rotation - expected.rotation).magnitude() < 1e-5);
        let off = Deg::from(
            pose_tool::firing_direction(&grip(&interaction), barrel)
                .angle(pose_tool::CONTROLLER_AIM),
        )
        .0;
        let reported = tool.aim.unwrap().error_deg();
        assert!(
            (reported - off).abs() < 1e-3 && off > 27.0,
            "{reported} vs {off}"
        );
        assert_eq!(
            pose_hand_switch(tool, interaction.fitted_grips[1].as_ref(), None),
            Err("end test hold first".to_owned())
        );
        assert_eq!(
            support_mode(
                tool,
                interaction.fitted_grips[1].as_ref(),
                &interaction.support_profiles
            ),
            Err("end test hold first".to_owned())
        );

        // Set down (the zero quaternion): it stays where it was held.
        input.right_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        step(&mut interaction, &input);
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert_eq!(tool.item_placed.unwrap().position, placed.position);
        assert!(tool.aim.is_none());
        assert_eq!(
            tool.panel_view().status.text(),
            "Pick up the Right controller"
        );

        // Align aim: fires down the controller, roll kept; the bench item
        // stays put for when test hold ends.
        interaction.align_aim(vec3(0.0, 0.0, 0.0));
        let aligned = grip(&interaction);
        let error = Deg::from(
            pose_tool::firing_direction(&aligned, barrel).angle(pose_tool::CONTROLLER_AIM),
        )
        .0;
        assert!(error < 1e-3, "{error}");
        let tool = interaction.pose_tool.as_ref().unwrap();
        let back = pose_tool::item_pose(tool.hand_pose, &aligned, vec3(0.0, 0.0, 0.0));
        assert!((back.position - bench.position).magnitude() < 1e-5);
        input.right_hand.rotation = Quaternion::from_angle_x(Deg(-20.0));
        step(&mut interaction, &input);
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert!(tool.aim.unwrap().error_deg() < 1e-3);

        // Leaving mid-hold saves the aligned grip.
        interaction.toggle_grip_pose_tool();
        assert!(!interaction.pose_workbench_open());
        let saved = crate::vr_grip::GripOverrides::load(&path).expect("saved on exit");
        let saved = saved.grips.lookup("wrench_h", "right").unwrap();
        let error =
            Deg::from(pose_tool::firing_direction(saved, barrel).angle(pose_tool::CONTROLLER_AIM))
                .0;
        assert!(error < 1e-3, "{error}");
    }

    #[test]
    fn a_support_capture_previews_against_the_frozen_primary_then_saves_or_discards() {
        use cgmath::Rotation3;
        let dir = crate::test_support::TempDir::new("pose-support");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        let mut interaction = staged_workbench(path.clone());
        let mode = |interaction: &VrInteraction| {
            let tool = interaction.pose_tool.as_ref().unwrap();
            support_mode(
                tool,
                interaction.fitted_grips[1].as_ref(),
                &interaction.support_profiles,
            )
        };
        assert_eq!(mode(&interaction), Err("save or discard first".to_owned()));
        interaction.pose_tool.as_mut().unwrap().staged = None;
        assert_eq!(mode(&interaction), Ok(()));
        interaction.pose_tool.as_mut().unwrap().support_mode = Ok(());
        interaction.toggle_support_mode();
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert!(tool.support);
        assert_eq!(tool.posing(), 0, "the left hand poses the support grip");
        let primary = tool.hand_pose;
        let authored = interaction.support_profiles["wrench_h"].clone();

        // A tracked left hand on the wrench's shaft, turned.
        let glove = GripPose {
            position: vec3(0.03, 1.25, -0.02),
            rotation: Quaternion::from_angle_z(cgmath::Deg(80.0)),
        };
        let bones = crate::vr_grip::FingerPose::curls([0.8; 5]).to_bones();
        let crate::vr_grip::FingerPose::Bones { bones, .. } = bones else {
            unreachable!()
        };
        assert!(interaction.capture_support(Some((glove, bones))));
        let tool = interaction.pose_tool.as_ref().unwrap();
        assert!(tool.staged.is_some() && !tool.saved);
        assert_eq!(
            (tool.hand_pose.position, tool.hand_pose.rotation),
            (primary.position, primary.rotation),
            "the primary glove stays frozen"
        );
        // The previewed support glove lands where the hand was captured.
        let (drawn, _) = interaction.pose_tool_support_glove(tool).unwrap();
        assert!((drawn.position - glove.position).magnitude() < 1e-4);
        for axis in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
            assert!((drawn.rotation * axis - glove.rotation * axis).magnitude() < 1e-4);
        }
        let posed = interaction.support_profiles["wrench_h"].clone();
        assert_eq!(posed.region, authored.region);
        assert_eq!(posed.grab_radius, authored.grab_radius);

        interaction.save_staged();
        let saved = crate::vr_grip::GripOverrides::load(&path).expect("saved");
        assert!(saved.grips.entries.is_empty());
        assert_eq!(authored.with_pose(&saved.support["wrench_h"]), posed);
        assert_eq!(
            interaction.take_pose_tool_pulses(),
            vec![(Handedness::Right, crate::haptics::GRIP_POSE_SAVED)],
            "the free (primary) controller clicks"
        );

        // Another capture, discarded, returns to the saved pose.
        let moved = GripPose {
            position: glove.position + vec3(0.0, 0.05, 0.0),
            ..glove
        };
        assert!(interaction.capture_support(Some((moved, bones))));
        assert_ne!(interaction.support_profiles["wrench_h"], posed);
        interaction.discard_staged();
        assert_eq!(interaction.support_profiles["wrench_h"], posed);
        assert!(interaction.pose_tool.as_ref().unwrap().staged.is_none());
    }

    #[test]
    fn a_handed_weapon_poses_its_other_hand_mirrored_unless_that_hand_has_its_own() {
        use cgmath::Rotation3;
        let dir = crate::test_support::TempDir::new("pose-handed");
        let interaction = staged_workbench(dir.path().join("o.json"));
        let mut tool = interaction.pose_tool.unwrap();
        tool.staged = None;
        let base = interaction.fitted_grips[1].as_ref().unwrap();
        let grip = |offset: Vector3<f32>, yaw: f32| crate::vr_grip::ResolvedGrip {
            offset,
            rotation: Quaternion::from_angle_y(cgmath::Deg(yaw)),
            ..base.resolved.clone().unwrap()
        };
        let entry =
            |hand: &str, grip: crate::vr_grip::ResolvedGrip| crate::vr_grip::BakedGripEntry {
                authored: true,
                model: "wrench_h".into(),
                hand: hand.into(),
                surface_hash: String::new(),
                kinematics_hash: String::new(),
                hints_hash: String::new(),
                grip,
            };
        let shipped_left = grip(vec3(0.0, 0.1, 0.0), 0.0);
        let posed_right = grip(vec3(0.05, 0.01, 0.02), 30.0);
        let mut library = crate::vr_grip::GripLibrary {
            entries: vec![entry("left", shipped_left.clone())],
            ..Default::default()
        };
        library.prepend(crate::vr_grip::GripLibrary {
            entries: vec![entry("right", posed_right.clone())],
            ..Default::default()
        });
        let held = interaction.fitted_grips[1].clone().unwrap();
        // The right hand's headset pose, mirrored, outranks the shipped left.
        assert_eq!(
            pose_hand_switch(&tool, Some(&held), Some(&library)),
            Ok(posed_right.mirrored(Handedness::Left.mirror()).unwrap())
        );
        // A glove weapon the bench cannot mirror stays in its holder's hand.
        let unmirrored = HeldGrip {
            model_mirror: None,
            ..held.clone()
        };
        assert_eq!(
            pose_hand_switch(&tool, Some(&unmirrored), Some(&library)),
            Err("hold it in that hand".to_owned())
        );
        // An item drawn alike in both hands keeps its own hand's grip.
        let prop = HeldGrip {
            model: "mug".into(),
            ..unmirrored.clone()
        };
        for entry in &mut library.entries {
            entry.model = "mug".into();
        }
        assert_eq!(
            pose_hand_switch(&tool, Some(&prop), Some(&library)),
            Ok(shipped_left)
        );
        for entry in &mut library.entries {
            entry.model = "wrench_h".into();
        }
        // A left pose of its own wins.
        let posed_left = grip(vec3(0.0, 0.0, 0.07), -10.0);
        library.prepend(crate::vr_grip::GripLibrary {
            entries: vec![entry("left", posed_left.clone())],
            ..Default::default()
        });
        assert_eq!(
            pose_hand_switch(&tool, Some(&held), Some(&library)),
            Ok(posed_left)
        );
        // The bench draws the item mirrored only for the hand not holding it.
        assert_eq!(pose_frame(&tool, Some(&held)), None);
        tool.hand = 0;
        assert_eq!(
            pose_frame(&tool, Some(&held)),
            Some(Handedness::Left.mirror())
        );
        assert_eq!(pose_frame(&tool, Some(&unmirrored)), None);
    }

    #[test]
    fn manual_slide_tracking_pause_motion_and_ownership() {
        use crate::runtime_props::{
            RuntimePropGloveWeapon, RuntimePropJointTransforms, RuntimePropObjectArticulation,
        };
        use cgmath::{Matrix4, SquareMatrix};
        use dark::object_articulation::{ObjectArticulation, ObjectJoint};
        use dark::ss2_skeleton::{Bone, Skeleton};
        let (mut world, entity, physics, mut interaction, mut input) = wrench_support_fixture();
        interaction.fitted_grips[1].as_mut().unwrap().model = "atek_h".into();
        let rig = ObjectArticulation {
            skeleton: Skeleton::create_from_bones(vec![
                Bone {
                    joint_id: 0,
                    parent_id: None,
                    local_transform: Matrix4::identity(),
                },
                Bone {
                    joint_id: 1,
                    parent_id: Some(0),
                    local_transform: Matrix4::identity(),
                },
            ]),
            joints: vec![ObjectJoint {
                index: 1,
                parameter: 0,
                motion_type: 2,
                vhot_range: 0..0,
            }],
            vhots: Vec::new(),
        };
        let transforms = rig.pose(&[]);
        world.add_component(
            entity,
            (
                RuntimePropObjectArticulation(std::sync::Arc::new(rig)),
                RuntimePropJointTransforms(transforms),
                RuntimePropGloveWeapon {
                    item_scale: 1.0,
                    magazine_removed: false,
                    magazine_anchor: None,
                },
            ),
        );
        interaction.update(&context(&world, &physics, &input));
        interaction.synchronize_slide(&world);
        let diagnostic = interaction.slide_diagnostics(entity).unwrap();
        let position: Vector3<f32> =
            serde_json::from_value(diagnostic["controller_position"].clone()).unwrap();
        // Diagnostic controller positions undo the runtime calibration; unit inputs
        // already are calibrated, so use the contact's glove position directly.
        let rotation: Quaternion<f32> =
            serde_json::from_value(diagnostic["controller_rotation"].clone()).unwrap();
        input.left_hand.rotation = rotation;
        input.left_hand.position = position
            + crate::glove_fit::forward_translation(
                rotation,
                crate::dev_params::get(crate::dev_params::GLOVE_FORWARD_CM),
            );
        interaction.update(&context(&world, &physics, &input));
        // The squeeze frame's poses, not last frame's hands, locate acquisition.
        input.left_hand.position += vec3(1.0, 0.0, 0.0);
        input.right_hand.position += vec3(1.0, 0.0, 0.0);
        input.left_hand.squeeze_value = 1.0;
        let mut ctx = context(&world, &physics, &input);
        ctx.step_dt = 0.0;
        assert!(
            !interaction
                .update(&ctx)
                .iter()
                .any(|e| matches!(e, VirtualHandEffect::MoveSlide { .. }))
        );
        assert!(
            interaction.slide.is_some(),
            "paused redraw consumes a grip edge"
        );
        input.left_hand.position.x += 0.25 / dark::SCALE_FACTOR;
        let effects = interaction.update(&context(&world, &physics, &input));
        assert!(effects.iter().any(
            |e| matches!(e, VirtualHandEffect::MoveSlide { fraction, .. } if *fraction > 0.99)
        ));
        // Common hand translation must not count as pulling or break the grip.
        input.left_hand.position += vec3(1.0, 0.0, 0.0);
        input.right_hand.position += vec3(1.0, 0.0, 0.0);
        let effects = interaction.update(&context(&world, &physics, &input));
        assert!(effects.iter().any(
            |e| matches!(e, VirtualHandEffect::MoveSlide { fraction, .. } if *fraction > 0.99)
        ));
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [false, true],
        });
        interaction.update(&context(&world, &physics, &input));
        assert!(interaction.slide.is_none());
        input.pose_tracking = None;
        interaction.update(&context(&world, &physics, &input));
        assert!(
            interaction.slide.is_none(),
            "tracking recovery cannot reacquire a held squeeze"
        );
        input.right_hand.squeeze_value = 0.0;
        let effects = interaction.update(&context(&world, &physics, &input));
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, VirtualHandEffect::HoldItem { .. })),
            "a slide grip must not become a handoff"
        );
        assert_eq!(interaction.held_entities(), (None, None));
    }

    #[test]
    fn weapon_clearance_starts_at_palm_despite_model_anchor_offset() {
        let (world, entity, physics, mut interaction, input) = wrench_support_fixture();
        interaction.grip_kinematics.as_mut().unwrap()[1].palm = vec3(0.0, 0.0, 0.1);
        interaction.fitted_grips[1]
            .as_mut()
            .unwrap()
            .resolved
            .as_mut()
            .unwrap()
            .offset = vec3(0.0, 0.0, 0.5);
        interaction.update(&context(&world, &physics, &input));
        let start = interaction.held_launch_origin(entity).unwrap();
        assert!(
            (start.z - 0.1).abs() < 0.001,
            "must start at the palm, not the offset model: {start:?}"
        );
    }

    #[test]
    fn recoil_routes_only_to_the_owner_and_active_support_hand() {
        let (world, entity, physics, mut interaction, mut input) = wrench_support_fixture();
        assert_eq!(
            interaction.haptic_hands(entity),
            [Some(Handedness::Right), None]
        );
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert_eq!(
            interaction.haptic_hands(entity),
            [Some(Handedness::Right), Some(Handedness::Left)]
        );
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert_eq!(
            interaction.haptic_hands(entity),
            [Some(Handedness::Right), None]
        );
        assert_eq!(interaction.haptic_hands(EntityId::dead()), [None; 2]);
        assert_eq!(FlatInteraction::new().haptic_hands(entity), [None; 2]);
    }

    #[test]
    fn support_tracking_loss_requires_a_fresh_grab() {
        let (world, _, physics, mut interaction, mut input) = wrench_support_fixture();
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [false, true],
        });
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        input.pose_tracking = None;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(
            !interaction.support.as_ref().unwrap().active,
            "tracking recovery is not a new squeeze"
        );
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
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
        assert!(interaction.is_supported(entity));
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
        assert!(interaction.is_supported(entity));
        input.left_hand.position.y = 2.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        assert!(!interaction.is_supported(entity));
        input.left_hand.position.y = 1.2;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        assert!(!interaction.is_supported(entity));
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        assert!(interaction.is_supported(entity));
        input.left_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        assert!(!interaction.is_supported(entity));
        input.left_hand.rotation = identity();
        interaction.update_support(&context(&world, &physics, &input));
        assert!(!interaction.support.as_ref().unwrap().active);
        assert!(!interaction.is_supported(entity));
        input.left_hand.squeeze_value = 0.0;
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update_support(&context(&world, &physics, &input));
        assert!(interaction.support.as_ref().unwrap().active);
        assert!(interaction.is_supported(entity));
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
        assert!(interaction.is_supported(entity));
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
        assert!(!interaction.is_supported(entity));
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

        // The release may be rewritten into a backpack deposit by the caller.
        // Until effects remove its body, the left hand must not steal it.
        input.right_hand.squeeze_value = 0.0;
        let reserved = [item];
        let mut ctx = context(&world, &physics, &input);
        ctx.reserved_releases = &reserved;
        let effects = interaction.update(&ctx);
        assert_eq!(interaction.held_entities(), (None, None));
        assert!(effects.iter().any(|effect| matches!(
            effect, VirtualHandEffect::DropItem { entity_id, .. } if *entity_id == item
        )));
        assert!(!effects.iter().any(|effect| matches!(
            effect, VirtualHandEffect::HoldItem { entity_id } if *entity_id == item
        )));
    }

    #[test]
    fn nearby_squeezed_hand_receives_the_same_item_on_release_in_either_direction() {
        for giver in [Handedness::Left, Handedness::Right] {
            let mut world = World::new();
            let item = grabbable(&mut world);
            let physics = PhysicsWorld::new(); // No collider or pointing ray required.
            let mut interaction = VrInteraction::new();
            interaction.grab(&world, item, giver);
            let mut input = InputContext::default();
            input.left_hand.position = vec3(0.2, 0.0, 0.0);
            input.left_hand.squeeze_value = 1.0;
            input.right_hand.squeeze_value = 1.0;
            // An already-held trigger must not use/fire the receiving item.
            input.left_hand.trigger_value = 1.0;
            input.right_hand.trigger_value = 1.0;
            interaction.update(&context(&world, &physics, &input));
            assert_eq!(interaction.holding_hand(item), Some(giver));
            let receiver = if giver == Handedness::Left {
                input.left_hand.squeeze_value = 0.0;
                Handedness::Right
            } else {
                input.right_hand.squeeze_value = 0.0;
                Handedness::Left
            };
            let effects = interaction.update(&context(&world, &physics, &input));
            assert_eq!(interaction.holding_hand(item), Some(receiver));
            assert_eq!(
                interaction.held_entities(),
                if receiver == Handedness::Left {
                    (Some(item), None)
                } else {
                    (None, Some(item))
                }
            );
            assert!(
                matches!(effects.as_slice(), [
                VirtualHandEffect::OutMessage { message: crate::scripts::Message { to, payload: MessagePayload::Drop } },
                VirtualHandEffect::HoldItem { entity_id },
                VirtualHandEffect::SetPositionRotation { entity_id: moved, .. },
            ] if *to == item && *entity_id == item && *moved == item),
                "{effects:?}"
            );
            let effects = interaction.update(&context(&world, &physics, &input));
            assert!(effects.iter().all(|e| !matches!(
                e,
                VirtualHandEffect::HoldItem { .. } | VirtualHandEffect::DropItem { .. }
            )));
            // The recipient still drops normally when it opens its own grip.
            input.left_hand.squeeze_value = 0.0;
            input.right_hand.squeeze_value = 0.0;
            let effects = interaction.update(&context(&world, &physics, &input));
            assert_eq!(interaction.held_entities(), (None, None));
            assert!(effects.iter().any(
                |e| matches!(e, VirtualHandEffect::DropItem { entity_id, .. } if *entity_id == item)
            ));
        }
    }

    #[test]
    fn handoff_requires_a_nearby_free_tracked_hand_and_an_unclaimed_real_release() {
        for case in [
            "far",
            "open",
            "occupied",
            "still_held",
            "reserved",
            "head_untracked",
            "giver_untracked",
            "receiver_untracked",
            "invalid_pose",
            "restored",
            "climbing",
        ] {
            let mut world = World::new();
            let item = grabbable(&mut world);
            let physics = PhysicsWorld::new();
            let mut interaction = VrInteraction::new();
            interaction.grab(&world, item, Handedness::Right);
            let mut input = InputContext::default();
            input.left_hand.position = vec3(0.2, 0.0, 0.0);
            input.left_hand.squeeze_value = 1.0;
            let mut reserved = Vec::new();
            match case {
                "far" => input.left_hand.position.x = 0.31,
                "open" => input.left_hand.squeeze_value = 0.0,
                "occupied" => {
                    let other = grabbable(&mut world);
                    interaction.grab(&world, other, Handedness::Left);
                }
                "still_held" => input.right_hand.squeeze_value = 1.0,
                "reserved" => reserved.push(item),
                "head_untracked" | "giver_untracked" | "receiver_untracked" => {
                    input.pose_tracking = Some(crate::input_context::PoseTracking {
                        head: case != "head_untracked",
                        hands: [case != "receiver_untracked", case != "giver_untracked"],
                    })
                }
                "invalid_pose" => input.left_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0),
                "restored" => interaction.right_hand.preserve_restored_grip(),
                "climbing" => {
                    for squeeze in [0.0, 1.0] {
                        interaction.hand_climb.update(
                            vec3(0.0, 0.0, 0.0),
                            identity(),
                            1.0 / 60.0,
                            [
                                crate::vr_climb::ClimbHandInput {
                                    local_position: input.left_hand.position,
                                    squeeze,
                                    is_empty: true,
                                },
                                crate::vr_climb::ClimbHandInput {
                                    local_position: input.right_hand.position,
                                    squeeze: 1.0,
                                    is_empty: false,
                                },
                            ],
                            |point, _| {
                                Some(crate::physics::ClimbGrip {
                                    kind: crate::physics::ClimbGripKind::Ladder,
                                    entity_id: None,
                                    point,
                                    normal: vec3(1.0, 0.0, 0.0),
                                })
                            },
                            |_| true,
                        );
                    }
                    assert_eq!(interaction.hand_climb.grips().count(), 1);
                }
                _ => unreachable!(),
            }
            let mut ctx = context(&world, &physics, &input);
            ctx.reserved_releases = &reserved;
            let effects = interaction.update(&ctx);
            assert_ne!(
                interaction.holding_hand(item),
                Some(Handedness::Left),
                "{case}"
            );
            assert!(
                !effects.iter().any(
                    |e| matches!(e, VirtualHandEffect::HoldItem { entity_id } if *entity_id == item)
                ),
                "{case}: {effects:?}"
            );
        }
    }

    #[test]
    fn supporting_hand_can_receive_a_nearby_release_without_regripping() {
        let (world, item, physics, mut interaction, mut input) = wrench_support_fixture();
        interaction.update_support(&context(&world, &physics, &input));
        input.left_hand.squeeze_value = 1.0;
        interaction.update(&context(&world, &physics, &input));
        assert!(interaction.is_supported(item));
        assert!(interaction.support_blocked[0]);
        input.right_hand.squeeze_value = 0.0;
        let effects = interaction.update(&context(&world, &physics, &input));
        assert_eq!(interaction.held_entities(), (Some(item), None));
        assert!(!interaction.is_supported(item));
        assert!(!interaction.support_blocked[0]);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, VirtualHandEffect::DropItem { .. }))
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
