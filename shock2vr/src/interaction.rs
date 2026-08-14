//! Player interaction abstraction.
//!
//! One trait with two implementations - VR (two `VirtualHand`s) and flatscreen
//! (`FlatPlayerController`) - so `mission_core` drives interaction
//! polymorphically through a single `Box<dyn PlayerInteraction>` instead of
//! branching on `PresentationMode` and holding both sets of state.
//!
//! Both implementations speak the same `VirtualHandEffect` language, which
//! `mission_core` already processes in one place.

use std::cell::RefCell;

use cgmath::{InnerSpace, Point3, Quaternion, Vector3, Vector4};
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
    virtual_hand::{VirtualHand, VirtualHandEffect},
    vr_config::Handedness,
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
}

/// How the player interacts with the world. The effects returned by `update`
/// (and `grab`/`wield`) are applied by `mission_core::process_virtual_hand_effects`.
pub trait PlayerInteraction {
    /// Per-frame update; returns effects to apply.
    fn update(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect>;

    /// Entities held in (left, right) - for `PlayerInfo`. Flat reports its
    /// wielded weapon as the "left".
    fn held_entities(&self) -> (Option<EntityId>, Option<EntityId>);

    /// Entities under the reticle/hands, for the hover-highlight overlay.
    fn highlighted_entities(&self) -> Vec<EntityId>;

    /// The first-person viewmodel entity (drawn on top); `None` for VR.
    fn viewmodel_entity(&self) -> Option<EntityId> {
        None
    }

    /// 3D visuals owned by the controller (VR: hand models + forearm HUD
    /// panels). Flat draws nothing here; its weapon is drawn from
    /// `viewmodel_entity`.
    fn render(&self, _asset_cache: &mut AssetCache, _world: &World) -> Vec<SceneObject> {
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

    /// Whether `entity_id` is currently held.
    fn is_holding(&self, entity_id: EntityId) -> bool;

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
}

impl VrInteraction {
    pub fn new() -> Self {
        Self {
            left_hand: VirtualHand::new(Handedness::Left),
            right_hand: VirtualHand::new(Handedness::Right),
            glove_renderer: RefCell::new(None),
        }
    }
}

impl Default for VrInteraction {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerInteraction for VrInteraction {
    fn update(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect> {
        let left_held_entity = self.left_hand.get_held_entity();
        let (right_hand, mut right_msgs) = VirtualHand::update(
            &self.right_hand,
            ctx.physics,
            ctx.world,
            ctx.player_pos,
            ctx.player_rotation,
            &ctx.input.right_hand,
            left_held_entity,
        );
        self.right_hand = right_hand;

        // Right updates first, so a same-frame right-hand grab is visible to
        // the left hand and one physical item cannot enter both hand states.
        let right_held_entity = self.right_hand.get_held_entity();
        let (left_hand, mut left_msgs) = VirtualHand::update(
            &self.left_hand,
            ctx.physics,
            ctx.world,
            ctx.player_pos,
            ctx.player_rotation,
            &ctx.input.left_hand,
            right_held_entity,
        );
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

    fn render(&self, asset_cache: &mut AssetCache, world: &World) -> Vec<SceneObject> {
        let mut glove_slot = self.glove_renderer.borrow_mut();
        let glove_renderer = glove_slot
            .get_or_insert_with(|| GloveRenderer::new(asset_cache))
            .as_mut();

        let mut objs = Vec::new();
        match glove_renderer {
            Some(renderer) => {
                objs.append(&mut self.left_hand.render(Some(renderer)));
                objs.append(&mut self.right_hand.render(Some(renderer)));
            }
            None => {
                objs.append(&mut self.left_hand.render(None));
                objs.append(&mut self.right_hand.render(None));
            }
        }
        objs.append(&mut create_arm_hud_panels(
            asset_cache,
            world,
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
        if hand == Handedness::Left {
            self.left_hand = self.left_hand.grab_entity(world, entity_id);
        } else {
            self.right_hand = self.right_hand.grab_entity(world, entity_id);
        }
        Vec::new()
    }

    fn replace_entity(&mut self, old: EntityId, new: EntityId, rigid_body: RigidBodyHandle) {
        self.left_hand = self.left_hand.replace_entity(old, new, rigid_body);
        self.right_hand = self.right_hand.replace_entity(old, new, rigid_body);
    }

    fn on_entity_destroyed(&mut self, entity_id: EntityId) {
        self.left_hand = self.left_hand.destroy_entity(entity_id);
        self.right_hand = self.right_hand.destroy_entity(entity_id);
    }

    fn is_holding(&self, entity_id: EntityId) -> bool {
        self.left_hand.is_holding(entity_id) || self.right_hand.is_holding(entity_id)
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

    fn is_holding(&self, entity_id: EntityId) -> bool {
        self.controller.wielded_entity() == Some(entity_id)
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
        }
    }

    fn step_physics(physics: &mut PhysicsWorld) {
        let player = EntityId::from_inner(10_000).unwrap();
        let mut player = physics.create_player(vec3(10.0, 10.0, 10.0), player);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
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
