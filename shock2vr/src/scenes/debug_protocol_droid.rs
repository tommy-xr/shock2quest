use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, point3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, entity_creator::CreateEntityOptions},
    scenes::debug_common::{DebugSceneBuildOptions, DebugSceneBuilder},
};

/// Far enough that the droid has to walk in (so the approach is observable),
/// close enough to spot the player from where it stands. `DebugSceneBuilder`
/// spawns the player above the origin, so this is a straight 10-unit walk
/// down +Z - world units, matching the identity root transform below.
const DROID_START_POS: Point3<f32> = point3(0.0, 1.8, 10.0);
const PROTOCOL_DROID_TEMPLATE_ID: i32 = -174;

/// Isolates the protocol droid's self-destruct: it spots the player, closes,
/// and detonates its `Corpse` Incendiary Explosion at melee range, which
/// damages the player standing there.
pub struct DebugProtocolDroidScene;

impl DebugProtocolDroidScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_protocol_droid").with_default_floor();

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let mut scene = builder.build(build_options);

        let droid_entity = scene
            .core_mut()
            .create_entity_with_position(
                asset_cache,
                PROTOCOL_DROID_TEMPLATE_ID,
                DROID_START_POS,
                Quaternion::from_angle_y(Deg(180.0)),
                Matrix4::identity(),
                CreateEntityOptions::default(),
            )
            .entity_id;

        info!("Spawned debug protocol droid entity {droid_entity:?}");

        Box::new(scene)
    }
}
