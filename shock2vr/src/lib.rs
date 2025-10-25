pub mod command;
pub mod game_scene;
pub mod input_context;
pub mod inventory;
pub mod save_load;
pub mod teleport;
pub mod time;

mod creature;
mod gui;
mod hud;
mod mission;
mod physics;
mod quest_info;
mod runtime_props;
mod scripts;
mod systems;
mod util;
mod virtual_hand;
mod vr_config;
pub mod zip_asset_path;

pub use mission::visibility_engine::CullingInfo;
pub use mission::SpawnLocation;

use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::BufReader,
    rc::Rc,
};

use cgmath::{vec3, InnerSpace, Matrix4, Quaternion, Vector2, Vector3};
use command::Command;
use dark::{
    gamesys,
    importers::{AUDIO_IMPORTER, FONT_IMPORTER, STRINGS_IMPORTER},
    motion::MotionDB,
    properties::{AmbientSoundFlags, PropAmbientHacked, PropPosition},
};
use engine::{
    assets::{asset_cache::AssetCache, asset_paths::AssetPath},
    audio::{AudioClip, AudioContext},
    file_system::FileSystem,
    game_log,
    scene::SceneObject,
};

use mission::entity_populator::{EntityPopulator, MissionEntityPopulator, SaveFileEntityPopulator};
use quest_info::QuestInfo;

use save_load::{EntitySaveData, GlobalData, HeldItemSaveData, SaveData};
use scripts::GlobalEffect;
use shipyard::*;
use shipyard::{self, View};
use time::Time;
use tracing::{info, span, trace, warn, Level};

use zip_asset_path::ZipAssetPath;

use crate::{
    game_scene::GameScene,
    mission::{GlobalContext, Mission, PlayerInfo},
    scripts::Effect,
};

#[cfg(target_os = "android")]
const BASE_PATH: &str = "/mnt/sdcard/shock2quest";

#[cfg(not(target_os = "android"))]
const BASE_PATH: &str = "../../Data";

pub fn resource_path(str: &str) -> String {
    format!("{BASE_PATH}/{str}")
}

pub struct GameOptions {
    pub mission: String,
    pub spawn_location: SpawnLocation,
    pub save_file: Option<String>,
    pub render_particles: bool,
    pub debug_physics: bool,
    pub debug_draw: bool,
    pub debug_portals: bool,
    pub debug_show_ids: bool,
    pub experimental_features: HashSet<String>,
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {
            mission: "earth.mis".to_owned(),
            spawn_location: SpawnLocation::MapDefault,
            save_file: None,
            debug_draw: false,
            debug_portals: false,
            debug_physics: false,
            debug_show_ids: false,
            render_particles: true,
            experimental_features: HashSet::new(),
        }
    }
}

pub struct Game {
    options: GameOptions,
    pub asset_cache: AssetCache,
    global_context: GlobalContext,
    active_game_scene: Box<dyn GameScene>,
    // physics: PhysicsWorld,
    // script_world: ScriptWorld,
    audio_context: AudioContext<EntityId, String>,
    // id_to_scene_objects: HashMap<EntityId, Vec<RefCell<SceneObject>>>,
    // id_to_physics: HashMap<EntityId, RigidBodyHandle>,
    // scene_objects: Vec<RefCell<SceneObject>>,
    //world: World,
    last_music_cue: Option<String>,
    last_env_sound: Option<String>,

    mission_to_save_data: HashMap<String, EntitySaveData>,
}

