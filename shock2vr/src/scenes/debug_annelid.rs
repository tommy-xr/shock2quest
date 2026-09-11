//! Annelid egg pods, one station per authored kind, tripped by walking up to
//! them.
//!
//! The three pods look identical and differ only in their script, so the bug
//! "eggs spawn nothing" is invisible in a real level: a pod that opens and
//! produces nothing is indistinguishable from one that was never near enough
//! to hatch. Here each kind stands alone at a labeled station with a known
//! trip radius, so "walk up, watch what comes out" answers the question for
//! one kind at a time:
//!
//! - Goo pod (`GooEgg`)     - a toxic emitter lobbing venom-stimmed goo shots
//! - Grub pod (`GrubEgg`)   - a crawling annelid
//! - Swarmer pod (`SwarmerEgg`) - a flying annelid swarm
//!
//! Missions trip their pods with an authored "Floor Egg Tripwire" (a once,
//! player-enter tripwire SwitchLinked to the pod). Runtime link authoring has
//! no effect of its own, so this scene stands in for that wiring with a
//! distance check that sends the same `TurnOn` - the pod script sees exactly
//! what a mission tripwire would send.

use cgmath::{Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, vec3};
use dark::{importers::FONT_IMPORTER, properties::PropTemplateId};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::{EntityId, IntoIter, IntoWithId, UniqueView, UniqueViewMut, View};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        GlobalContext, SpawnLocation,
        mission_core::{EffectQueue, MissionCore, PlayerInfo},
    },
    scripts::{Effect, Message, MessagePayload},
    time::Time,
};

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    boxes_to_geometry, max_player_stats, spawn_at,
};

struct EggStation {
    label: &'static str,
    /// The floor variant of each pod: the wall variants differ only in model
    /// orientation, and a floor pod sits where a walker can reach it.
    template_id: i32,
    /// Station centre along +Z; every pod is the same distance ahead.
    z: f32,
}

const STATIONS: &[EggStation] = &[
    EggStation {
        label: "Goo pod - toxic",
        template_id: -1476,
        z: -6.0,
    },
    EggStation {
        label: "Grub pod - crawler",
        template_id: -1335,
        z: 0.0,
    },
    EggStation {
        label: "Swarmer pod - flier",
        template_id: -1332,
        z: 6.0,
    },
];

/// Stations sit this far along -X, the default view forward at an identity
/// spawn yaw (same convention as `debug_melee` / `debug_weapons`).
const STATION_DISTANCE: f32 = 8.0;

/// How close the player must get for a station to hatch. Comfortably smaller
/// than the 6-unit gap between stations, so approaching one pod never trips
/// its neighbours - the whole point of separate stations.
const TRIP_RADIUS: f32 = 2.5;

pub fn create_debug_annelid_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let mut boxes = vec![(
        vec3(0.16, 0.18, 0.16),
        vec3(-STATION_DISTANCE / 2.0, -0.5, 0.0),
        vec3(40.0, 1.0, 40.0),
    )];
    // Low kerbs mark each station's footprint so the trip radius is visible
    // from the spawn rather than something you discover by walking into it.
    for station in STATIONS {
        boxes.push((
            vec3(0.26, 0.30, 0.24),
            vec3(-STATION_DISTANCE, 0.05, station.z),
            vec3(2.0 * TRIP_RADIUS, 0.1, 2.0 * TRIP_RADIUS),
        ));
    }
    let (objects, collider) = boxes_to_geometry(&boxes);

    let mut builder = DebugSceneBuilder::new("debug_annelid")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects {
        builder = builder.add_scene_object(object);
    }

    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    for station in STATIONS {
        let mut label = SceneObject::world_space_text(station.label, font.clone(), 0.0);
        label.set_transform(
            Matrix4::from_translation(vec3(-STATION_DISTANCE, 2.4, station.z))
                * Matrix4::from_nonuniform_scale(
                    0.10 * engine::measure_text_width(&**font, station.label, 1.0),
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
    // A hatching pod is a fight; start the player able to survive all three.
    max_player_stats(&mut core, "debug_annelid");

    println!(
        "[debug_annelid] Three annelid egg pods stand {STATION_DISTANCE} units ahead, one per kind.\n\
         Walk within {TRIP_RADIUS} units of a pod (onto its kerb) to trip it, exactly as a\n\
         mission's Floor Egg Tripwire would. Goo = toxic projectiles, Grub = a crawler,\n\
         Swarmer = a flying swarm. Each pod trips once."
    );

    Box::new(HookedDebugScene::new(core, AnnelidHooks::default()))
}

#[derive(Default)]
struct AnnelidHooks {
    populated: bool,
    /// One entry per station, in `STATIONS` order; `None` until the pod is
    /// spawned, dropped again once that pod has been tripped.
    pods: Vec<Option<EntityId>>,
}

impl DebugSceneHooks for AnnelidHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if !self.populated {
            self.populated = true;
            let spawns = STATIONS
                .iter()
                .map(|station| {
                    spawn_at(
                        station.template_id,
                        Point3::new(-STATION_DISTANCE, 0.6, station.z),
                    )
                })
                .collect();
            core.handle_effects(
                spawns,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
            // Runtime entity ids are assigned at creation and are not stable
            // across runs, so find each pod by the template it came from.
            self.pods = STATIONS
                .iter()
                .map(|station| {
                    core.world.run(|v_template: View<PropTemplateId>| {
                        (&v_template)
                            .iter()
                            .with_id()
                            .find(|(_, template)| template.template_id == station.template_id)
                            .map(|(id, _)| id)
                    })
                })
                .collect();
        }
    }

    /// The trip check belongs here, not beside the populate above: `update` is
    /// what a paused game skips, so a pod cannot hatch behind the pause menu.
    fn before_update(
        &mut self,
        core: &mut MissionCore,
        _time: &Time,
        _input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
        let player_position = core.world.run(|player: UniqueView<PlayerInfo>| player.pos);
        for (index, pod) in self.pods.iter_mut().enumerate() {
            let Some(entity_id) = *pod else { continue };
            // Compared on the floor plane: the pod sits at ankle height and
            // the player's position is their feet, but a VR crouch should not
            // change how close "close" is.
            let station_position = vec3(-STATION_DISTANCE, player_position.y, STATIONS[index].z);
            if (player_position - station_position).magnitude() > TRIP_RADIUS {
                continue;
            }
            // Once, like the authored tripwire: forget the pod so a second
            // pass over the kerb cannot re-hatch it.
            *pod = None;
            core.world.run(|mut effects: UniqueViewMut<EffectQueue>| {
                effects.push(Effect::Send {
                    msg: Message {
                        to: entity_id,
                        payload: MessagePayload::TurnOn { from: entity_id },
                    },
                });
            });
        }
    }
}
