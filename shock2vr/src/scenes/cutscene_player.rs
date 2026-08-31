use std::{collections::HashMap, rc::Rc};

#[cfg(not(feature = "ffmpeg"))]
use std::time::Duration;

use cgmath::{InnerSpace, Matrix3, Matrix4, Quaternion, Vector2, Vector3, vec2, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, basic_material, light::SpotLight},
    texture::{TextureOptions, TextureTrait, init_from_memory2},
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
    ui::{HAlign, Rect, UiCanvas, VAlign, VR_COMPONENT_Z_STEP, WorldPanel},
};

use super::cutscene_skip::{self, SkipHold};

#[cfg(feature = "ffmpeg")]
use engine_ffmpeg::{AudioPlayer, VideoPlayer};

#[cfg(not(feature = "ffmpeg"))]
use engine::texture_format::{PixelFormat, RawTextureData};

/// Displays a flat panel in front of the player and plays back a video file.
pub struct CutscenePlayerScene {
    world: World,
    head_rotation: Quaternion<f32>,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_height: f32,
    screen_distance: f32,
    screen_vertical_offset: f32,
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
    /// The generated progress-ring art, keyed on the fill step it was built
    /// for, so a held trigger does not re-upload it every frame.
    ring_art: Option<(u32, Rc<dyn TextureTrait>)>,
    #[cfg(feature = "ffmpeg")]
    audio_handle: engine::audio::AudioHandle,
    #[cfg(feature = "ffmpeg")]
    video_player: VideoPlayer,
}

/// Where the video screen hangs, and the viewer-facing basis it is built on.
struct ScreenBasis {
    center: Vector3<f32>,
    /// In-plane horizontal axis. It points to the viewer's *left*: the
    /// billboard basis below is mirrored, and this is the axis that mirroring
    /// is built from - it is not a "right" to offset something sideways by.
    screen_x: Vector3<f32>,
    up: Vector3<f32>,
    /// From the screen back toward the viewer.
    look_dir: Vector3<f32>,
    height: f32,
}

impl ScreenBasis {
    /// Orientation for a raw quad drawn on the screen. Decoded frames are
    /// top-row-first while a quad maps v=0 to its bottom edge, and the
    /// billboard basis faces the quad's back toward the player - so the quad
    /// is turned half a turn in its own plane, which corrects both at once.
    /// The generated ring art shares the row order, so it shares this basis.
    fn billboard_rotation(&self) -> Matrix3<f32> {
        Matrix3::from_cols(self.screen_x, self.up, -self.look_dir)
            * Matrix3::from_angle_z(cgmath::Deg(180.0))
    }

    /// Orientation for a [`WorldPanel`] on the screen: the honest basis the
    /// canvas path expects (+x the viewer's right, +y up, +z back at the
    /// viewer), which is what facing the viewer means.
    fn panel_rotation(&self) -> Quaternion<f32> {
        crate::util::get_rotation_from_forward_vector(self.look_dir)
    }

