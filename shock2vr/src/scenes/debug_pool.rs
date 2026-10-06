//! Real mission pools without authored entities, for swimming and hand exits.
//!
//! Keep the mission's spatial cells as well as its mesh: a synthetic water
//! plane alone would render a pool without enabling water locomotion.

use std::collections::HashSet;

use cgmath::{Deg, Quaternion, Rotation3, Vector3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{
        GlobalContext, Mission, SpawnLocation,
        entity_populator::empty_entity_populator::EmptyEntityPopulator,
    },
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
};

pub(super) struct PoolScene {
    name: &'static str,
    source: &'static str,
    position: Vector3<f32>,
    yaw: f32,
}

pub(super) const REC_POOL: PoolScene = PoolScene {
    name: "debug_rec_pool",
    source: "rec1.mis",
    position: Vector3::new(12.35, -6.25, -231.1),
    yaw: 90.0,
};

pub(super) const MANY_POOL: PoolScene = PoolScene {
    name: "debug_many_pool",
    source: "many.mis",
    // The bowl slopes up to the rim: spawning beside it would embed the
    // standing capsule in the slope. Start in deep water and swim toward it.
    position: Vector3::new(41.3, 9.3, 45.0),
    yaw: 0.0,
};

impl PoolScene {
    pub(super) fn create(
        &self,
        global: &GlobalContext,
        options: &GameOptions,
        assets: &mut AssetCache,
        audio: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let mut level = Mission::parse(
            assets.asset_paths(),
            assets.base_path(),
            self.source,
            global,
        );
        // Room scripts are instantiated separately from the entity populator;
        // omit those triggers too, while retaining the spatial water cells.
        level.room_database.rooms.clear();
        let lights: HashSet<_> = level
            .cells
            .iter()
            .flat_map(|cell| &cell.lights)
            .flat_map(|light| &light.switchable_layers)
            .map(|layer| layer.light_number)
            .collect();
        // Build under the debug name so restarting/launching from Developer
        // returns to this pool instead of loading the populated campaign map.
        let mut mission = Mission::build(
            level,
            self.name.to_owned(),
            assets,
            audio,
            global,
            SpawnLocation::PositionRotation(self.position, Quaternion::from_angle_y(Deg(self.yaw))),
            QuestInfo::with_difficulty(options.difficulty),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            options,
        );
        // No light scripts remain to enable their baked layers. Freeze those
        // lights on for inspection, without adding actors or changing the
        // global lighting settings of subsequent campaign missions.
        if let Some(controller) = &mut mission.mission_core.animated_lightmaps {
            for light in lights {
                controller.set_light_intensity(light, 1.0);
            }
            controller.flush();
        }
        Box::new(mission)
    }
}
