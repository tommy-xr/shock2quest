//! A quiet, repeatable rack for comparing VR grips across actual game items.
//! The fixture list also supplies model identities to authoring tools. Items
//! use their normal pickup/wield behavior; this scene does not invent grips.

use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, vec3};
use dark::importers::FONT_IMPORTER;
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scripts::{Effect, GlobalEffect},
};

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    boxes_to_geometry, max_player_stats, spawn_at,
};

/// Stable asset identities shared by the rack and VR model authoring tools.
/// `model` names the pickup; wielding may select its authored first-person model.
pub struct InteractionFixture {
    pub label: &'static str,
    pub template_id: i32,
    pub model: &'static str,
}

const MAGAZINE_TEMPLATE: i32 = -1255;

pub const INTERACTION_FIXTURES: &[InteractionFixture] = &[
    InteractionFixture {
        label: "Coffee mug",
        template_id: -1221,
        model: "mug",
    },
    InteractionFixture {
        label: "Magazine",
        template_id: MAGAZINE_TEMPLATE,
        model: "magci",
    },
    InteractionFixture {
        label: "Basketball",
        template_id: -4286,
        model: "hamball",
    },
    InteractionFixture {
        label: "Wrench",
        template_id: -928,
        model: "wrench_w",
    },
    InteractionFixture {
        label: "Pistol",
        template_id: -17,
        model: "atek_w",
    },
    InteractionFixture {
        label: "Shotgun",
        template_id: -19,
        model: "sg_w",
    },
    InteractionFixture {
        label: "Fusion cannon",
        template_id: -26,
        model: "fsn_w",
    },
    InteractionFixture {
        label: "Worm launcher",
        template_id: -27,
        model: "al_w",
    },
    InteractionFixture {
        label: "Ammo clip",
        template_id: -1358,
        model: "ammoss",
    },
    InteractionFixture {
        label: "Psi amp",
        template_id: -247,
        model: "amp_w",
    },
];

const RACK_HEIGHT: f32 = 1.1;

/// Five stations on each side of a clear aisle. Walk along the aisle to reach
/// each object; oversized pickups get the same generous spacing as the rest.
fn station_position(index: usize) -> cgmath::Vector3<f32> {
    vec3(
        -1.2 - (index % 5) as f32 * 1.8,
        RACK_HEIGHT,
        if index < 5 { -1.2 } else { 1.2 },
    )
}

pub fn create_debug_interactions_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let mut boxes = vec![(
        vec3(0.13, 0.16, 0.19),
        vec3(-4.0, -0.5, 0.0),
        vec3(24.0, 1.0, 18.0),
    )];
    for index in 0..INTERACTION_FIXTURES.len() {
        let p = station_position(index);
        boxes.push((
            vec3(0.23, 0.29, 0.32),
            vec3(p.x, RACK_HEIGHT / 2.0, p.z),
            vec3(1.2, RACK_HEIGHT, 0.7),
        ));
    }
    let (objects, collider) = boxes_to_geometry(&boxes);
    let mut builder = DebugSceneBuilder::new("debug_interactions")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects {
        builder = builder.add_scene_object(object);
    }
    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    for (index, fixture) in INTERACTION_FIXTURES.iter().enumerate() {
        let p = station_position(index);
        let mut label = SceneObject::world_space_text(fixture.label, font.clone(), 0.0);
        // Signs face the aisle. The glyphs use the shared canvas-y-down text
        // geometry in flat and VR, with its one boundary conversion here.
        label.set_transform(
            Matrix4::from_translation(vec3(p.x, 1.85, p.z))
                * Matrix4::from_angle_y(Deg(if p.z < 0.0 { 0.0 } else { 180.0 }))
                * Matrix4::from_nonuniform_scale(
                    0.10 * engine::measure_text_width(&**font, fixture.label, 1.0),
                    0.10,
                    1.0,
                )
                * Matrix4::from_angle_x(Deg(180.0)),
        );
        builder = builder.add_scene_object(label);
    }
    let mut core = builder.build_core(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });
    max_player_stats(&mut core, "debug_interactions");
    Box::new(HookedDebugScene::new(
        core,
        InteractionHooks { populated: false },
    ))
}

struct InteractionHooks {
    populated: bool,
}

impl DebugSceneHooks for InteractionHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        // A rack reset is a fresh debug scene, not a saved .mis reload (there
        // is no debug_interactions.mis to parse). Reuse the developer launcher
        // so held items and moved/consumed fixtures are discarded together.
        for effect in effects {
            if matches!(effect, Effect::GlobalEffect(GlobalEffect::TestReload)) {
                *effect = Effect::GlobalEffect(GlobalEffect::LaunchDebugScene {
                    name: "debug_interactions".to_owned(),
                });
            }
        }
        if self.populated {
            return;
        }
        self.populated = true;
        let spawns = INTERACTION_FIXTURES
            .iter()
            .enumerate()
            .map(|(index, fixture)| {
                let p = station_position(index);
                let mut spawn = spawn_at(fixture.template_id, Point3::new(p.x, p.y + 0.15, p.z));
                // Magazines have no gamesys model: missions choose a cover
                // per instance. Resolve it before visual/physics creation.
                if fixture.template_id == MAGAZINE_TEMPLATE {
                    if let Effect::CreateEntity { options, .. } = &mut spawn {
                        options.model_override = Some(fixture.model.to_owned());
                    }
                }
                spawn
            })
            .collect();
        core.handle_effects(
            spawns,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
    }
}