impl Game {
    fn switch_mission(&mut self, level_name: String, spawn_loc: SpawnLocation) {
        let current_quest_info = self
            .active_game_scene
            .world()
            .borrow::<UniqueView<QuestInfo>>()
            .unwrap()
            .clone();

        let (current_save_data, held_data) =
            save_load::to_save_data(self.active_game_scene.world());
        game_log!(
            DEBUG,
            "Saving {} entities to save data",
            current_save_data.all_entities.len()
        );

        self.mission_to_save_data.insert(
            self.active_game_scene.scene_name().to_ascii_lowercase(),
            current_save_data,
        );

        let populator: Box<dyn EntityPopulator> = {
            if let Some(save_data) = self
                .mission_to_save_data
                .get(&level_name.to_ascii_lowercase())
            {
                let save_data_cloned = save_data.clone();
                let populator = SaveFileEntityPopulator::create(save_data_cloned);
                Box::new(populator)
            } else {
                Box::new(MissionEntityPopulator::create())
            }
        };

        let active_mission = Mission::load(
            level_name,
            &mut self.asset_cache,
            &mut self.audio_context,
            &self.global_context,
            spawn_loc,
            current_quest_info,
            populator,
            held_data,
            &self.options,
        );
        self.active_game_scene = Box::new(active_mission);
    }
    pub fn init(_file_system: &dyn FileSystem, options: GameOptions) -> Game {
        let asset_paths = AssetPath::combine(vec![
            AssetPath::folder(resource_path("res/mesh")),
            // AssetPath::folder(resource_path("res/mesh/txt16")),
            AssetPath::folder(resource_path("res/obj")),
            // AssetPath::folder(resource_path("res/obj/txt16")),
            ZipAssetPath::new(resource_path("res/obj.crf")),
            ZipAssetPath::new(resource_path("res/bitmap.crf")),
            ZipAssetPath::new(resource_path("res/fam.crf")),
            ZipAssetPath::new(resource_path("res/iface.crf")),
            ZipAssetPath::new(resource_path("res/mesh.crf")),
            ZipAssetPath::new(resource_path("res/motions.crf")),
            ZipAssetPath::new(resource_path("res/objicon.crf")),
            ZipAssetPath::new(resource_path("res/snd.crf")),
            ZipAssetPath::new(resource_path("res/snd2.crf")),
            ZipAssetPath::new(resource_path("res/song.crf")),
            ZipAssetPath::new2(resource_path("res/strings.crf"), false),
            //AssetPath::folder("../assets/"),
            // Textures
            // AssetPath::folder("res/bitmap".to_owned()),
            // AssetPath::folder("res/bitmap/txt16".to_owned()),
            //AssetPath::folder("res/fam".to_owned()),
            // Models
            // AssetPath::folder("res/mesh/txt16".to_owned()),
            // AssetPath::folder("res/obj".to_owned()),
            // AssetPath::folder("res/mesh".to_owned()),
            // Animations
            //AssetPath::folder("res/motions".to_owned()),
            // Motion db
            AssetPath::folder("".to_owned()),
            // Audio
            // AssetPath::folder("res/snd".to_owned()),
            // AssetPath::folder("res/snd/amb".to_owned()),
            // AssetPath::folder("res/snd/Assassin".to_owned()),
            // AssetPath::folder("res/snd/BBetty".to_owned()),
            // AssetPath::folder("res/snd/Devices".to_owned()),
            // AssetPath::folder("res/snd/GRUB".to_owned()),
            // AssetPath::folder("res/snd/HITS".to_owned()),
            // AssetPath::folder("res/snd/MaintBot/english".to_owned()),
            // AssetPath::folder("res/snd/Midwife/english".to_owned()),
            // AssetPath::folder("res/snd/OGRUNT/english".to_owned()),
            // AssetPath::folder("res/snd/Overlord/english".to_owned()),
            // AssetPath::folder("res/snd/SONGS".to_owned()),
            // AssetPath::folder("res/snd/TurrCam".to_owned()),
            // AssetPath::folder("res/snd/Weapons".to_owned()),
            // AssetPath::folder("res/snd2/vBriefs/ENGLISH".to_owned()),
            // AssetPath::folder("res/snd2/vCs/ENGLISH".to_owned()),
            // AssetPath::folder("res/snd2/vEmails/english".to_owned()),
            // AssetPath::folder("res/snd2/vLogs/english".to_owned()),
            // AssetPath::folder("res/snd2/vTriggers/english".to_owned()),
        ]);
        // Global items
        let mut asset_cache = AssetCache::new(BASE_PATH.to_owned(), asset_paths);

        // TODO: Start ffmpeg stuff
        #[cfg(feature = "ffmpeg")]
        engine_ffmpeg::init().unwrap();

        let (properties, links, links_with_data) = dark::properties::get();

        let game_file = File::open(resource_path("shock2.gam")).unwrap();
        let mut game_reader = BufReader::new(game_file);

        let _strings = asset_cache.get(&STRINGS_IMPORTER, "objname.str");

        // vhot logging:
        // let atek_file = File::open(resource_path("res/obj/ar15_w.bin")).unwrap();
        // let mut atek_reader = BufReader::new(atek_file);
        // let header = ss2_bin_header::read(&mut atek_reader);
        // let obj = ss2_bin_obj_loader::read(&mut atek_reader, &header);

        let gamesys = gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

        let motiondb_file = File::open(resource_path("motiondb.bin")).unwrap();
        let mut motiondb_reader = BufReader::new(motiondb_file);
        let motiondb = MotionDB::read(&mut motiondb_reader);

        let mut audio_context = AudioContext::new();

        let global_context = GlobalContext {
            links,
            links_with_data,
            properties,
            motiondb,
            gamesys,
        };

        // TEST: Load all missions
        // Load all missions:
        // for _x in 0..100 {
        //     let all_missions = vec![
        //         "earth.mis",
        //         "station.mis",
        //         "medsci1.mis",
        //         "medsci2.mis",
        //         "eng1.mis",
        //         "eng2.mis",
        //         "hydro1.mis",
        //         "hydro2.mis",
        //         "hydro3.mis",
        //         "ops1.mis",
        //         "ops2.mis",
        //         "ops3.mis",
        //         "ops4.mis",
        //         "rec1.mis",
        //         "rec2.mis",
        //         "rec3.mis",
        //         "command1.mis",
        //         "command2.mis",
        //         "rick1.mis",
        //         "rick2.mis",
        //         "rick3.mis",
        //         "rick3.mis",
        //         "many.mis",
        //         "shodan.mis",
        //     ];

        //     for mission in all_missions {
        //         warn!("Loading mission: {}", mission);
        //         let _ = Mission::load(
        //             mission.to_owned(),
        //             &mut asset_cache,
        //             &mut audio_context,
        //             &global_context,
        //             None,
        //             QuestInfo::new(),
        //         );
        //     }
        // }

        let (active_mission, mission_to_save_data) =
            if let Some(save_file_path) = &options.save_file {
                let mut file = OpenOptions::new().read(true).open(save_file_path).unwrap();
                let save_data = SaveData::read(&mut file);
                Self::load_from_save_data(
                    save_data,
                    &mut asset_cache,
                    &mut audio_context,
                    &global_context,
                    &options,
                )
            } else {
                // Level specific items
                let mission_to_save_data = HashMap::new();
                let active_mission = Mission::load(
                    options.mission.to_owned(),
                    //"medsci2.mis".to_owned(),
                    &mut asset_cache,
                    &mut audio_context,
                    &global_context,
                    options.spawn_location.clone(),
                    QuestInfo::new(),
                    //Box::new(MissionEntityPopulator::create()),
                    Box::new(MissionEntityPopulator::create()),
                    HeldItemSaveData::empty(),
                    &options,
                );
                (active_mission, mission_to_save_data)
            };

        // log_entities_with_link(&active_mission.world, |link| {
        //     matches!(link, Link::AIWatchObj(_))
        // });
        // panic!();

        // log_property::<PropSignalType>(&active_mission.world);
        // panic!();

        // log_entity(
        //     &active_mission.world,
        //     **(&active_mission.template_to_entity_id.get(&365).unwrap()),
        // );
        // panic!();

        Game {
            asset_cache,
            audio_context,
            active_game_scene: Box::new(active_mission),
            global_context,
            last_music_cue: None,
            last_env_sound: None,
            options,
            mission_to_save_data,
        }
    }

