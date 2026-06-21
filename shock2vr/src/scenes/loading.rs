//! Loading screen.
//!
//! A `GameScene` that draws the original animated loading screen while a level
//! loads, so the window keeps presenting frames instead of freezing on a black
//! frame (projects/loading-screen.md). The original screen is a 3-layer composite,
//! all shipped in `intrface.crf`:
//!
//! 1. `LOADING.PCX` — the 640x480 backdrop (frame + bracket).
//! 2. `meters/LOADA_01..20.PCX` — a 272x272 center disc, cycled for the rotation.
//! 3. `meters/PROGRESS.PCX` — a 246x20 bar, clipped 0..1 as the "% Transfer
//!    Completed" fill.
//!
//! The rotation is driven by the frame clock (always animates). The bar fill is
//! driven by `progress`: `new_demo()` sweeps it for visual inspection via the
//! `debug_loading` scene; real background loading (PR 3) calls `set_progress`.

use std::collections::HashMap;

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalContext, GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{Rect, ScaleMode, UiCanvas},
};

/// The loading art is authored on the original 640x480 `LOADING.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "LOADING.PCX";
const PROGRESS_TEXTURE: &str = "meters/PROGRESS.PCX";
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// The rotating center disc (`meters/LOADA_01..20.PCX`, 272x272), centered on the
/// backdrop's circular frame.
const DISC_SIZE: f32 = 272.0;
const DISC_CENTER_X: f32 = 320.0;
const DISC_CENTER_Y: f32 = 268.0;
const LOADA_FRAME_COUNT: u32 = 20;
/// Rotation speed; 20 frames at 15 fps -> a full cycle every ~1.3s.
const LOADA_FPS: f32 = 15.0;

/// The "% Transfer Completed" bar fill (`meters/PROGRESS.PCX`, 246x20), in the
/// backdrop's bracket near the bottom.
const BAR_W: f32 = 246.0;
const BAR_H: f32 = 20.0;
const BAR_X: f32 = (CANVAS_W - BAR_W) / 2.0;
const BAR_Y: f32 = 398.0;

/// Demo sweep period (seconds) for the `debug_loading` scene: fill 0->1, repeat.
const DEMO_FILL_SECS: f32 = 4.0;

pub struct LoadingScene {
    world: World,
    scene_name: String,
    /// Load progress in 0..=1 (drives the bar fill).
    progress: f32,
    /// Total elapsed seconds (drives the rotation, independent of progress).
    elapsed_secs: f32,
    /// When true, `progress` is swept from `elapsed_secs` for visual inspection.
    demo: bool,
}

impl LoadingScene {
    /// A loading scene whose bar is driven externally via [`set_progress`].
    pub fn new() -> Self {
        Self::build(false)
    }

    /// A self-animating loading scene for the `debug_loading` inspection scene.
    pub fn new_demo() -> Self {
        Self::build(true)
    }

    fn build(demo: bool) -> Self {
        // Mirror `MainMenuScene`'s minimal world so the transition machinery
        // (`switch_mission` -> `to_save_data` on the outgoing scene) has the uniques
        // it expects.
        let mut world = World::new();
        let player_entity = world.add_entity(());
        let inventory_entity = PlayerInventoryEntity::create(&mut world);
        PlayerInventoryEntity::set_position_rotation(
            &mut world,
            vec3(0.0, -1000.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory_entity,
        });
        world.add_unique(QuestInfo::new());
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        world.add_unique(GlobalEntityMetadata(HashMap::new()));
        world.add_unique(Time::default());

        Self {
            world,
            scene_name: "loading".to_owned(),
            progress: 0.0,
            elapsed_secs: 0.0,
            demo,
        }
    }

    /// Set the load progress (0..=1). Used by the background loader in PR 3.
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// The `meters/LOADA_NN.PCX` frame name for the current rotation phase.
    fn current_disc_frame(&self) -> String {
        let frame = ((self.elapsed_secs * LOADA_FPS) as u32 % LOADA_FRAME_COUNT) + 1;
        format!("meters/LOADA_{frame:02}.PCX")
    }
}

impl Default for LoadingScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for LoadingScene {
    fn update(
        &mut self,
        time: &Time,
        _input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        self.elapsed_secs += time.elapsed.as_secs_f32();
        if self.demo {
            self.progress = (self.elapsed_secs / DEMO_FILL_SECS).rem_euclid(1.0);
        }

        Vec::new()
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Drawn in screen space in `render_per_eye`; the 3D scene is empty.
        (
            Vec::new(),
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        )
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        _view: cgmath::Matrix4<f32>,
        _projection: cgmath::Matrix4<f32>,
        screen_size: Vector2<f32>,
        _options: &GameOptions,
    ) -> Vec<SceneObject> {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));

        // 1. Full-screen backdrop.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        // 2. Rotating center disc (cycled LOADA frames).
        let disc_frame = self.current_disc_frame();
        canvas.image(
            Rect::new(
                DISC_CENTER_X - DISC_SIZE / 2.0,
                DISC_CENTER_Y - DISC_SIZE / 2.0,
                DISC_SIZE,
                DISC_SIZE,
            ),
            &disc_frame,
        );

        // 3. Progress bar fill (clipped 0..1).
        canvas.bar(
            Rect::new(BAR_X, BAR_Y, BAR_W, BAR_H),
            PROGRESS_TEXTURE,
            self.progress,
        );

        canvas.render_screen_space(asset_cache, screen_size, SCALE_MODE)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.scene_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_progress_clamps() {
        let mut scene = LoadingScene::new();
        scene.set_progress(1.5);
        assert_eq!(scene.progress, 1.0);
        scene.set_progress(-0.2);
        assert_eq!(scene.progress, 0.0);
    }

    #[test]
    fn disc_frame_cycles_through_all_20() {
        let mut scene = LoadingScene::new();
        // frame 1 at t=0
        scene.elapsed_secs = 0.0;
        assert_eq!(scene.current_disc_frame(), "meters/LOADA_01.PCX");
        // frame 2 after one frame-period
        scene.elapsed_secs = 1.0 / LOADA_FPS;
        assert_eq!(scene.current_disc_frame(), "meters/LOADA_02.PCX");
        // wraps back to frame 1 after a full cycle
        scene.elapsed_secs = LOADA_FRAME_COUNT as f32 / LOADA_FPS;
        assert_eq!(scene.current_disc_frame(), "meters/LOADA_01.PCX");
    }

    #[test]
    fn world_supports_transition_save_data() {
        let scene = LoadingScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
