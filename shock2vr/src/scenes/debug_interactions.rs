//! A quiet, repeatable rack for comparing VR grips across actual game items.
//! The fixture list also supplies model identities to authoring tools. Items
//! use their normal pickup/wield behavior; this scene does not invent grips.

use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, vec3};
use dark::importers::FONT_IMPORTER;
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View};

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
        label: "Assault rifle",
        template_id: -18,
        model: "ar15_w",
    },
    InteractionFixture {
        label: "Laser pistol",
        template_id: -22,
        model: "laser",
    },
    InteractionFixture {
        label: "EMP rifle",
        template_id: -23,
        model: "empgun",
    },
    InteractionFixture {
        label: "Grenade launcher",
        template_id: -21,
        model: "gren_w",
    },
    InteractionFixture {
        label: "Stasis field generator",
        template_id: -25,
        model: "sfg_w",
    },
    InteractionFixture {
        label: "Viral proliferator",
        template_id: -29,
        model: "viro_w",
    },
    InteractionFixture {
        label: "Electro shock",
        template_id: -24,
        model: "rapier_w",
    },
    InteractionFixture {
        label: "Crystal shard",
        template_id: -28,
        model: "shard_w",
    },
    InteractionFixture {
        label: "Psi sword (shard stand-in)",
        template_id: -2291,
        model: "shard_w",
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
    InteractionFixture {
        label: "Health hypo",
        template_id: -52,
        model: "medpatch",
    },
    InteractionFixture {
        label: "Psi hypo",
        template_id: -57,
        model: "psipatch",
    },
    InteractionFixture {
        label: "Anti-radiation hypo",
        template_id: -54,
        model: "radpatch",
    },
    InteractionFixture {
        label: "Anti-toxin hypo",
        template_id: -53,
        model: "toxpatch",
    },
    InteractionFixture {
        label: "Maintenance tool",
        template_id: -2949,
        model: "techt",
    },
    InteractionFixture {
        label: "French-Epstein device",
        template_id: -1488,
        model: "dingus",
    },
    InteractionFixture {
        label: "Auto-repair unit",
        template_id: -74,
        model: "molean",
    },
    // The base card has no credential: it is a holdable geometry sample.
    InteractionFixture {
        label: "Card grip sample (inert)",
        template_id: -157,
        model: "scipass",
    },
    InteractionFixture {
        label: "Security access card",
        template_id: -2998,
        model: "scipass",
    },
    InteractionFixture {
        label: "Bridge access card",
        template_id: -2594,
        model: "scipass",
    },
    InteractionFixture {
        label: "BrawnBoost implant",
        template_id: -101,
        model: "softred",
    },
    InteractionFixture {
        label: "EndurBoost implant",
        template_id: -102,
        model: "softred",
    },
    InteractionFixture {
        label: "SwiftBoost implant",
        template_id: -103,
        model: "softblue",
    },
    InteractionFixture {
        label: "SmartBoost implant",
        template_id: -104,
        model: "softpurp",
    },
    InteractionFixture {
        label: "LabAssistant implant",
        template_id: -969,
        model: "softgren",
    },
    InteractionFixture {
        label: "RunFast implant (inert)",
        template_id: -1344,
        model: "softblue",
    },
    InteractionFixture {
        label: "ExperTech implant",
        template_id: -1661,
        model: "softblue",
    },
    InteractionFixture {
        label: "WormBlood implant",
        template_id: -106,
        model: "animp01",
    },
    InteractionFixture {
        label: "WormBlend implant (inert)",
        template_id: -762,
        model: "animp02",
    },
    InteractionFixture {
        label: "WormHeart implant",
        template_id: -1334,
        model: "animp03",
    },
    InteractionFixture {
        label: "WormMind implant",
        template_id: -1660,
        model: "animp04",
    },
    InteractionFixture {
        label: "Small worm beaker",
        template_id: -48,
        model: "beakew1",
    },
    InteractionFixture {
        label: "Large worm beaker",
        template_id: -1264,
        model: "beakew2",
    },
    InteractionFixture {
        label: "GamePig",
        template_id: -3864,
        model: "gameboy",
    },
    InteractionFixture {
        label: "Research: monkey brain",
        template_id: -148,
        model: "monbr",
    },
    InteractionFixture {
        label: "Research: hybrid organ",
        template_id: -1095,
        model: "organ",
    },
    InteractionFixture {
        label: "Research: Toxin-A",
        template_id: -1341,
        model: "filter",
    },
    InteractionFixture {
        label: "Chemical: Antimony",
        template_id: -145,
        model: "Sb",
    },
    InteractionFixture {
        label: "Chemical: Vanadium",
        template_id: -139,
        model: "V",
    },
    InteractionFixture {
        label: "Chemical: Fermium",
        template_id: -20,
        model: "Fm",
    },
    InteractionFixture {
        label: "ICE-Pick hack tool",
        template_id: -73,
        model: "icepick",
    },
];

