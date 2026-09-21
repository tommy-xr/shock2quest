//! Controller-to-glove fit check. Quest supplies passthrough underneath this
//! scene; desktop/debug show the identical gloves against transparent black.
use cgmath::{Matrix4, Quaternion, Rotation3, SquareMatrix, Vector3, vec3};
use dark::{model::Model, motion::AnimationPlayer};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::EntityId;

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
};
use crate::{
    GameOptions,
    game_scene::GameScene,
    glove_fit::GloveFit,
    hand_glove::{GloveRenderer, HandLight},
    input_context::InputContext,
    mission::{GlobalContext, mission_core::MissionCore},
    time::Time,
    vr_config::Handedness,
    vr_support::GripPose,
};

struct GloveFitHooks {
    renderer: Option<GloveRenderer>,
    amps: Option<[Model; 2]>,
    input: InputContext,
}

impl DebugSceneHooks for GloveFitHooks {
    fn before_update(
        &mut self,
        _core: &mut MissionCore,
        _time: &Time,
        input: &InputContext,
        _assets: &mut AssetCache,
        _options: &GameOptions,
    ) {
        self.input = input.clone();
    }

    fn after_render(
        &mut self,
        core: &mut MissionCore,
        objects: &mut Vec<SceneObject>,
        camera_position: &mut Vector3<f32>,
        camera_rotation: &mut Quaternion<f32>,
        _assets: &mut AssetCache,
        _options: &GameOptions,
    ) {
        // Keep the normal physics/camera rig, but strip the gallery, body gear,
        // wrist readouts and rays so only the hand silhouette covers the room.
        objects.clear();
        let fit = GloveFit::current();
        if self.amps.is_none() && !fit.visible {
            return;
        }
        // MissionCore may have changed stance since before_update. Match the
        // same rebase used by the production hands and the runtime's two eyes.
        let crouched = core.player_tracking_is_crouched();
        crate::vr_tracking::TrackingTransform::rebase_input(
            &mut self.input,
            crate::physics::player_center_above_floor(crouched),
            crate::physics::player_eye_cap_above_center(crouched),
        );
        for (index, (input, side)) in [
            (&self.input.left_hand, Handedness::Left),
            (&self.input.right_hand, Handedness::Right),
        ]
        .into_iter()
        .enumerate()
        {
            let pose = GripPose {
                position: crate::virtual_hand::hand_world_position(
                    *camera_position,
                    *camera_rotation,
                    input.position,
                ),
                rotation: *camera_rotation * input.rotation,
            };
            if !pose.is_tracked()
                || self
                    .input
                    .pose_tracking
                    .is_some_and(|tracking| !tracking.hands[index])
            {
                continue;
            }
            if let Some(amps) = &self.amps {
                // The production importer, bind pose, handedness and live grip
                // are shared with the held amp. No extra fit-scene correction.
                let grip =
                    crate::vr_config::get_vr_hand_model_adjustments_from_model("amp_h", side);
                let transform = Matrix4::from_translation(pose.position)
                    * Matrix4::from(pose.rotation)
                    * Matrix4::from_translation(grip.offset)
                    * Matrix4::from(grip.rotation)
                    * Matrix4::from_scale(crate::dev_params::get(crate::dev_params::PSI_AMP_SCALE));
                let mut hand = amps[index].to_animated_scene_objects(&AnimationPlayer::empty());
                for object in &mut hand {
                    object.set_transform(transform);
                }
                crate::util::tag_render_source(&mut hand, crate::util::render_source::PLAYER_HANDS);
                objects.extend(hand);
                continue;
            }
            let Some(renderer) = self.renderer.as_mut() else {
                continue;
            };
            // Production geometry, retargeting, analog curls and handedness.
            // Only this scene's outer transform is adjustable: baked weapon
            // grips and cached wrist frames elsewhere remain coherent.
            let mut hand = renderer.render_hand(
                vec3(0.0, 0.0, 0.0),
                Quaternion::from_angle_y(cgmath::Deg(0.0)),
                side,
                input.trigger_value,
                input.squeeze_value,
                false,
                None,
                HandLight::Off,
                None,
            );
            let transform = fit.transform(pose, side);
            for object in &mut hand {
                object.set_transform(transform * object.get_transform());
            }
            crate::util::tag_render_source(&mut hand, crate::util::render_source::PLAYER_HANDS);
            objects.extend(hand);
        }
    }
}

pub struct DebugGlovesScene;

impl DebugGlovesScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        Self::build(
            global_context,
            game_options,
            asset_cache,
            audio_context,
            false,
        )
    }

    pub fn psi_amp(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        Self::build(
            global_context,
            game_options,
            asset_cache,
            audio_context,
            true,
        )
    }

    fn build(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
        psi_amp: bool,
    ) -> Box<dyn GameScene> {
        let renderer = if psi_amp {
            None
        } else {
            GloveRenderer::new(asset_cache)
        };
        let amps = if psi_amp {
            asset_cache
                .get_opt(&dark::importers::VR_HELD_MODELS_IMPORTER, "amp_h.bin")
                .map(|source| {
                    [Handedness::Left, Handedness::Right].map(|side| {
                        let mut model = Model::transform(&source.model, Matrix4::identity());
                        model.apply_local_transform(side.gun_mirror());
                        model
                    })
                })
        } else {
            None
        };
        // Invisible support keeps the pawn from falling. The scene hook removes
        // its picture, not its collider; physical head/hand tracking stays live.
        let core = DebugSceneBuilder::new(if psi_amp {
            "debug_psi_fit"
        } else {
            "debug_gloves"
        })
        .with_default_floor()
        .build_core(DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        });
        Box::new(HookedDebugScene::new(
            core,
            GloveFitHooks {
                renderer,
                amps,
                input: InputContext::default(),
            },
        ))
    }
}
