//! Skill-training machines from the station recruit deck and the decks beyond.
//!
//! The retail game gates these behind cyber modules and a full skill/stat
//! economy (weapon proficiencies, tech/repair/hack skills, STR/END/AGI/CYB
//! stats, trained psi powers). None of that economy exists yet - and learned
//! psi powers aren't even persisted across level loads (`PlayerPsiKnownPowers`
//! is rebuilt from the player template each load) - so a trainer can't yet
//! grant anything durable or costed. Rather than vend free, non-persistent
//! upgrades, this replaces the previous `UnimplementedScript` warning spam with
//! an intentional, named no-op placeholder until those systems land (#424).

use shipyard::{EntityId, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct SkillTrainerScript {
    /// The trainer's script name (e.g. "psitrainer"), for the frob log.
    name: &'static str,
}

impl SkillTrainerScript {
    pub fn new(name: &'static str) -> SkillTrainerScript {
        SkillTrainerScript { name }
    }
}

impl Script for SkillTrainerScript {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if let MessagePayload::Frob = msg {
            info!(
                "'{}' skill trainer frobbed - no skill/stat economy yet (#424)",
                self.name
            );
        }
        Effect::NoEffect
    }
}
