use std::{collections::HashMap, rc::Rc, time::Duration};

use cgmath::{InnerSpace, Matrix3, Matrix4, Quaternion, Vector3, vec3};
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
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::Effect,
    time::Time,
};

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
    total_time: Duration,
    #[cfg(feature = "ffmpeg")]
    video_player: VideoPlayer,
}

impl CutscenePlayerScene {
    pub fn new(
        video_name: String,
        video_path: String,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let world = Self::initialize_world();

        #[cfg(feature = "ffmpeg")]
        {
            use engine::audio::{AudioHandle, test_audio};

            let video_player = VideoPlayer::from_filename(&video_path)?;
            let audio_clip = Rc::new(AudioPlayer::from_filename(&video_path)?);
            test_audio(audio_context, AudioHandle::new(), None, audio_clip);

            return Ok(Self {
                world,
                head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                player_position: vec3(0.0, 0.0, 0.0),
                player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                head_height: 4.0 / dark::SCALE_FACTOR,
                screen_distance: 6.0 / dark::SCALE_FACTOR,
                screen_vertical_offset: 1.5 / dark::SCALE_FACTOR,
                video_name,
                total_time: Duration::ZERO,
                video_player,
            });
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            let _ = audio_context;
            let _ = video_path;
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

    fn build_screen_object(&self) -> SceneObject {
        let (texture, aspect_ratio) = self.build_video_texture();
        let material = basic_material::create(texture, 1.0, 0.0);
        let mut quad = SceneObject::new(material, Box::new(engine::scene::quad::create()));

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
        let rotation_matrix = Matrix3::from_cols(right, true_up, -look_dir);

        let screen_height = 2.0 / dark::SCALE_FACTOR;
        let screen_width = screen_height * aspect_ratio;

        let transform = Matrix4::from_translation(screen_position)
            * Matrix4::from(rotation_matrix)
            * Matrix4::from_nonuniform_scale(screen_width, screen_height, 1.0);
        quad.set_transform(transform);
        quad
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
                    &TextureOptions { wrap: false },
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
                    &TextureOptions { wrap: false },
                )),
                aspect_ratio,
            )
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
        self.total_time += time.elapsed;
        self.update_player_info();

        #[cfg(feature = "ffmpeg")]
        {
            self.video_player.advance_by_time(time.elapsed);
        }

        Vec::new()
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let screen = self.build_screen_object();
        (vec![screen], self.player_position, self.player_rotation)
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
