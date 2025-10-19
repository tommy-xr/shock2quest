use cgmath::{Matrix4, Quaternion, Vector2, Vector3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::SceneObject,
};
use shipyard::EntityId;
use tracing::{debug, info, warn};

use crate::{
    input_context::InputContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

use super::{
    mission_trait::{Mission, MissionTransition, MissionType},
    GlobalContext,
};

/// Central coordinator for mission management.
/// Handles the current active mission and transitions between different mission types.
pub struct MissionManager {
    current_mission: Box<dyn Mission>,
    pending_transition: Option<MissionTransition>,
}

impl MissionManager {
    /// Create a new mission manager with an initial mission.
    pub fn new(initial_mission: Box<dyn Mission>) -> Self {
        info!("MissionManager initialized with mission type: {}", initial_mission.mission_type());
        Self {
            current_mission: initial_mission,
            pending_transition: None,
        }
    }

    /// Get the current mission type.
    pub fn current_mission_type(&self) -> MissionType {
        self.current_mission.mission_type()
    }

    /// Check if there's a pending transition.
    pub fn has_pending_transition(&self) -> bool {
        self.pending_transition.is_some()
    }

    /// Get the pending transition without consuming it.
    pub fn peek_pending_transition(&self) -> Option<&MissionTransition> {
        self.pending_transition.as_ref()
    }

    /// Consume the pending transition.
    pub fn take_pending_transition(&mut self) -> Option<MissionTransition> {
        self.pending_transition.take()
    }

    /// Set a new mission, replacing the current one.
    /// This is typically called by the runtime when handling mission transitions.
    pub fn set_mission(&mut self, new_mission: Box<dyn Mission>) {
        let old_type = self.current_mission.mission_type();
        let new_type = new_mission.mission_type();
        info!("Mission transition: {} -> {}", old_type, new_type);

        self.current_mission = new_mission;
        self.pending_transition = None;
    }

    /// Update the current mission and handle any requested transitions.
    pub fn update(
        &mut self,
        time: &Time,
        asset_cache: &mut AssetCache,
        input_context: &InputContext,
    ) -> Vec<Effect> {
        // Update the current mission
        let effects = self.current_mission.update(time, asset_cache, input_context);

        // Check for transition requests
        if let Some(transition) = self.current_mission.should_transition() {
            debug!("Mission requested transition: {}", transition);
            self.pending_transition = Some(transition);
        }

        effects
    }

    /// Render the current mission.
    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        self.current_mission.render(asset_cache, options)
    }

    /// Handle effects from the current mission.
    pub fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.current_mission.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    /// Render per-eye content from the current mission.
    pub fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        self.current_mission.render_per_eye(asset_cache, view, projection, screen_size)
    }

    /// Finalize rendering for the current mission.
    pub fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.current_mission.finish_render(asset_cache, view, projection, screen_size)
    }
}

/// Factory functions for creating different mission types.
/// This will be expanded as we implement each mission type.
pub mod mission_factory {
    use super::*;

    /// Create a mission manager with a gameplay mission (current implementation).
    /// This is a temporary function until we implement the full mission system.
    pub fn create_gameplay_mission_manager(
        mission: crate::mission::Mission,
    ) -> MissionManager {
        // For now, we'll wrap the existing Mission in a compatibility layer
        let gameplay_mission = Box::new(GameplayMissionWrapper::new(mission));
        MissionManager::new(gameplay_mission)
    }
}

/// Temporary wrapper to make the existing Mission struct compatible with the new trait.
/// This will be replaced with a proper GameplayMission implementation in Phase 1.4.
struct GameplayMissionWrapper {
    inner: crate::mission::Mission,
}

impl GameplayMissionWrapper {
    fn new(mission: crate::mission::Mission) -> Self {
        Self { inner: mission }
    }
}

impl Mission for GameplayMissionWrapper {
    fn update(
        &mut self,
        _time: &Time,
        _asset_cache: &mut AssetCache,
        _input_context: &InputContext,
    ) -> Vec<Effect> {
        // Delegate to the existing Mission's update method
        // Note: We'll need to adjust this once we examine the existing Mission's update signature
        warn!("GameplayMissionWrapper::update - using temporary implementation");
        Vec::new()
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Delegate to the existing Mission's render method
        warn!("GameplayMissionWrapper::render - using temporary implementation");
        (Vec::new(), Vector3::new(0.0, 0.0, 0.0), Quaternion::new(1.0, 0.0, 0.0, 0.0))
    }

    fn handle_effects(
        &mut self,
        _effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        // Delegate to the existing Mission's handle_effects method
        warn!("GameplayMissionWrapper::handle_effects - using temporary implementation");
        Vec::new()
    }

    fn render_per_eye(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        // Delegate to the existing Mission's render_per_eye method
        warn!("GameplayMissionWrapper::render_per_eye - using temporary implementation");
        Vec::new()
    }

    fn finish_render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
    ) {
        // Delegate to the existing Mission's finish_render method
        warn!("GameplayMissionWrapper::finish_render - using temporary implementation");
    }

    fn mission_type(&self) -> MissionType {
        MissionType::Gameplay
    }

    fn should_transition(&self) -> Option<MissionTransition> {
        // For now, no transitions from gameplay
        None
    }
}