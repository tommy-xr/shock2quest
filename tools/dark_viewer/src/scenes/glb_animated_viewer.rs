use super::ToolScene;
use cgmath::Matrix4;
use dark::importers::{GLB_ANIMATION_IMPORTER, GLB_MODELS_IMPORTER};
use dark::motion::{AnimationClip, AnimationEvent, AnimationPlayer};
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone)]
struct GlbAnimationController {
    clips: Vec<Rc<AnimationClip>>,
    next_index: usize,
}

impl GlbAnimationController {
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

pub struct GlbAnimatedViewerScene {
    model_name: String,
    scale: f32,
    animation_player: AnimationPlayer,
    animation_controller: Option<GlbAnimationController>,
}

impl GlbAnimatedViewerScene {
    pub fn from_model_and_animations(
        model_name: String,
        animation_names: Vec<String>,
        scale: f32,
        asset_cache: &mut AssetCache,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Load animations from the GLB file
        let animation_clips = asset_cache.get(&GLB_ANIMATION_IMPORTER, &model_name);

        println!("Loaded {} animations from GLB file:", animation_clips.len());
        for (i, clip) in animation_clips.iter().enumerate() {
            let name = clip.name.as_deref().unwrap_or("Unnamed");
            println!(
                "  {}: {} ({} frames, {:.2}s)",
                i,
                name,
                clip.num_frames,
                clip.duration.as_secs_f32()
            );
        }

        // Filter animations by requested names (if any)
        let filtered_clips: Vec<Rc<AnimationClip>> = if animation_names.is_empty() {
            // If no specific animations requested, use all animations
            animation_clips
                .iter()
                .map(|clip| Rc::new(clip.clone()))
                .collect()
        } else {
            // Filter by requested animation names
            let mut filtered = Vec::new();
            for name in &animation_names {
                if let Some(clip) = animation_clips
                    .iter()
                    .find(|c| c.name.as_deref().unwrap_or("").to_lowercase() == name.to_lowercase())
                {
                    filtered.push(Rc::new(clip.clone()));
                    println!("Found requested animation: {}", name);
                } else {
                    println!("Warning: Animation '{}' not found in GLB file", name);
                }
            }
            filtered
        };

        if filtered_clips.is_empty() {
            return Err(format!(
                "No animations found. Available animations: {}",
                animation_clips
                    .iter()
                    .map(|c| c.name.as_deref().unwrap_or("Unnamed"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into());
        }

        // Set up animation controller
        let mut controller = GlbAnimationController::new(filtered_clips);
        let mut animation_player = AnimationPlayer::empty();

        // Start with the first animation
        if let Some(first_clip) = controller.take_next() {
            println!(
                "Starting with animation: {}",
                first_clip.name.as_deref().unwrap_or("Unnamed")
            );
            animation_player = AnimationPlayer::queue_animation(&animation_player, first_clip);
        }

        Ok(GlbAnimatedViewerScene {
            model_name,
            scale,
            animation_player,
            animation_controller: Some(controller),
        })
    }
}

impl ToolScene for GlbAnimatedViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);

        if let Some(controller) = &mut self.animation_controller {
            let (updated_player, _flags, events, _velocity) =
                AnimationPlayer::update(&self.animation_player, elapsed);

            self.animation_player = updated_player;

            // Handle animation events (like completion)
            for event in events {
                if matches!(event, AnimationEvent::Completed) {
                    if let Some(next_clip) = controller.take_next() {
                        println!(
                            "Animation completed, starting: {}",
                            next_clip.name.as_deref().unwrap_or("Unnamed")
                        );
                        self.animation_player =
                            AnimationPlayer::queue_animation(&self.animation_player, next_clip);
                    }
                }
            }
        }
    }

    fn render(&self, asset_cache: &mut AssetCache) -> Scene {
        // Load the GLB model and render with current animation state
        let model = asset_cache.get(&GLB_MODELS_IMPORTER, &self.model_name);
        let mut scene_objects = model.to_animated_scene_objects(&self.animation_player);

        // Apply scale transformation to all scene objects
        let scale_matrix = Matrix4::from_scale(self.scale);
        for scene_object in &mut scene_objects {
            scene_object.transform = scale_matrix * scene_object.transform;
        }

        Scene::from_objects(scene_objects)
    }
}
