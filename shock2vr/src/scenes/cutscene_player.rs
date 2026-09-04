use std::{collections::HashMap, rc::Rc};

#[cfg(not(feature = "ffmpeg"))]
use std::time::Duration;

use cgmath::{Matrix3, Matrix4, Quaternion, Rotation, Vector2, Vector3, vec2, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, basic_material, light::SpotLight},
    texture::{TextureOptions, TextureTrait, init_from_memory2},
};
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    GameOptions, PresentationMode,
    game_scene::GameScene,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalContext, GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{
        FRONTEND_PANEL_SIZE, FrontendPanelAnchor, HAlign, Rect, UiCanvas, VAlign,
        VR_COMPONENT_Z_STEP, WorldPanel, frontend_panel_distance,
    },
};

use super::cutscene_skip::{RingArt, SkipHold};

#[cfg(feature = "ffmpeg")]
use engine_ffmpeg::{AudioPlayer, VideoPlayer};

#[cfg(not(feature = "ffmpeg"))]
use engine::texture_format::{PixelFormat, RawTextureData};

/// Displays a flat panel in front of the player and plays back a video file.
pub struct CutscenePlayerScene {
    world: World,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    /// Where the screen hangs in VR: the same head-anchored placement the
    /// frontend panels use - placed once from the head's yaw, world-locked
    /// afterward, lazily recentered when the player turns away and stays away.
    panel_anchor: FrontendPanelAnchor,
    /// The live head pose, which is what flatscreen hangs the screen off (see
    /// [`Self::screen_panel`]).
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
    video_name: String,
    /// Only the stub has no decoder to ask how far playback has got.
    #[cfg(not(feature = "ffmpeg"))]
    total_time: Duration,
    /// Dispatched once, when playback ends. Without it a finished cutscene
    /// would hold its last frame forever.
    on_complete: GlobalEffect,
    completion_emitted: bool,
    /// Holding either trigger finishes the cutscene early.
    skip_hold: SkipHold,
    /// The generated progress-ring art, cached by the fill step it was built
    /// for, so a held trigger does not re-upload it every frame.
    ring_art: RingArt,
    #[cfg(feature = "ffmpeg")]
    audio_handle: engine::audio::AudioHandle,
    #[cfg(feature = "ffmpeg")]
    video_player: VideoPlayer,
}

/// Where the video screen hangs: the panel it is drawn on, and the height the
/// video occupies inside it once letterboxed.
struct ScreenBasis {
    panel: WorldPanel,
    height: f32,
}

impl ScreenBasis {
    /// Orientation for a raw quad drawn on the screen. Decoded frames are
    /// top-row-first while a quad maps v=0 to its bottom edge, and the panel
    /// basis faces the quad's back toward the player - so the quad is turned
    /// half a turn about its own x axis, which corrects both at once. The
    /// generated ring art shares the row order, so it shares this basis.
    fn billboard_rotation(&self) -> Matrix3<f32> {
        Matrix3::from(self.panel.rotation) * Matrix3::from_angle_x(cgmath::Deg(180.0))
    }

    /// In-plane up: the panel's own, since a placement is gravity-aligned.
    fn up(&self) -> Vector3<f32> {
        self.panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0))
    }

    /// Off the screen plane toward the viewer, so an overlay never z-fights the
    /// video it sits on.
    fn toward_viewer(&self, fraction_of_height: f32) -> Vector3<f32> {
        self.panel.normal() * (self.height * fraction_of_height)
    }
}

/// The screen the video is drawn on, fitted inside `panel`.
fn screen_basis(panel: WorldPanel, aspect_ratio: f32) -> ScreenBasis {
    let height = fitted_screen_height(panel.size, aspect_ratio);
    ScreenBasis { panel, height }
}

/// A panel hung squarely in front of a head pose, pitch included.
///
/// Flatscreen's counterpart to the VR anchor: there is no tracked head there,
/// only the mouse-look camera, so a world-locked screen would slide out of
/// frame on the first look and (below the anchor's recenter threshold) never
/// come back. Keeping it on the view is what "full-screen" means on a monitor.
fn head_locked_panel(head_position: Vector3<f32>, head_rotation: Quaternion<f32>) -> WorldPanel {
    let forward = head_rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
    WorldPanel {
        center: head_position + forward * frontend_panel_distance(),
        rotation: crate::util::get_rotation_from_forward_vector(-forward),
        size: FRONTEND_PANEL_SIZE,
    }
}

/// The video's height once fitted inside the panel's rect, letterboxed rather
/// than cropped or stretched: wide videos are limited by the panel's width,
/// tall ones by its height.
fn fitted_screen_height(panel_size: Vector2<f32>, aspect_ratio: f32) -> f32 {
    (panel_size.x / aspect_ratio.max(1e-3)).min(panel_size.y)
}

