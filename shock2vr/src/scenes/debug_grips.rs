//! One held item at a time, in the right hand, at a fixed spot: the contact
//! sheet for the finger fit ([`crate::hand_fit`]).
//!
//! The item is drawn at the very transform the fit measured it in
//! ([`vr_config::held_model_hand_transform`]) and the glove at the very grip
//! the fit solved ([`GloveRenderer::fitted_grip`], the same call the wield
//! makes), so what the camera sees is what the solver decided rather than a
//! second, flattering placement.
//!
//! Cycle items with the `grip_item` dev param; frame them with `/v1/camera`.

use cgmath::{Deg, Quaternion, Rotation3, Vector3, vec3};
use dark::{
    importers::{MODELS_IMPORTER, VR_HELD_GUN_MODELS_IMPORTER},
    model::Model,
};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    hand_glove::{self, GloveRenderer, HandLight, HandPreshape},
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
    vr_config::{self, Handedness},
};

/// Where the held item hangs, in front of the scene's camera.
const HAND_POSITION: Vector3<f32> = vec3(0.0, 2.6, 1.0);

/// The owner's grip test cases: a mug (a handle its bounding box cannot see),
/// a printed magazine (a slab), a clip (a small box), a basketball (bigger
/// than the palm), and the four guns whose `_h` models range from an authored
/// pistol grip to no grip at all.
///
/// `magci` is one of the six magazine covers the Magazines template picks
/// from; they share a mesh, so any of them is the slab case.
const GRIP_TEST_ITEMS: &[&str] = &[
    "mug", "magci", "ammoss", "hamball", "atek_h", "sg_h", "fsn_h", "al_h",
];

struct GripHooks {
    /// Which of [`GRIP_TEST_ITEMS`] was drawn last, so the log line is printed
    /// on a change rather than every frame.
    shown: Option<usize>,
    glove: Option<GloveRenderer>,
}

/// The item the `grip_item` dev param selects, clamped to the list.
fn selected_item() -> usize {
    let index = crate::dev_params::get(crate::dev_params::GRIP_ITEM).round();
    (index.max(0.0) as usize).min(GRIP_TEST_ITEMS.len() - 1)
}

impl DebugSceneHooks for GripHooks {
    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        scene_objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) {
        let index = selected_item();
        let model_name = GRIP_TEST_ITEMS[index];
        let Some(glove) = self.glove.as_mut() else {
            return;
        };

        // Guns are wielded shrunk to life size and stripped of their baked
        // arm; everything else is held as authored.
        let is_gun = vr_config::is_vr_gun_view_model(model_name);
        let scale = if is_gun {
            vr_config::gun_wield_scale()
        } else {
            1.0
        };

        // The right hand at a fixed spot, fingers along -X and rolled so the
        // palm - the side the fit happens on - faces the -Z camera.
        let hand = hand_glove::hand_to_world(
            HAND_POSITION,
            Quaternion::from_angle_y(Deg(90.0)) * Quaternion::from_angle_z(Deg(90.0)),
            Handedness::Right,
        );

        // The same two calls the mission loop makes before a hand places
        // anything: the profiles, then the seat measured off any model they
        // leave out.
        crate::vr_grips::ensure_loaded(asset_cache);
        hand_glove::warm_held_seat(model_name, asset_cache);

        // The same cached entry point the wield uses; the mesh size and solve
        // time are logged by the solve itself, at debug level.
        let fit = glove.fitted_grip(model_name, Handedness::Right, scale, asset_cache);

        if self.shown != Some(index) {
            self.shown = Some(index);
            match &fit {
                Some(fit) => info!(
                    "debug_grips [{index}] {model_name}: family {} -> {:?}",
                    fit.family.as_str(),
                    fit.amounts
                ),
                None => info!("debug_grips [{index}] {model_name}: no contact mesh"),
            }
        }

        scene_objects.extend(glove.render_hand(
            hand,
            0.0,
            0.0,
            hand_glove::Hold::Item(fit.map(|fit| fit.amounts)),
            HandLight::Off,
            HandPreshape::None,
        ));

        if let Some(model) = held_model(model_name, is_gun, asset_cache) {
            let world =
                hand * vr_config::held_model_hand_transform(model_name, Handedness::Right, scale);
            scene_objects.extend(model.clone_scene_objects().into_iter().map(|mut object| {
                object.set_transform(world);
                object
            }));
        }
    }
}

/// The held item's model, drawn the way a VR wield would draw it - a gun
/// through the arm-stripping importer, everything else as authored.
fn held_model(model_name: &str, is_gun: bool, asset_cache: &mut AssetCache) -> Option<Model> {
    let file = format!("{model_name}.BIN");
    if is_gun {
        asset_cache
            .get_opt(&VR_HELD_GUN_MODELS_IMPORTER, &file)
            .map(|held| held.0.clone())
    } else {
        asset_cache
            .get_opt::<_, Model, _>(&MODELS_IMPORTER, &file)
            .map(|model| model.as_ref().clone())
    }
}

pub struct DebugGripsScene;

impl DebugGripsScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_grips")
            .with_default_floor()
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 2.6, 0.35),
                Quaternion::from_angle_y(Deg(90.0)),
            ));

        let core = builder.build_core(DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        });

        info!(
            "Created debug grips scene: {} items, select with the grip_item dev param",
            GRIP_TEST_ITEMS.len()
        );

        Box::new(HookedDebugScene::new(
            core,
            GripHooks {
                shown: None,
                glove: GloveRenderer::new(asset_cache),
            },
        ))
    }
}