const RACK_HEIGHT: f32 = 1.1;

/// Stations on each side of a clear aisle. Walk along the aisle to reach
/// each object; oversized pickups get the same generous spacing as the rest.
fn station_position(index: usize) -> cgmath::Vector3<f32> {
    let per_side = INTERACTION_FIXTURES.len().div_ceil(2);
    vec3(
        -1.2 - (index % per_side) as f32 * 1.8,
        RACK_HEIGHT,
        if index < per_side { -1.2 } else { 1.2 },
    )
}

pub fn create_debug_interactions_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let per_side = INTERACTION_FIXTURES.len().div_ceil(2);
    let rack_length = (per_side - 1) as f32 * 1.8;
    let mut boxes = vec![(
        vec3(0.13, 0.16, 0.19),
        vec3(-1.2 - rack_length / 2.0, -0.5, 0.0),
        vec3(rack_length + 12.0, 1.0, 18.0),
    )];
    for index in 0..INTERACTION_FIXTURES.len() {
        let p = station_position(index);
        boxes.push((
            vec3(0.23, 0.29, 0.32),
            vec3(p.x, RACK_HEIGHT / 2.0, p.z),
            vec3(1.2, RACK_HEIGHT, 0.7),
        ));
    }
    // Production buttons on small posts provide repeatable glove-feedback targets.
    for x in [1.2, 2.2] {
        boxes.push((
            vec3(0.23, 0.29, 0.32),
            vec3(x, 0.6, -1.4),
            vec3(0.5, 1.2, 0.3),
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
    for (x, text) in [(1.2, "Feedback: locked"), (2.2, "Feedback: ready")] {
        let mut label = SceneObject::world_space_text(text, font.clone(), 0.0);
        label.set_transform(
            Matrix4::from_translation(vec3(x, 1.8, -1.2))
                * Matrix4::from_nonuniform_scale(
                    0.07 * engine::measure_text_width(&**font, text, 1.0),
                    0.07,
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
        let mut spawns: Vec<Effect> = INTERACTION_FIXTURES
            .iter()
            .enumerate()
            .map(|(index, fixture)| {
                let p = station_position(index);
                let mut spawn = spawn_at(fixture.template_id, Point3::new(p.x, p.y + 0.15, p.z));
                // Magazines have no gamesys model: missions choose a cover
                // per instance. The summoned psi sword has no world model;
                // give its rack sample a labeled shard stand-in. Wielding
                // still uses the real psword_h limb model.
                // Resolve both before visual/physics creation.
                if matches!(fixture.template_id, MAGAZINE_TEMPLATE | -2291) {
                    if let Effect::CreateEntity { options, .. } = &mut spawn {
                        options.model_override = Some(fixture.model.to_owned());
                    }
                }
                spawn
            })
            .collect();
        spawns.extend([1.2, 2.2].map(|x| spawn_at(-201, Point3::new(x, 1.3, -1.2))));
        core.handle_effects(
            spawns,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
        // Toxin-A requests Antimony twice. Provide both doses without
        // requiring a scene reset (which also clears research progress).
        let antimony: Vec<_> = core
            .world
            .borrow::<View<dark::properties::PropTemplateId>>()
            .unwrap()
            .iter()
            .with_id()
            .filter(|(_, template)| template.template_id == -145)
            .map(|(entity, _)| entity)
            .collect();
        for entity in antimony {
            core.world
                .add_component(entity, dark::properties::PropStackCount(2));
        }
        let buttons = {
            let templates = core
                .world
                .borrow::<View<dark::properties::PropTemplateId>>()
                .unwrap();
            let positions = core
                .world
                .borrow::<View<dark::properties::PropPosition>>()
                .unwrap();
            (&templates)
                .iter()
                .with_id()
                .filter(|(_, template)| template.template_id == -201)
                .map(|(entity, _)| (entity, positions.get(entity).unwrap().position.x < 1.7))
                .collect::<Vec<_>>()
        };
        for (entity, locked) in buttons {
            core.world.add_component(
                entity,
                (
                    dark::properties::PropLocked(locked),
                    dark::properties::PropSymName(
                        if locked {
                            "Feedback locked button"
                        } else {
                            "Feedback ready button"
                        }
                        .to_owned(),
                    ),
                ),
            );
        }
        // These two gameplay scripts are unimplemented and panic on initialize.
        // Keep their actual geometry available as explicitly labeled inert grip
        // samples here; production implant behavior is untouched.
        let templates = core
            .world
            .borrow::<View<dark::properties::PropTemplateId>>()
            .unwrap();
        for (entity, template) in (&templates).iter().with_id() {
            if matches!(template.template_id, -1344 | -762) {
                core.script_world.remove_entity(entity);
            }
        }
    }
}