/// The skip ring's diameter and its center height, both as a fraction of the
/// video screen's height (center height is signed, up from the screen center).
const RING_SIZE: f32 = 0.18;
const RING_CENTER_HEIGHT: f32 = -0.28;
/// The caption's panel: size and center height, in the same units.
const HINT_WIDTH: f32 = 0.6;
const HINT_HEIGHT: f32 = 0.075;
const HINT_CENTER_HEIGHT: f32 = -0.42;
/// How far the affordance floats off the screen plane, in the same units.
const OVERLAY_DEPTH: f32 = 0.01;
/// Canvas the caption is authored on - only its aspect matters, since the panel
/// scales it to `HINT_WIDTH` x `HINT_HEIGHT`.
const HINT_CANVAS: Vector2<f32> = Vector2 { x: 320.0, y: 40.0 };
const HINT_FONT: &str = "metafont.fon";

/// How long the non-ffmpeg stub shows its placeholder before completing. There
/// is no decoder to ask, so a build without video still has to move on rather
/// than sit on a blank panel forever.
#[cfg(not(feature = "ffmpeg"))]
const STUB_PLAYBACK_DURATION: Duration = Duration::from_secs(2);

impl CutscenePlayerScene {
    /// `video_name` is a cutscene name, resolved to a file here so every caller
    /// selects the same one (see [`super::resolve_cutscene_path`]). The error
    /// names both, so a caller only has to report it.
    pub fn new(
        video_name: String,
        on_complete: GlobalEffect,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let world = Self::initialize_world();

        #[cfg(feature = "ffmpeg")]
        {
            use engine::audio::{AudioHandle, play_streaming_audio};

            let video_path = super::resolve_cutscene_path(&video_name)
                .to_string_lossy()
                .into_owned();
            let describe = |error: &dyn std::fmt::Display| {
                format!("cutscene '{video_name}' could not be opened from '{video_path}': {error}")
            };
            let video_player =
                VideoPlayer::from_filename(&video_path).map_err(|error| describe(&error))?;
            // Streamed rather than decoded into a clip: a feature-length
            // cutscene is minutes of PCM, and waiting for all of it here would
            // stall the first frame and hold tens of MiB for the whole scene.
            let audio_stream =
                AudioPlayer::open_stream(&video_path).map_err(|error| describe(&error))?;
            let audio_handle = AudioHandle::new();
            play_streaming_audio(audio_context, audio_handle.clone(), audio_stream);

            return Ok(Self {
                world,
                player_position: vec3(0.0, 0.0, 0.0),
                player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                panel_anchor: FrontendPanelAnchor::new(),
                head_position: vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
                head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                video_name,
                on_complete,
                completion_emitted: false,
                skip_hold: SkipHold::default(),
                ring_art: RingArt::default(),
                audio_handle,
                video_player,
            });
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            let _ = audio_context;
            Ok(Self {
                world,
                player_position: vec3(0.0, 0.0, 0.0),
                player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                panel_anchor: FrontendPanelAnchor::new(),
                head_position: vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
                head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                video_name,
                total_time: Duration::ZERO,
                on_complete,
                completion_emitted: false,
                skip_hold: SkipHold::default(),
                ring_art: RingArt::default(),
            })
        }
    }

    fn initialize_world() -> World {
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

        world
    }

    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    /// The panel the video screen is drawn on, per presentation.
    ///
    /// In VR that is the frontend panels' own placement - gravity-aligned,
    /// world-locked, lazily recentered - so a cutscene hangs where a menu
    /// would instead of riding the gaze (which tilts with every glance and
    /// cannot be looked at). Flatscreen keeps the screen on the view; see
    /// [`head_locked_panel`].
    fn screen_panel(&self, presentation: PresentationMode) -> WorldPanel {
        match presentation {
            PresentationMode::Vr => self.panel_anchor.panel(),
            PresentationMode::Flat => head_locked_panel(self.head_position, self.head_rotation),
        }
    }

    fn build_screen_object(
        &self,
        basis: &ScreenBasis,
        texture: Rc<dyn TextureTrait>,
        aspect_ratio: f32,
    ) -> SceneObject {
        let material = basic_material::create(texture, 1.0, 0.0);
        let mut quad = SceneObject::new(material, Box::new(engine::scene::quad::create()));

        let screen_height = basis.height;
        let screen_width = screen_height * aspect_ratio;

        let transform = Matrix4::from_translation(basis.panel.center)
            * Matrix4::from(basis.billboard_rotation())
            * Matrix4::from_nonuniform_scale(screen_width, screen_height, 1.0);
        quad.set_transform(transform);
        quad
    }

