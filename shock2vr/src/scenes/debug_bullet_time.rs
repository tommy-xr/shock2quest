//! Bullet-time showcase: a shotgun hybrid stands down a range from a player
//! holding a Speed Boost hypo in the left hand and a pistol in the right, with
//! a second pistol in the left thigh holster and every stat at cap. Both
//! pistols carry a lit laser sight.
//!
//! Pull the left trigger to use the hypo: the world drops to 0.25 speed (Slow
//! world mode, the default), so the hybrid's slugs and the pistols' shots
//! travel visibly with trails and can be sidestepped. Draw the holstered pistol
//! to dual wield. Turn on the `cheat` dev param to survive the slugs you don't
//! dodge.

use cgmath::{Deg, Point3, Quaternion, Rotation3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::{DebugItemTemplate, DebuggableScene, GameScene},
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scripts::Effect,
    vr_config::Handedness,
    weapon_upgrades::{UpgradeSource, WeaponAccessory, WeaponUpgrade, WeaponUpgrades},
};

use super::debug_common::{
    DebugBox, DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, boxes_to_geometry,
    max_player_stats, spawn_at_oriented,
};

const OG_SHOTGUN: i32 = -175;
const SPEED_BOOST: i32 = -58;
const PISTOL: i32 = -17;
/// The left thigh holster.
const LEFT_HOLSTER: usize = 1;

/// Distance down the range (along -X, the spawn's forward) to the hybrid:
/// outside its minimum range, so it opens fire rather than closing in.
const SHOOTER_DISTANCE: f32 = 12.0;

pub fn create_debug_bullet_time_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let pillar = vec3(0.4, 0.38, 0.36);
    let boxes: &[DebugBox] = &[
        // floor
        (
            vec3(0.18, 0.18, 0.22),
            vec3(-6.0, -0.5, 0.0),
            vec3(30.0, 1.0, 20.0),
        ),
        // back wall, behind the hybrid
        (
            vec3(0.45, 0.45, 0.5),
            vec3(-SHOOTER_DISTANCE - 4.0, 3.0, 0.0),
            vec3(1.0, 6.0, 20.0),
        ),
        // pillars either side of the range, for depth as shots cross it
        (pillar, vec3(-4.0, 2.0, -3.5), vec3(1.0, 4.0, 1.0)),
        (pillar, vec3(-4.0, 2.0, 3.5), vec3(1.0, 4.0, 1.0)),
        (pillar, vec3(-9.0, 2.0, -3.5), vec3(1.0, 4.0, 1.0)),
        (pillar, vec3(-9.0, 2.0, 3.5), vec3(1.0, 4.0, 1.0)),
    ];
    let (scene_objects, collider) = boxes_to_geometry(boxes);
    let mut builder = DebugSceneBuilder::new("debug_bullet_time")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for scene_object in scene_objects {
        builder = builder.add_scene_object(scene_object);
    }
    println!(
        "[debug_bullet_time] Speed Boost hypo in the left hand, pistols in the right hand and \
         left holster, a shotgun hybrid {SHOOTER_DISTANCE} units ahead. Use the hypo with the \
         left trigger, draw the second pistol, then dodge and return fire. Set the `cheat` dev \
         param to survive."
    );
    builder.build_with_hooks(
        DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        },
        BulletTimeHooks { populated: false },
    )
}

struct BulletTimeHooks {
    populated: bool,
}

impl DebugSceneHooks for BulletTimeHooks {
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
        max_player_stats(core, "debug_bullet_time");
        // In VR each hand keeps its item only while its grip is squeezed.
        let loadout = [
            (SPEED_BOOST, Some(Handedness::Left)),
            (PISTOL, Some(Handedness::Right)),
            (PISTOL, None),
        ];
        for (template_id, hand) in loadout {
            let entity_id = match hand {
                Some(hand) => core
                    .spawn_item_for_player(
                        asset_cache,
                        &DebugItemTemplate::Id(template_id),
                        Some(hand),
                    )
                    .and_then(|item| {
                        core.resolve_entity_id(item.entity_id)
                            .ok_or_else(|| "spawned item vanished".to_string())
                    }),
                None => core.spawn_into_backpack(asset_cache, template_id),
            };
            let entity_id = match entity_id {
                Ok(entity_id) => entity_id,
                Err(err) => {
                    tracing::warn!("[debug_bullet_time] could not provision {template_id}: {err}");
                    continue;
                }
            };
            if template_id == PISTOL {
                core.world.add_component(entity_id, laser_sight());
            }
            if hand.is_none() {
                core.holster(entity_id, LEFT_HOLSTER);
            }
        }
        core.handle_effects(
            vec![spawn_at_oriented(
                OG_SHOTGUN,
                Point3::new(-SHOOTER_DISTANCE, 1.0, 0.0),
                // Face the player (+X).
                Quaternion::from_angle_y(Deg(90.0)),
            )],
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
    }
}

/// An installed, switched-on laser sight.
fn laser_sight() -> WeaponUpgrades {
    let mut upgrades = WeaponUpgrades::default()
        .with_upgrade(
            WeaponUpgrade::Laser,
            &[WeaponUpgrade::Laser],
            UpgradeSource::Device,
            0,
        )
        .expect("a bare weapon takes a laser");
    upgrades
        .set_accessory_enabled(WeaponAccessory::Laser, true)
        .expect("the laser is installed");
    upgrades
}
