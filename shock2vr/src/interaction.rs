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
}

struct GripGeometry {
    triangles: std::rc::Rc<Vec<[Point3<f32>; 3]>>,
    fingerprint: String,
}

struct HeldGrip {
    entity: EntityId,
    model: String,
    resolved: Option<crate::vr_grip::ResolvedGrip>,
    solve_ms: f64,
    surface_hash: String,
    kinematics_hash: String,
    hints_hash: String,
    source: &'static str,
    authored: bool,
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
        }
    }
}

impl Default for VrInteraction {
    fn default() -> Self {
        Self::new()
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
            self.grip_library = Some(
                assets
                    .get_opt(&TEXT_IMPORTER, "astra-vr-grips.json")
                    .and_then(|text| {
                        serde_json::from_str::<crate::vr_grip::GripLibrary>(&text).ok()
                    })
                    .filter(|library| library.version == 1)
                    .unwrap_or_default(),
            );
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
                        .entry(model.clone())
                        .or_insert_with(|| {
                            let triangles = assets
                                .get_opt(&GRIP_SURFACE_IMPORTER, &format!("{}.bin", model))?;
                            let fingerprint = crate::vr_grip::surface_fingerprint(&triangles);
                            Some(GripGeometry {
                                triangles,
                                fingerprint,
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
                let resolved = if bake {
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
                let authored = !bake
                    && resolved.is_some()
                    && self
                        .grip_library
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .any(|e| e.model == model && e.hand == hand_name && e.authored);
                self.fitted_grips[index] = Some(HeldGrip {
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
                effects.push(VirtualHandEffect::SetPositionRotation {
                    entity_id: entity,
                    position: hand.get_position() + hand.get_rotation().rotate_vector(grip.offset),
                    rotation: hand.get_rotation() * grip.rotation,
                    scale: cgmath::vec3(1.0, 1.0, 1.0),
                });
            }
        }
    }

    fn grip_diagnostics(&self) -> serde_json::Value {
        serde_json::Value::Array(self.fitted_grips.iter().enumerate().filter_map(|(i, grip)| {
            let grip=grip.as_ref()?;
            Some(serde_json::json!({"hand": if i == 0 {"left"} else {"right"}, "entity_id": grip.entity.inner() as i32,
                "model": grip.model, "solve_ms": grip.solve_ms, "grip": grip.resolved,
                "surface_hash": grip.surface_hash, "kinematics_hash": grip.kinematics_hash, "hints_hash": grip.hints_hash, "solver_revision": crate::vr_grip::SOLVER_REVISION, "source": grip.source, "authored": grip.authored,
                "palm": self.grip_kinematics.as_ref().map(|k| k[i].palm), "palm_normal": self.grip_kinematics.as_ref().map(|k| k[i].normal)}))
        }).collect())
    }

    fn update_hand_climb(&mut self, ctx: &ClimbContext) -> crate::vr_climb::ClimbFrame {
        use shipyard::EntitiesView;

        let hand_input = |hand: &VirtualHand, input: &crate::input_context::Hand| {
            crate::vr_climb::ClimbHandInput {
                local_position: input.position,
                squeeze: input.squeeze_value,
                // A hand that is carrying something cannot also hold a ladder.
                is_empty: hand.get_held_entity().is_none(),
            }
        };
        self.hand_climb.update(
            ctx.pawn_pos,
            ctx.pawn_rotation,
            ctx.step_dt,
            [
                hand_input(&self.left_hand, &ctx.input.left_hand),
                hand_input(&self.right_hand, &ctx.input.right_hand),
            ],
            |point, from_ladder| {
                if from_ladder {
                    ctx.physics.climbable_grip_from_ladder_at(point, ctx.feet_y)
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

    fn release_climb_grips(&mut self) {
        self.hand_climb.release_all();
    }

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
        let glove_renderer = glove_slot
            .get_or_insert_with(|| GloveRenderer::new(asset_cache))
            .as_mut();

        let mut objs = Vec::new();
        match glove_renderer {
            Some(renderer) => {
                objs.append(
                    &mut self.left_hand.render(
                        world,
                        Some(renderer),
                        self.fitted_grips[0]
                            .as_ref()
                            .and_then(|g| g.resolved.as_ref()),
                    ),
                );
                objs.append(
                    &mut self.right_hand.render(
                        world,
                        Some(renderer),
                        self.fitted_grips[1]
                            .as_ref()
                            .and_then(|g| g.resolved.as_ref()),
                    ),
                );
            }
            None => {
                objs.append(&mut self.left_hand.render(world, None, None));
                objs.append(&mut self.right_hand.render(world, None, None));
            }
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