    /// The radial hold-to-skip progress ring, drawn on the video screen while
    /// the trigger is touched. The ring is the shared hold readout; only where
    /// it hangs is the cutscene's own.
    fn build_skip_ring(&mut self, basis: &ScreenBasis, alpha: f32) -> SceneObject {
        let mut quad = self.ring_art.quad(self.skip_hold.progress(), alpha);

        let size = basis.height * RING_SIZE;
        let center = basis.panel.center
            + basis.up() * (RING_CENTER_HEIGHT * basis.height)
            + basis.toward_viewer(OVERLAY_DEPTH);

        quad.set_transform(
            Matrix4::from_translation(center)
                * Matrix4::from(basis.billboard_rotation())
                * Matrix4::from_nonuniform_scale(size, size, 1.0),
        );
        quad
    }

    /// The "hold to skip" caption under the ring, on its own world panel so the
    /// text goes through the shared UI path rather than a bespoke one.
    fn build_skip_hint(
        &self,
        asset_cache: &mut AssetCache,
        basis: &ScreenBasis,
        alpha: f32,
    ) -> Vec<SceneObject> {
        let panel = WorldPanel {
            center: basis.panel.center
                + basis.up() * (HINT_CENTER_HEIGHT * basis.height)
                + basis.toward_viewer(OVERLAY_DEPTH),
            rotation: basis.panel.rotation,
            size: vec2(basis.height * HINT_WIDTH, basis.height * HINT_HEIGHT),
        };

        let mut canvas = UiCanvas::new(HINT_CANVAS);
        canvas.text_native(
            Rect::new(0.0, 0.0, HINT_CANVAS.x, HINT_CANVAS.y),
            "HOLD TO SKIP",
            HINT_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
        canvas.render_world_space(
            asset_cache,
            panel.transform(),
            None,
            Some(alpha),
            VR_COMPONENT_Z_STEP,
        )
    }

    fn build_video_texture(&self) -> (Rc<dyn TextureTrait>, f32) {
        #[cfg(feature = "ffmpeg")]
        {
            let texture_data = self.video_player.get_current_frame();
            let aspect_ratio = if texture_data.height == 0 {
                16.0 / 9.0
            } else {
                texture_data.width as f32 / texture_data.height as f32
            };
            (
                Rc::new(init_from_memory2(
                    texture_data,
                    &TextureOptions {
                        wrap: false,
                        ..Default::default()
                    },
                )),
                aspect_ratio,
            )
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            let white_pixel = vec![255u8, 255u8, 255u8, 255u8];
            let aspect_ratio = 16.0 / 9.0;
            let texture_data = RawTextureData {
                width: 1,
                height: 1,
                bytes: white_pixel,
                format: PixelFormat::RGBA,
            };
            (
                Rc::new(init_from_memory2(
                    texture_data,
                    &TextureOptions {
                        wrap: false,
                        ..Default::default()
                    },
                )),
                aspect_ratio,
            )
        }
    }

    fn playback_is_finished(&self) -> bool {
        #[cfg(feature = "ffmpeg")]
        {
            self.video_player.is_finished()
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            self.total_time >= STUB_PLAYBACK_DURATION
        }
    }
}

impl GameScene for CutscenePlayerScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = command_effects;

        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        self.head_position = input_context.head.position;
        self.head_rotation = input_context.head.rotation;
        self.panel_anchor.update(
            input_context.head.position,
            input_context.head.rotation,
            time.elapsed,
        );
        self.player_position = vec3(0.0, 0.0, 0.0);
        self.player_rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        self.update_player_info();

        #[cfg(feature = "ffmpeg")]
        {
            self.video_player.advance_by_time(time.elapsed);
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            self.total_time += time.elapsed;
        }

        // Contextual, not an `InputAction`: the hold needs the analog value, and
        // either hand can give it.
        let trigger = input_context
            .left_hand
            .trigger_value
            .max(input_context.right_hand.trigger_value);
        let skipped = self.skip_hold.update(trigger, time.elapsed);

        // Latched because not every `on_complete` replaces this scene - a
        // follow-on like `Quit` leaves it running, and re-emitting would then
        // repeat the effect every frame. A skip finishes the cutscene through
        // this same path, so the follow-on (and the audio stop in `on_exit`)
        // cannot differ between ending early and playing out.
        if !self.completion_emitted && (skipped || self.playback_is_finished()) {
            self.completion_emitted = true;
            return vec![Effect::GlobalEffect(self.on_complete.clone())];
        }

