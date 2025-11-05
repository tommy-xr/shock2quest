use cgmath::{vec3, Matrix4, Quaternion, SquareMatrix};

use dark::SCALE_FACTOR;
use shipyard::{UniqueView, World};

use crate::{
    mission::entity_creator::CreateEntityOptions, scripts::Effect, util::vec3_to_point3, PlayerInfo,
};

use super::Command;
// SpawnItemCommand
#[derive(Debug)]
pub struct SpawnItemCommand {
    head_rotation: Quaternion<f32>,
}

impl SpawnItemCommand {
    pub fn new(head_rotation: Quaternion<f32>) -> SpawnItemCommand {
        SpawnItemCommand { head_rotation }
    }
}

impl Command for SpawnItemCommand {
    fn execute(&self, world: &World) -> Effect {
        let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();

        let pos = vec3_to_point3(player.pos);
        let rot = player.rotation * self.head_rotation;

        let forward = rot * vec3(0.0, 2.5 / SCALE_FACTOR, -10.0 / SCALE_FACTOR);

        Effect::CreateEntity {
            // Pistol: -17
            // Laser: -22,
            // Wrench: -928
            // assault flash: -2653,
            // vent parts: -1998, -1999, -2000
            // template_id: -22,
            // grunt og-pipe: -397
            // grunt og-shotgun: -175
            // grunt og-grenade: -176
            // monkey - red: -1432
            template_id: -1432,
            position: pos + forward,
            orientation: rot,
            root_transform: Matrix4::identity(),
            options: CreateEntityOptions::default(),
        }
    }
}
