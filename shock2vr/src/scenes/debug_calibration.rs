//! Life-size, frozen hybrid and floor ruler over Quest passthrough.
use cgmath::{Deg, Matrix4, Quaternion, Rotation3, Vector3, vec2, vec3};
use dark::{
    importers::{ANIMATION_CLIP_IMPORTER, MODELS_IMPORTER, SystemShockContentModel, skinned_point},
    motion::AnimationPlayer,
    ss2_bin_ai_loader,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SKINNING_PALETTE_SIZE, SceneObject, VertexPositionTextureSkinnedNormal},
};
use rapier3d::prelude::*;
use shipyard::EntityId;

use super::debug_common::{DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks};
use crate::{
    GameOptions, METERS_PER_WORLD_UNIT, SpawnLocation,
    game_scene::GameScene,
    mission::{GlobalContext, mission_core::MissionCore},
    ui::{HAlign, Rect, UiCanvas, VAlign, WorldPanel},
};

fn posed_vertical_bounds(
    vertices: &[VertexPositionTextureSkinnedNormal],
    palette: &[Matrix4<f32>; SKINNING_PALETTE_SIZE],
) -> (f32, f32) {
    vertices
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), vertex| {
            let y = skinned_point(vertex, palette)
                .expect("valid hybrid skinning joint")
                .y;
            (min.min(y), max.max(y))
        })
}

struct CalibrationHooks(Vec<SceneObject>);

impl DebugSceneHooks for CalibrationHooks {
    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _assets: &mut AssetCache,
        _options: &GameOptions,
    ) {
        // Leave the room visible, retaining the optional height/crouch readout.
        objects.retain(|object| {
            object.debug_tag().and_then(|tag| tag.source.as_deref())
                == Some(crate::util::render_source::DEBUG_OVERLAY)
        });
        objects.extend(self.0.iter().cloned());
    }
}

