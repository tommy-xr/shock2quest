use engine::audio::AudioHandle;
use shipyard::{EntityId, World};

use super::{Effect, Script, script_util::play_environmental_sound};

pub struct CreateSound;
impl CreateSound {
    pub fn new() -> CreateSound {
        CreateSound
    }
}

impl Script for CreateSound {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        play_environmental_sound(world, entity_id, "create", vec![], AudioHandle::new())
    }
}
