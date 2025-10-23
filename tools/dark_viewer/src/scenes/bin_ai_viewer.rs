use super::ToolScene;
use dark::{
    importers::{ANIMATION_CLIP_IMPORTER, MOTIONDB_IMPORTER},
    model::Model,
    motion::{AnimationClip, AnimationEvent, AnimationPlayer, MotionQuery, MotionQueryItem},
    ss2_bin_ai_loader, ss2_bin_header, ss2_cal_loader, ss2_skeleton,
};
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
use std::{fs::File, io::BufReader, rc::Rc, time::Duration};

#[derive(Clone, Debug)]
pub enum BinAiAnimationConfig {
    Clip { clip_name: String },
    Tag { tag: String, actor_type: u32 },
}

#[derive(Clone)]
struct AnimationController {
    clips: Vec<Rc<AnimationClip>>,
    next_index: usize,
}

impl AnimationController {
    fn new(clips: Vec<Rc<AnimationClip>>) -> Self {
        Self {
            clips,
            next_index: 0,
        }
    }

    fn take_next(&mut self) -> Option<Rc<AnimationClip>> {
        if self.clips.is_empty() {
            return None;
        }

        let clip = self.clips[self.next_index].clone();
        self.next_index = (self.next_index + 1) % self.clips.len();
        Some(clip)
    }

    fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

pub struct BinAiViewerScene {
    model: Model,
    animation_player: AnimationPlayer,
    animation_controller: Option<AnimationController>,
}

impl BinAiViewerScene {
    pub fn from_config(
        mesh_file_path: String,
        skeleton_file_path: String,
        animation: BinAiAnimationConfig,
        asset_cache: &mut AssetCache,
        resource_path_fn: fn(&str) -> String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let skeleton = {
            let skeleton_file = File::open(resource_path_fn(&skeleton_file_path))?;
            let mut skeleton_reader = BufReader::new(skeleton_file);
            let ss2_cal = ss2_cal_loader::read(&mut skeleton_reader);
            Rc::new(ss2_skeleton::create(ss2_cal))
        };

        let ai_mesh = {
            let mesh_file = File::open(resource_path_fn(&mesh_file_path))?;
            let mut mesh_reader = BufReader::new(mesh_file);
            let header = ss2_bin_header::read(&mut mesh_reader);
            ss2_bin_ai_loader::read(&mut mesh_reader, &header)
        };

        let model = Model::from_ai_bin(ai_mesh, skeleton, asset_cache);

        let mut controller = load_animation_controller(animation, asset_cache)?;

        if controller.is_empty() {
            return Err("Animation playlist is empty.".into());
        }

        let mut animation_player = AnimationPlayer::empty();
        if let Some(first_clip) = controller.take_next() {
            animation_player = AnimationPlayer::queue_animation(&animation_player, first_clip);
        }

        Ok(BinAiViewerScene {
            model,
            animation_player,
            animation_controller: Some(controller),
        })
    }
}

impl ToolScene for BinAiViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);

        if let Some(controller) = &mut self.animation_controller {
            let (updated_player, _flags, events, _velocity) =
                AnimationPlayer::update(&self.animation_player, elapsed);
            self.animation_player = updated_player;

            for event in events {
                if matches!(event, AnimationEvent::Completed) {
                    if let Some(next_clip) = controller.take_next() {
                        self.animation_player =
                            AnimationPlayer::queue_animation(&self.animation_player, next_clip);
                    }
                }
            }
        }
    }

    fn render(&self, _asset_cache: &mut AssetCache) -> Scene {
        let objects = self.model.to_animated_scene_objects(&self.animation_player);
        Scene::from_objects(objects)
    }
}

fn load_animation_controller(
    animation: BinAiAnimationConfig,
    asset_cache: &mut AssetCache,
) -> Result<AnimationController, Box<dyn std::error::Error>> {
    let clips = match animation {
        BinAiAnimationConfig::Clip { clip_name } => {
            vec![asset_cache.get(&ANIMATION_CLIP_IMPORTER, clip_name.as_str())]
        }
        BinAiAnimationConfig::Tag { tag, actor_type } => {
            let motion_db = asset_cache.get(&MOTIONDB_IMPORTER, "motiondb.bin");
            let query = MotionQuery::new(actor_type, vec![MotionQueryItem::new(&tag)]);
            let results = motion_db.query_all(query);

            if results.is_empty() {
                return Err(format!(
                    "No animations found for tag '+{}' and actor type {}",
                    tag, actor_type
                )
                .into());
            }

            results
                .into_iter()
                .map(|name| {
                    let clip_name = format!("{}_.mc", name);
                    asset_cache.get(&ANIMATION_CLIP_IMPORTER, clip_name.as_str())
                })
                .collect::<Vec<_>>()
        }
    };

    Ok(AnimationController::new(clips))
}
