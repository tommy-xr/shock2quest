use cgmath::{vec3, Quaternion, Rotation3};

use dark::SCALE_FACTOR;
use shipyard::{UniqueView, World};

use crate::{scripts::Effect, PlayerInfo};

use super::Command;
// SpawnItemCommand
#[derive(Debug)]
pub struct MoveInventoryCommand {
    head_rotation: Quaternion<f32>,
}

impl MoveInventoryCommand {
    pub fn new(head_rotation: Quaternion<f32>) -> MoveInventoryCommand {
        MoveInventoryCommand { head_rotation }
    }
}

impl Command for MoveInventoryCommand {
    fn execute(&self, world: &World) -> Effect {
        let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();

        let pos = player.pos;
        let rot = player.rotation * self.head_rotation;

        let forward = rot * vec3(0.0, 0.5 / SCALE_FACTOR, -8.0 / SCALE_FACTOR);

        Effect::PositionInventory {
            position: pos + forward,
            rotation: Quaternion::from_angle_y(cgmath::Deg(180.0)) * rot,
        }
    }
}
