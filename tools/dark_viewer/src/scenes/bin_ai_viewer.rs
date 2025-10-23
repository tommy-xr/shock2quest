use super::ToolScene;
use dark::{ss2_bin_ai_loader, ss2_bin_header, ss2_cal_loader, ss2_skeleton};
use dark::ss2_skeleton::Skeleton;
use dark::motion::{AnimationEvent, AnimationPlayer, MotionDB, MotionQuery, MotionQueryItem, MotionQuerySelectionStrategy};
use dark::model::Model;
use dark::importers::ANIMATION_CLIP_IMPORTER;
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
// use shock2vr::creature::creature_definitions::get_creature_definition;
use std::fs::File;
use std::io::BufReader;
use std::rc::Rc;
use std::time::Duration;
use num::ToPrimitive;

pub struct BinAiViewerScene {
    mesh_file_path: String,
    skeleton_file_path: String,
    skeleton: Option<Rc<Skeleton>>,
    ai_mesh: Option<ss2_bin_ai_loader::SystemShock2AIMesh>,
    animation_player: Option<AnimationPlayer>,
    animation_param: Option<String>,
    creature_type: Option<String>,
    motiondb: Option<MotionDB>,
    total_time: Duration,
    current_animation_index: usize,
    available_animations: Vec<String>,
    animation_loaded: bool,
}

impl BinAiViewerScene {
    pub fn from_files(
        mesh_file_path: String,
        skeleton_file_path: String,
        animation_param: Option<String>,
        creature_type: Option<String>,
        resource_path_fn: fn(&str) -> String
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Load skeleton
        let skeleton_file = File::open(resource_path_fn(&skeleton_file_path))?;
        let mut skeleton_reader = BufReader::new(skeleton_file);
        let ss2_cal = ss2_cal_loader::read(&mut skeleton_reader);
        let skeleton = Rc::new(ss2_skeleton::create(ss2_cal));

        // Load AI mesh
        let mesh_file = File::open(resource_path_fn(&mesh_file_path))?;
        let mut mesh_reader = BufReader::new(mesh_file);
        let header = ss2_bin_header::read(&mut mesh_reader);
        let ai_mesh = ss2_bin_ai_loader::read(&mut mesh_reader, &header);

        // Load MotionDB if we have animation parameters
        let motiondb = if animation_param.is_some() {
            match File::open(resource_path_fn("motiondb.bin")) {
                Ok(motiondb_file) => {
                    let mut motiondb_reader = BufReader::new(motiondb_file);
                    Some(MotionDB::read(&mut motiondb_reader))
                }
                Err(_) => {
                    println!("Warning: Could not load motiondb.bin, animation support disabled");
                    None
                }
            }
        } else {
            None
        };

        let mut available_animations = Vec::new();
        let animation_player = if let Some(ref anim_param) = animation_param {
            if anim_param.starts_with('+') {
                // Tag-based animation - we'll populate available_animations later
                Some(AnimationPlayer::empty())
            } else if anim_param.ends_with(".mc") {
                // Direct .mc file
                available_animations.push(anim_param.clone());
                Some(AnimationPlayer::empty())
            } else {
                None
            }
        } else {
            None
        };

        Ok(BinAiViewerScene {
            mesh_file_path,
            skeleton_file_path,
            skeleton: Some(skeleton),
            ai_mesh: Some(ai_mesh),
            animation_player,
            animation_param,
            creature_type,
            motiondb,
            total_time: Duration::ZERO,
            current_animation_index: 0,
            available_animations,
            animation_loaded: false,
        })
    }
}

impl ToolScene for BinAiViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);
        self.total_time += elapsed;

        // Update animation if we have one
        if let Some(ref mut player) = self.animation_player {
            let (updated_player, _motion_flags, events, _velocity) = AnimationPlayer::update(player, elapsed);
            *player = updated_player;

            // Check for animation completion events
            for event in events {
                if let AnimationEvent::Completed = event {
                    if self.available_animations.len() > 1 {
                        // Cycle to next animation
                        self.current_animation_index = (self.current_animation_index + 1) % self.available_animations.len();
                        self.animation_loaded = false; // Mark that we need to load the next animation
                        println!("Animation completed. Cycling to animation: {}", self.available_animations[self.current_animation_index]);
                        // We'll load the next animation in the render method where we have access to asset_cache
                    }
                }
            }
        }
    }

    fn render(&mut self, asset_cache: &mut AssetCache) -> Scene {
        let mut scene = vec![];

        if let (Some(ai_mesh), Some(skeleton)) = (&self.ai_mesh, &self.skeleton) {
            // Initialize animations if needed
            if let (Some(ref anim_param), Some(ref motiondb), Some(ref creature_type)) =
                (&self.animation_param, &self.motiondb, &self.creature_type) {

                if anim_param.starts_with('+') && self.available_animations.is_empty() {
                    // Parse tags and query motiondb
                    let tags: Vec<&str> = anim_param[1..].split(',').collect();
                    let query_items: Vec<MotionQueryItem> = tags.iter()
                        .map(|tag| MotionQueryItem::new(tag.trim_start_matches('+')))
                        .collect();

                    // For now, use a hardcoded actor type since creature module is private
                    // TODO: Make creature definitions public or find another way to get actor_type
                    let actor_type = creature_type.parse::<u32>().unwrap_or(0);
                    let query = MotionQuery::new(actor_type, query_items)
                        .with_selection_strategy(MotionQuerySelectionStrategy::Sequential(0));

                    // Query all possible animations
                    let mut temp_animations = Vec::new();
                    for i in 0..10 { // Try up to 10 variants
                        let mut seq_query = query.clone();
                        seq_query.selection_strategy = MotionQuerySelectionStrategy::Sequential(i);
                        if let Some(animation_name) = motiondb.query(seq_query) {
                            let full_name = format!("{}_.mc", animation_name);
                            if !temp_animations.contains(&full_name) {
                                temp_animations.push(full_name);
                            }
                        } else {
                            break; // No more animations
                        }
                    }
                    self.available_animations = temp_animations;
                    println!("Found {} animations for tags: {:?}", self.available_animations.len(), tags);
                }
            }

            // Load current animation if we have one and the animation list is ready
            if let Some(ref mut player) = self.animation_player {
                if !self.available_animations.is_empty() && !self.animation_loaded {
                    let animation_name = &self.available_animations[self.current_animation_index];
                    if let Some(clip) = asset_cache.get_opt(&ANIMATION_CLIP_IMPORTER, animation_name) {
                        *player = AnimationPlayer::queue_animation(player, clip);
                        self.animation_loaded = true;
                        println!("Playing animation: {}", animation_name);
                    }
                }
            }

            // Create model and apply animation if available
            let model = Model::from_ai_bin(ai_mesh.clone(), skeleton.clone(), asset_cache);

            if let Some(ref player) = self.animation_player {
                // Use animated scene objects with the animation player
                for obj in model.to_animated_scene_objects(player) {
                    scene.push(obj);
                }
            } else {
                // Use static scene objects
                for obj in model.to_scene_objects() {
                    scene.push(obj.clone());
                }
            }
        }

        Scene::from_objects(scene)
    }
}