    pub fn update(
        &mut self,
        time: &Time,
        input_context: &input_context::InputContext,
        commands: Vec<Box<dyn Command>>,
    ) {
        let span = span!(Level::INFO, "update");
        let _enter = span.enter();
        let delta_time = time.elapsed.as_secs_f32();
        trace!("delta_time: {}", delta_time);

        // Process commands into effects
        let mut command_effects = Vec::new();
        for command in commands {
            let eff = command.execute(self.active_game_scene.world());
            command_effects.push(eff);
        }

        // Update the scene (handles movement, physics, collision, teleport internally)
        let effects = self.active_game_scene.update(
            time,
            input_context,
            &mut self.asset_cache,
            &self.options,
            command_effects,
        );

        // Handle ambient sound processing
        let (new_character_pos, next_env_sound, new_cue, potential_ambient_sounds) = {
            let player_info = self.active_game_scene.world().borrow::<UniqueView<PlayerInfo>>().unwrap();
            let current_pos = player_info.pos;

            let mut next_env_sound = None;
            let mut new_cue = None;
            let mut potential_ambient_sounds = Vec::new();

            self.active_game_scene.world().run(
                |v_ambient_hacked: View<PropAmbientHacked>,
                 v_position: View<PropPosition>,
                 v_player_position: UniqueView<PlayerInfo>| {
                    for (id, (ambient_sound, position)) in
                        (&v_ambient_hacked, &v_position).iter().with_id()
                    {
                        let dist_squared = (position.position - v_player_position.pos).magnitude2();

                        if dist_squared < ambient_sound.radius_squared {
                            if ambient_sound.sound_flags.contains(AmbientSoundFlags::MUSIC) {
                                new_cue = Some(ambient_sound.schema.to_owned());
                            } else if ambient_sound
                                .sound_flags
                                .contains(AmbientSoundFlags::ENVIRONMENTAL)
                            {
                                let schema_val = self.resolve_schema(&ambient_sound.schema);
                                next_env_sound = Some(schema_val)
                            } else {
                                potential_ambient_sounds.push((
                                    dist_squared,
                                    position.position,
                                    id,
                                    ambient_sound.schema.to_owned(),
                                ))
                            }
                        }
                    }
                },
            );

            (current_pos, next_env_sound, new_cue, potential_ambient_sounds)
        };

        // Update audio
        if let Some(cue) = new_cue {
            self.update_music_cue_if_necessary(cue);
        }

        if let Some(cue) = next_env_sound {
            self.update_env_sound_if_necessary(cue);
        }

        // Process ambient sounds
        let mut ambient_sounds_sorted = potential_ambient_sounds;
        ambient_sounds_sorted.sort_by(|a, b| a.0.total_cmp(&b.0));

        let ambient_sounds = ambient_sounds_sorted
            .iter()
            .take(8)
            .filter_map(|(_dist, position, id, schema)| {
                let asset_name = self.resolve_schema(schema);
                let maybe_audio_clip = self
                    .asset_cache
                    .get_opt(&AUDIO_IMPORTER, &format!("{asset_name}.wav"));
                maybe_audio_clip.map(|clip| (*id, *position, clip.clone()))
            })
            .collect::<Vec<(EntityId, Vector3<f32>, Rc<AudioClip>)>>();

        self.audio_context.update(new_character_pos, ambient_sounds);

        // Handle global effects
        let global_effects = self.active_game_scene.handle_effects(
            effects,
            &self.global_context,
            &self.options,
            &mut self.asset_cache,
            &mut self.audio_context,
        );

        for effect in global_effects {
            self.handle_global_effect(effect);
        }
    }

