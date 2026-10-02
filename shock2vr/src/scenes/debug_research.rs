//! A fresh research lab using ordinary specimens, chemicals and research UI.

use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, Vector3, vec3};
use dark::{
    importers::FONT_IMPORTER,
    properties::{PropStackCount, PropTemplateId},
};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::{EntityId, IntoIter, IntoWithId, View};

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    boxes_to_geometry, max_player_stats, spawn_at,
};
use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scripts::Effect,
};

const SPECIMENS: &[(i32, &str)] = &[
    (-1095, "Hybrid organ"),
    (-1341, "Anti-Annelid toxin"),
    (-148, "Monkey brain"),
    (-229, "Arachnid organ"),
    (-220, "Rumbler organ"),
    (-28, "Crystal shard"),
];

const CHEMICALS: &[(i32, &str)] = &[
    (-20, "Fermium (Fm)"),
    (-139, "Vanadium (V)"),
    (-143, "Gallium (Ga)"),
    (-145, "Antimony (Sb)"),
    (-135, "Yttrium (Y)"),
    (-130, "Copper (Cu)"),
    (-144, "Californium (Cf)"),
    (-141, "Sodium (Na)"),
    (-138, "Osmium (Os)"),
    (-137, "Iridium (Ir)"),
    (-140, "Arsenic (As)"),
    (-129, "Cesium (Cs)"),
    (-146, "Hassium (Hs)"),
    (-981, "Tellurium (Te)"),
    (-979, "Molybdenum (Mo)"),
    (-131, "Technetium (Tc)"),
    (-980, "Radium (Ra)"),
    (-142, "Barium (Ba)"),
    (-136, "Selenium (Se)"),
];

fn station_position(index: usize, specimen: bool) -> Vector3<f32> {
    vec3(
        -1.5 - index as f32 * 1.6,
        1.15,
        if specimen { -2.0 } else { 2.0 },
    )
}

pub fn create_debug_research_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let mut boxes = vec![(
        vec3(0.12, 0.15, 0.18),
        vec3(-14.0, -0.5, 0.0),
        vec3(36.0, 1.0, 10.0),
    )];
    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    let mut labels = Vec::new();
    for (fixtures, specimen) in [(SPECIMENS, true), (CHEMICALS, false)] {
        for (index, (_, name)) in fixtures.iter().enumerate() {
            let p = station_position(index, specimen);
            boxes.push((
                if specimen {
                    vec3(0.32, 0.24, 0.34)
                } else {
                    vec3(0.18, 0.32, 0.33)
                },
                vec3(p.x, 0.5, p.z),
                vec3(1.35, 1.0, 0.85),
            ));
            let mut label = SceneObject::world_space_text(name, font.clone(), 0.0);
            label.set_transform(
                Matrix4::from_translation(vec3(p.x, 1.75, p.z))
                    * Matrix4::from_angle_y(Deg(if specimen { 0.0 } else { 180.0 }))
                    * Matrix4::from_nonuniform_scale(
                        0.075 * engine::measure_text_width(&**font, name, 1.0),
                        0.075,
                        1.0,
                    )
                    * Matrix4::from_angle_x(Deg(180.0)),
            );
            labels.push(label);
        }
    }
    for (index, text) in [
        "RESEARCH LAB",
        "Research 6 - six specimens - five doses of every chemical",
        "Carry and use a specimen to begin. Use its requested chemical.",
        "Reload level to reset projects and supplies.",
    ]
    .iter()
    .enumerate()
    {
        let mut label = SceneObject::world_space_text(text, font.clone(), 0.0);
        let height = if index == 0 { 0.18 } else { 0.075 };
        label.set_transform(
            Matrix4::from_translation(vec3(-4.0, 3.3 - index as f32 * 0.3, 0.0))
                * Matrix4::from_angle_y(Deg(90.0))
                * Matrix4::from_nonuniform_scale(
                    height * engine::measure_text_width(&**font, text, 1.0),
                    height,
                    1.0,
                )
                * Matrix4::from_angle_x(Deg(180.0)),
        );
        labels.push(label);
    }
    let (objects, collider) = boxes_to_geometry(&boxes);
    let mut builder = DebugSceneBuilder::new("debug_research")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(1.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects.into_iter().chain(labels) {
        builder = builder.add_scene_object(object);
    }
    let mut core = builder.build_core(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });
    max_player_stats(&mut core, "debug_research");
    Box::new(HookedDebugScene::new(
        core,
        ResearchHooks { populated: false },
    ))
}

struct ResearchHooks {
    populated: bool,
}

impl DebugSceneHooks for ResearchHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.populated {
            return;
        }
        self.populated = true;
        let mut spawns = Vec::new();
        for (fixtures, specimen) in [(SPECIMENS, true), (CHEMICALS, false)] {
            for (index, (template, _)) in fixtures.iter().enumerate() {
                let p = station_position(index, specimen);
                spawns.push(spawn_at(*template, Point3::new(p.x, p.y, p.z)));
            }
        }
        core.handle_effects(
            spawns,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
        let chemicals: Vec<_> = core
            .world
            .borrow::<View<PropTemplateId>>()
            .unwrap()
            .iter()
            .with_id()
            .filter(|(_, template)| CHEMICALS.iter().any(|(id, _)| *id == template.template_id))
            .map(|(entity, _)| entity)
            .collect();
        for entity in chemicals {
            core.world.add_component(entity, PropStackCount(5));
        }
    }
}