        Vec::new()
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // One world-space list for both presentations: the affordance is
        // anchored to the video screen, so flatscreen and VR place it alike.
        let (texture, aspect_ratio) = self.build_video_texture();
        let basis = screen_basis(self.screen_panel(options.presentation_mode), aspect_ratio);
        let mut objects = vec![self.build_screen_object(&basis, texture, aspect_ratio)];
        let alpha = self.skip_hold.alpha();
        if alpha > 0.01 {
            objects.push(self.build_skip_ring(&basis, alpha));
            objects.extend(self.build_skip_hint(asset_cache, &basis, alpha));
        }
        (objects, self.player_position, self.player_rotation)
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
            .filter_map(|effect| match effect {
                Effect::GlobalEffect(global) => Some(global),
                _ => None,
            })
            .collect()
    }

    fn on_exit(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        // The soundtrack is one clip for the whole video; without this it keeps
        // playing over whatever scene follows until it drains.
        #[cfg(feature = "ffmpeg")]
        engine::audio::stop_audio(audio_context, self.audio_handle.clone());

        #[cfg(not(feature = "ffmpeg"))]
        let _ = audio_context;
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.video_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_context::DEFAULT_HEAD_HEIGHT;
    use cgmath::{Deg, InnerSpace, Rotation3};
    use std::time::Duration;

    fn eye() -> Vector3<f32> {
        vec3(0.0, DEFAULT_HEAD_HEIGHT, 0.0)
    }

    /// A head pitched at the floor: VR hangs the screen upright at eye level
    /// (the frontend placement), while flatscreen keeps it squarely on the
    /// view - a world-locked screen there would slide out of frame on the
    /// first mouse-look and never come back.
    #[test]
    fn vr_hangs_the_screen_upright_while_flat_keeps_it_on_the_view() {
        let pitched = Quaternion::from_angle_y(Deg(90.0)) * Quaternion::from_angle_x(Deg(-70.0));

        let mut anchor = FrontendPanelAnchor::new();
        let vr = anchor.update(eye(), pitched, Duration::from_millis(16));
        assert!(
            (vr.center.y - eye().y).abs() < 1e-4,
            "VR screen should hang at eye level, got {:?}",
            vr.center
        );
        let vr_up = vr.rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
        assert!(
            vr_up.dot(vec3(0.0, 1.0, 0.0)) > 0.999,
            "VR screen is upright"
        );

        let flat = head_locked_panel(eye(), pitched);
        let gaze = pitched.rotate_vector(vec3(0.0, 0.0, -1.0));
        let to_screen = (flat.center - eye()).normalize();
        assert!(
            to_screen.dot(gaze) > 0.999,
            "flat screen should sit on the gaze, got {to_screen:?} vs {gaze:?}"
        );
        assert!(
            flat.normal().dot(-gaze) > 0.999,
            "flat screen faces the eye"
        );
    }

    /// A turned head leaves the VR screen where it was placed (it is
    /// world-locked, like a menu) - the property the whole anchor exists for.
    #[test]
    fn turning_the_head_does_not_drag_the_vr_screen_along() {
        let mut anchor = FrontendPanelAnchor::new();
        let placed = anchor.update(
            eye(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            Duration::from_millis(16),
        );
        let after_glance = anchor.update(
            eye(),
            Quaternion::from_angle_y(Deg(30.0)),
            Duration::from_millis(16),
        );
        assert!((after_glance.center - placed.center).magnitude() < 1e-4);
    }

    /// The quad's basis flips the decoded frame's row order (top-row-first art
    /// on a quad whose v=0 is its bottom edge) without mirroring it sideways.
    #[test]
    fn the_billboard_basis_flips_rows_and_nothing_else() {
        let basis = screen_basis(
            head_locked_panel(eye(), Quaternion::new(1.0, 0.0, 0.0, 0.0)),
            16.0 / 9.0,
        );
        let quad = basis.billboard_rotation();
        let right = basis.panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0));
        assert!(quad.x.dot(right) > 0.999, "not mirrored sideways");
        assert!(quad.y.dot(basis.up()) < -0.999, "rows are flipped");
    }

    /// The video is letterboxed into the frontend panel's rect: a wide clip is
    /// bounded by the panel's width, a tall one by its height. Getting this
    /// backwards would hang a screen wider or taller than the menus.
    #[test]
    fn the_video_is_fitted_inside_the_panel_rect() {
        let panel = FRONTEND_PANEL_SIZE;

        let wide = fitted_screen_height(panel, 16.0 / 9.0);
        assert!((wide - panel.x / (16.0 / 9.0)).abs() < 1e-4);
        assert!(wide <= panel.y);

        let tall = fitted_screen_height(panel, 0.5);
        assert!((tall - panel.y).abs() < 1e-4);
        assert!(tall * 0.5 <= panel.x);
    }
}
