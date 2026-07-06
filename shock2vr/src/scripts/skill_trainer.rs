//! Skill-training machines from the station recruit deck and the decks beyond.
//!
//! The retail game gates these behind cyber modules and a full skill/stat
//! economy (weapon proficiencies, tech/repair/hack skills, STR/END/AGI/CYB
//! stats). That economy does not exist yet (issue #424), so only the psi
//! trainer - which has a real system to hook into (`crate::psi`) - does
//! something: on frob it trains the player in a psi power. Granting a power is
//! idempotent (it unions into the known-power set), so a repeat frob is a
//! harmless no-op. The weapon/tech/stats trainers log and no-op until their
//! backing systems land, replacing the previous `UnimplementedScript` warning
//! spam with an intentional, named placeholder.

use shipyard::{EntityId, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// `PsiHeal` (Cerebro-Stimulated Regeneration) - the tier-2 power the psi
/// trainer teaches.
const PSIHEAL_TEMPLATE_ID: i32 = -1017;

#[derive(Debug, Clone, Copy)]
pub enum TrainerKind {
    Weapon,
    Psi,
    Tech,
    Stats,
}

pub struct SkillTrainerScript {
    kind: TrainerKind,
}

impl SkillTrainerScript {
    pub fn new(kind: TrainerKind) -> SkillTrainerScript {
        SkillTrainerScript { kind }
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
        match msg {
            MessagePayload::Frob => match self.kind {
                TrainerKind::Psi => Effect::GrantPsiPower {
                    template_id: PSIHEAL_TEMPLATE_ID,
                },
                TrainerKind::Weapon | TrainerKind::Tech | TrainerKind::Stats => {
                    info!(
                        "{:?} skill trainer frobbed - no skill/stat system yet (#424)",
                        self.kind
                    );
                    Effect::NoEffect
                }
            },
            _ => Effect::NoEffect,
        }
    }
}
