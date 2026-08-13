//! Tuning harness for the free-hand pose library (`hand_pose_library`).
//!
//! Each authored pose is drawn at its own hand origin, with a marker showing
//! that origin's axes. Because the poses are anchored by their geometry, a
//! correctly aligned pose puts its wrist on the marker with the fingers down the
//! blue (-Z, "where the hand points") axis. Whatever roll is left over is the
//! number to author in `HandPose::source`.
//!
//! ```bash
//! cargo dbgr --mission debug_hand_poses --port 8080
//! curl -X POST http://127.0.0.1:8080/v1/step -d '{"frames": 30}'
//! curl -X POST http://127.0.0.1:8080/v1/screenshot -d '{"filename": "poses.png"}'
//! ```

use cgmath::{Deg, Matrix4, Quaternion, Rotation3, Vector3, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, cube},
};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    hand_pose_library::{self, LoadedPose},
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
};

const ROW_HEIGHT: f32 = 3.0;
const SPACING: f32 = 0.7;

/// A stubby axis marker at the hand origin: red +X, green +Y, blue -Z (forward).
fn axis_marker() -> Vec<SceneObject> {
    let axes = [
        (
            vec3(1.0, 0.0, 0.0),
            vec3(0.09, 0.006, 0.006),
            vec3(0.05, 0.0, 0.0),
        ),
        (
            vec3(0.0, 1.0, 0.0),
            vec3(0.006, 0.09, 0.006),
            vec3(0.0, 0.05, 0.0),
        ),
        (
            vec3(0.2, 0.4, 1.0),
            vec3(0.006, 0.006, 0.14),
            vec3(0.0, 0.0, -0.07),
        ),
    ];

    axes.into_iter()
        .map(|(color, scale, offset)| {
            let mut object =
                SceneObject::new(color_material::create(color), Box::new(cube::create()));
            object.set_transform(
                Matrix4::from_translation(offset)
                    * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z),
            );
            object
        })
        .collect()
}

struct PoseHooks {
    poses: Vec<LoadedPose>,
    markers: Vec<SceneObject>,
}

impl DebugSceneHooks for PoseHooks {
    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        scene_objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) {
        let count = self.poses.len() as f32;
        for (index, pose) in self.poses.iter().enumerate() {
            // Negated so the row reads left to right (the camera looks along -X).
            let offset = ((count - 1.0) / 2.0 - index as f32) * SPACING;
            let world = Matrix4::from_translation(vec3(offset, ROW_HEIGHT, 1.0))
                * Matrix4::from_angle_y(Deg(90.0));

            scene_objects.extend(pose.at(world));
            scene_objects.extend(self.markers.iter().map(|marker| {
                let mut clone = marker.clone();
                clone.set_transform(world * marker.get_transform());
                clone
            }));
        }
    }
}

pub struct DebugHandPosesScene;

impl DebugHandPosesScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_hand_poses")
            .with_default_floor()
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 3.0, -1.4),
                Quaternion::from_angle_y(Deg(90.0)),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let core = builder.build_core(build_options);
        let poses = hand_pose_library::load(asset_cache);

        info!(
            "Created debug hand poses scene: {:?}",
            poses.iter().map(|pose| pose.pose).collect::<Vec<_>>()
        );

        Box::new(HookedDebugScene::new(
            core,
            PoseHooks {
                poses,
                markers: axis_marker(),
            },
        ))
    }
}