    /// Off the screen plane toward the viewer, so an overlay never z-fights the
    /// video it sits on.
    fn toward_viewer(&self, fraction_of_height: f32) -> Vector3<f32> {
        self.look_dir * (self.height * fraction_of_height)
    }
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
                head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                player_position: vec3(0.0, 0.0, 0.0),
                player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                head_height: 4.0 / dark::SCALE_FACTOR,
                screen_distance: 6.0 / dark::SCALE_FACTOR,
                screen_vertical_offset: 1.5 / dark::SCALE_FACTOR,
                video_name,
                on_complete,
                completion_emitted: false,
                skip_hold: SkipHold::default(),
                ring_art: None,
                audio_handle,
                video_player,
            });
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            let _ = audio_context;
            Ok(Self {
                world,
                head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                player_position: vec3(0.0, 0.0, 0.0),
                player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                head_height: 4.0 / dark::SCALE_FACTOR,
                screen_distance: 6.0 / dark::SCALE_FACTOR,
                screen_vertical_offset: 1.5 / dark::SCALE_FACTOR,
                video_name,
                total_time: Duration::ZERO,
                on_complete,
                completion_emitted: false,
                skip_hold: SkipHold::default(),
                ring_art: None,
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

    fn head_base(&self) -> Vector3<f32> {
        self.player_position + vec3(0.0, self.head_height, 0.0)
    }

    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    /// Where the video screen hangs and how it is oriented. The skip affordance
    /// is anchored to the same basis, so it rides the screen in flatscreen and
    /// in VR alike rather than being placed twice.
    fn screen_basis(&self) -> ScreenBasis {
        let forward = self.head_rotation * vec3(0.0, 0.0, -1.0);
        let base = self.head_base();
        let mut screen_position = base + forward * self.screen_distance;
        screen_position.y += self.screen_vertical_offset;

        let mut look_dir = base - screen_position;
        if look_dir.magnitude2() < 1e-6 {
            look_dir = vec3(0.0, 0.0, 1.0);
        } else {
            look_dir = look_dir.normalize();
        }

        let mut up = vec3(0.0, 1.0, 0.0);
        let mut right = look_dir.cross(up);
        if right.magnitude2() < 1e-6 {
            up = vec3(0.0, 0.0, 1.0);
            right = look_dir.cross(up);
        }
        right = right.normalize();
        let true_up = right.cross(look_dir).normalize();

        ScreenBasis {
            center: screen_position,
            screen_x: right,
            up: true_up,
            look_dir,
            height: 2.0 / dark::SCALE_FACTOR,
        }
    }

    fn build_screen_object(&self, basis: &ScreenBasis) -> SceneObject {
        let (texture, aspect_ratio) = self.build_video_texture();
        let material = basic_material::create(texture, 1.0, 0.0);
        let mut quad = SceneObject::new(material, Box::new(engine::scene::quad::create()));

        let screen_height = basis.height;
        let screen_width = screen_height * aspect_ratio;

        let transform = Matrix4::from_translation(basis.center)
            * Matrix4::from(basis.billboard_rotation())
            * Matrix4::from_nonuniform_scale(screen_width, screen_height, 1.0);
        quad.set_transform(transform);
        quad
    }

    /// The radial hold-to-skip progress ring, drawn on the video screen while
    /// the trigger is touched.
    fn build_skip_ring(&mut self, basis: &ScreenBasis, alpha: f32) -> SceneObject {
        // The art changes only when the fill crosses a step, so a held trigger
        // rasterizes and uploads a few dozen textures rather than one a frame.
        let step = cutscene_skip::ring_step(self.skip_hold.progress());
        let texture = match &self.ring_art {
            Some((cached_step, texture)) if *cached_step == step => texture.clone(),
            _ => {
                let texture: Rc<dyn TextureTrait> = Rc::new(init_from_memory2(
                    cutscene_skip::ring_texture_for_step(step),
                    &TextureOptions {
                        wrap: false,
                        ..Default::default()
                    },
                ));
                self.ring_art = Some((step, texture.clone()));
                texture
            }
        };

        // Held just under fully opaque: the material only joins the blended
        // pass when it is transparent at all, and the ring is nothing but
        // per-pixel alpha.
        let transparency = (1.0 - alpha).max(0.02);
        let material = basic_material::create(texture, 1.0, transparency);
        let mut quad = SceneObject::new(material, Box::new(engine::scene::quad::create()));

        let size = basis.height * RING_SIZE;
        let center = basis.center
            + basis.up * (RING_CENTER_HEIGHT * basis.height)
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
            center: basis.center
                + basis.up * (HINT_CENTER_HEIGHT * basis.height)
                + basis.toward_viewer(OVERLAY_DEPTH),
            rotation: basis.panel_rotation(),
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

        self.head_rotation = input_context.head.rotation;
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
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // One world-space list for both presentations: the affordance is
        // anchored to the video screen, so flatscreen and VR place it alike.
        let basis = self.screen_basis();
        let mut objects = vec![self.build_screen_object(&basis)];
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