pub fn create_debug_calibration_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    // Keep CPU vertices only in this diagnostic, and use the production importer
    // and its actual animation palette so remaster and classic assets both work.
    let mut reader = asset_cache
        .get_raw_reader("grunt_p.bin")
        .expect("hybrid model")
        .into_inner();
    let content = (MODELS_IMPORTER.loader)("grunt_p.bin".into(), &mut reader, asset_cache, &());
    let SystemShockContentModel::Mesh(mesh, skeleton, pmnm) = &content else {
        panic!("hybrid must be a skeletal mesh");
    };
    let classic = ss2_bin_ai_loader::to_vertices(mesh, skeleton).0;
    let upgraded = pmnm.as_ref().map(|mesh| mesh.to_skinned_vertices());
    let model = (MODELS_IMPORTER.processor)(content, asset_cache, &());
    let vertices: Vec<_> = if model.bind_matrices().is_some() {
        upgraded.expect("rendered PMNM geometry")
    } else {
        classic
    }
    .into_iter()
    .flat_map(|(_, vertices)| vertices)
    .collect();
    let pose =
        AnimationPlayer::from_pose(asset_cache.get(&ANIMATION_CLIP_IMPORTER, "ogpstd_.mc"), 0);
    let mut objects = model.to_animated_scene_objects(&pose);
    let (sole_y, crown_y) = posed_vertical_bounds(&vertices, &objects[0].skinning_data);
    let height_m = (crown_y - sole_y) * METERS_PER_WORLD_UNIT;
    tracing::info!(
        height_m,
        height_ft = height_m / 0.3048,
        "SHOCK2QUEST_CALIBRATION grunt_p ogpstd frame 0"
    );
    let hybrid_x = -2.0 / METERS_PER_WORLD_UNIT;
    for object in &mut objects {
        object.set_transform(
            Matrix4::from_translation(vec3(hybrid_x, -sole_y, 0.0))
                * Matrix4::from_angle_y(Deg(90.0)),
        );
    }
    crate::util::tag_render_source(&mut objects, "calibration_hybrid");

    // One pixel layout for both presentations; 400 pixels represent one metre.
    let mut ruler = UiCanvas::new(vec2(400.0, 1000.0));
    ruler.fill(Rect::new(0.0, 0.0, 3.0, 1000.0), [180, 220, 255]);
    for tick in 0..=25 {
        let y = 1000.0 - tick as f32 * 40.0;
        let major = tick % 5 == 0;
        ruler.fill(
            Rect::new(0.0, y.min(998.0), if major { 55.0 } else { 25.0 }, 2.0),
            [180, 220, 255],
        );
        if major {
            ruler.text_native(
                Rect::new(65.0, (y - 12.0).clamp(0.0, 976.0), 180.0, 24.0),
                &format!("{:.1} m", tick as f32 / 10.0),
                "mainfont.fon",
                HAlign::Left,
                VAlign::Middle,
            );
        }
    }
    let six_ft_y = 1000.0 - 1.8288 * 400.0;
    ruler.fill(Rect::new(0.0, six_ft_y, 160.0, 3.0), [255, 210, 80]);
    ruler.text_native(
        Rect::new(65.0, six_ft_y - 25.0, 320.0, 24.0),
        "6 ft (top of head)",
        "mainfont.fon",
        HAlign::Left,
        VAlign::Middle,
    );
    ruler.text_native(
        Rect::new(65.0, 52.0, 330.0, 48.0),
        &format!("Hybrid: {height_m:.2} m / {:.2} ft", height_m / 0.3048),
        "mainfont.fon",
        HAlign::Left,
        VAlign::Middle,
    );
    let panel = WorldPanel {
        center: vec3(
            hybrid_x,
            1.25 / METERS_PER_WORLD_UNIT,
            -0.9 / METERS_PER_WORLD_UNIT,
        ),
        rotation: Quaternion::from_angle_y(Deg(90.0)),
        size: vec2(1.0, 2.5) / METERS_PER_WORLD_UNIT,
    };
    let mut ruler_objects =
        ruler.render_world_space(asset_cache, panel.transform(), None, None, 0.001);
    crate::util::tag_render_source(&mut ruler_objects, "calibration_ruler");
    objects.extend(ruler_objects);

    // The invisible collision floor's TOP, the soles and the ruler zero agree.
    let floor = ColliderBuilder::cuboid(10.0, 0.1, 10.0)
        .translation(vector![0.0, -0.1, 0.0])
        .build();
    DebugSceneBuilder::new("debug_calibration")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(-90.0)),
        ))
        .with_physics_geometry(floor)
        .build_with_hooks(
            DebugSceneBuildOptions {
                global_context,
                game_options,
                asset_cache,
                audio_context,
            },
            CalibrationHooks(objects),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Point3, SquareMatrix, Transform};

    #[test]
    fn posed_soles_ground_without_rescaling_the_mesh() {
        let vertex = |y| VertexPositionTextureSkinnedNormal {
            position: vec3(0.0, y, 0.0),
            uv: vec2(0.0, 0.0),
            bone_indices: [0, 1, 0, 0],
            bone_weights: [0.25, 0.75, 0.0, 0.0],
            normal: vec3(0.0, 1.0, 0.0),
        };
        let mut palette = [Matrix4::identity(); SKINNING_PALETTE_SIZE];
        palette[1] = Matrix4::from_translation(vec3(0.0, -2.0, 0.0));
        let (sole, crown) = posed_vertical_bounds(&[vertex(-1.0), vertex(2.0)], &palette);
        assert_eq!((sole, crown), (-2.5, 0.5));
        let grounding = Matrix4::from_translation(vec3(-2.0, -sole, 0.0));
        assert_eq!(
            grounding.transform_point(Point3::new(0.0, sole, 0.0)).y,
            0.0
        );
        assert_eq!(
            grounding.transform_point(Point3::new(0.0, crown, 0.0)).y,
            3.0
        );
    }
}