    fn save_to_file(&self, file_name: String) {
        let save_data = self.build_save_data();
        let mut zip_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file_name)
            .unwrap();
        save_data.write(&mut zip_file);
    }

    fn load_from_file(&mut self, file_name: String) {
        let mut file = OpenOptions::new().read(true).open(file_name).unwrap();
        let save_data = SaveData::read(&mut file);
        let (mission, level_map) = Self::load_from_save_data(
            save_data,
            &mut self.asset_cache,
            &mut self.audio_context,
            &mut self.global_context,
            &self.options,
        );
        self.active_game_scene = Box::new(mission);
        self.mission_to_save_data = level_map;
    }

    fn load_from_save_data(
        save_data: SaveData,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
    ) -> (Mission, HashMap<String, EntitySaveData>) {
        let current_mission = save_data.global_data.active_mission.clone();
        //self.mission_to_save_data = save_data.level_data;

        let populator: Box<dyn EntityPopulator> = {
            if let Some(save_data) = save_data
                .level_data
                .get(&current_mission.to_ascii_lowercase())
            {
                let save_data_cloned = save_data.clone();
                let populator = SaveFileEntityPopulator::create(save_data_cloned);
                Box::new(populator)
            } else {
                Box::new(MissionEntityPopulator::create())
            }
        };

        let spawn_loc = SpawnLocation::PositionRotation(
            save_data.global_data.position,
            save_data.global_data.rotation,
        );

        let active_mission = Mission::load(
            current_mission,
            asset_cache,
            audio_context,
            global_context,
            spawn_loc,
            save_data.global_data.quest_info,
            populator,
            save_data.global_data.held_items,
            game_options,
        );

        //self.active_mission = active_mission;
        (active_mission, save_data.level_data)
    }

    fn build_save_data(&self) -> SaveData {
        let mut level_data = self.mission_to_save_data.clone();

        let (save_data, held_items) = save_load::to_save_data(self.active_game_scene.world());

        level_data.insert(self.active_game_scene.scene_name().to_string(), save_data);

        let (position, rotation) = {
            let player_info = self
                .active_game_scene
                .world()
                .borrow::<UniqueView<PlayerInfo>>()
                .unwrap();
            (player_info.pos, player_info.rotation)
        };

        let quest_info = self
            .active_game_scene
            .world()
            .borrow::<UniqueView<QuestInfo>>()
            .unwrap()
            .clone();

        let global_data = GlobalData {
            held_items,
            position,
            rotation,
            quest_info,
            active_mission: self.active_game_scene.scene_name().to_string(),
        };

        SaveData {
            global_data,
            level_data,
        }
    }

    fn handle_global_effect(&mut self, global_effect: GlobalEffect) {
        match global_effect {
            GlobalEffect::Save { file_name } => self.save_to_file(file_name),
            GlobalEffect::Load { file_name } => self.load_from_file(file_name),
            GlobalEffect::TransitionLevel { level_file, loc } => {
                let spawn_loc = match loc {
                    None => SpawnLocation::MapDefault,
                    Some(marker) => SpawnLocation::Marker(marker),
                };

                self.switch_mission(level_file, spawn_loc);
            }
            GlobalEffect::TestReload => {
                let (position, rotation) = {
                    let player_info = self
                        .active_game_scene
                        .world()
                        .borrow::<UniqueView<PlayerInfo>>()
                        .unwrap();
                    (player_info.pos, player_info.rotation)
                };
                self.switch_mission(
                    self.active_game_scene.scene_name().to_string(),
                    SpawnLocation::PositionRotation(position, rotation),
                );
            }
        }
    }

    /// Get hand spotlights for enhanced lighting when experimental flag is enabled
    pub fn get_hand_spotlights(&self) -> Vec<engine::scene::light::SpotLight> {
        self.active_game_scene.get_hand_spotlights(&self.options)
    }

    pub fn render(&mut self) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let (scene, pos, rot) = self
            .active_game_scene
            .render(&mut self.asset_cache, &self.options);

        // let font = File::open(resource_path("res/fonts/mainfont.FON")).unwrap();
        // let mut font_reader = BufReader::new(font);
        // let font: Rc<Box<dyn engine::Font>> =
        //     Rc::new(Box::new(dark::font::Font::read(&mut font_reader)));

        // let text = SceneObject::world_space_text("test1234567890", font, 0.0);
        // scene.push(RefCell::new(text));

        (scene, pos, rot)
    }

    pub fn render_per_eye(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        let hand_material = engine::scene::color_material::create(vec3(1.0, 0.0, 0.0));
        let transform = Matrix4::from_scale(0.25) * Matrix4::from_translation(vec3(0.0, 4.0, 0.0));
        let mut hand_obj = SceneObject::new(hand_material, Box::new(engine::scene::cube::create()));
        hand_obj.set_transform(transform);

        // Sample for rendering
        let font = self.asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
        // let text_obj_0_0 =
        //     SceneObject::screen_space_text("0, 0", font.clone(), 16.0, 0.5, 0.0, 0.0);

        let mut objs = self.active_game_scene.render_per_eye(
            &mut self.asset_cache,
            view,
            projection,
            screen_size,
            &self.options,
        );

        let world_position = vec3(0.0, 1.0, 0.0);
        let screen_width = screen_size.x;
        let screen_height = screen_size.y;
        let world_space_pos = engine::util::project(
            view,
            projection,
            world_position,
            screen_width,
            screen_height,
        );
        let _text_obj_dynamic = SceneObject::screen_space_text(
            "{dynamic}",
            font.clone(),
            16.0,
            0.5,
            world_space_pos.x,
            world_space_pos.y,
        );
        // let text_material =
        //     TextMaterial::create(font.get_texture().clone(), vec4(1.0, 0.0, 0.0, 1.0));
        // let font_size = 30.0f32;

        // 4 years earlier
        // Ramsey Recruitment Ctr
        // let text_string = "Ramsey Recruitment Ctr.";
        objs.extend(vec![hand_obj /*  text_obj_dynamic*/]);
        objs
    }

    pub fn finish_render(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.active_game_scene
            .finish_render(&mut self.asset_cache, view, projection, screen_size)
    }

    fn update_music_cue_if_necessary(&mut self, new_cue: String) {
        if self.last_music_cue.is_none() || !self.last_music_cue.as_ref().unwrap().eq(&new_cue) {
            info!("updating music cue: {}", new_cue);
            self.audio_context
                .set_background_music_cue(new_cue.to_owned());
            self.last_music_cue = Some(new_cue);
        }
    }

    fn update_env_sound_if_necessary(&mut self, new_cue: String) {
        if self.last_env_sound.is_none() || !self.last_env_sound.as_ref().unwrap().eq(&new_cue) {
            let maybe_audio_clip = self
                .asset_cache
                .get_opt(&AUDIO_IMPORTER, &format!("{new_cue}.wav"));

            if let Some(audio_clip) = maybe_audio_clip {
                info!("updating env_sound: {}", new_cue);
                self.audio_context.set_environmental_sound(audio_clip);
                self.last_env_sound = Some(new_cue);
            } else {
                warn!("env_sound: unable to load sound: {}", new_cue)
            }
        }
    }

    fn resolve_schema(&self, name: &str) -> String {
        let sound_schema = &self.global_context.gamesys.sound_schema;
        let ret = sound_schema
            .get_random_sample(name)
            .unwrap_or_else(|| name.to_owned());
        trace!("resolved sound schema {} to {}", name, ret);
        ret
    }
}
