use super::ToolScene;
use dark::importers::{ANIMATION_CLIP_IMPORTER, MODELS_IMPORTER};
use dark::motion::{AnimationClip, AnimationEvent, AnimationPlayer};
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
use std::rc::Rc;
use std::time::Duration;

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
    model: Rc<dark::model::Model>,
    animation_player: AnimationPlayer,
    animation_controller: Option<AnimationController>,
}

impl BinAiViewerScene {
    pub fn from_clips(
        mesh_file_path: String,
        clip_names: Vec<String>,
        asset_cache: &mut AssetCache,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let model = asset_cache.get(&MODELS_IMPORTER, mesh_file_path.as_str());

        let mut controller = load_animation_controller(clip_names, asset_cache)?;
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
    clip_names: Vec<String>,
    asset_cache: &mut AssetCache,
) -> Result<AnimationController, Box<dyn std::error::Error>> {
    if clip_names.is_empty() {
        return Ok(AnimationController::new(Vec::new()));
    }

    let mut clips = Vec::new();
    for name in clip_names {
        if let Some(clip) = asset_cache.get_opt(&ANIMATION_CLIP_IMPORTER, name.as_str()) {
            clips.push(clip);
        } else {
            return Err(format!("Unable to load animation clip '{name}'. Ensure the file exists under Data/res/motions.").into());
        }
    }

    Ok(AnimationController::new(clips))
}
