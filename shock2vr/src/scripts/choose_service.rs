use dark::properties::{PropDestLevel, PropDestLoc, PropService};
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::career::Career;
use crate::physics::PhysicsWorld;

use super::{Effect, GlobalEffect, MessagePayload, Script};

/// Records the service branch (Marine/Navy/OSA) the player enlisted in on the
/// recruitment intro (`earth.mis`), then sends them on to the recruit station.
///
/// The three career-choice markers in `earth.mis` (`SendToMarines` and its two
/// unnamed siblings) carry this script plus a `P$Service` value (0 = Marine,
/// 1 = Navy, 2 = OSA) and a `P$DestLevel`/`P$DestLoc` transition target. A
/// tripwire `TurnOn`s the marker matching the branch the player walked into; on
/// that message this script persists the chosen career as a quest bit - so the
/// career loadout applies to the player on every subsequent mission load (see
/// `crate::career` + `MissionCore::load`) - and then transitions to the marker's
/// own destination (the station recruit deck). The three branches are mutually
/// exclusive: selecting one clears the others.
pub struct ChooseServiceScript {}
impl ChooseServiceScript {
    pub fn new() -> ChooseServiceScript {
        ChooseServiceScript {}
    }
}

impl Script for ChooseServiceScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_service = world.borrow::<View<PropService>>().unwrap();
                let service = v_service.get(entity_id).map(|s| s.0).unwrap_or(0);
                let career = Career::from_service(service);
                info!(
                    "Recruitment: enlisted in {:?} (service {})",
                    career, service
                );

                let mut effects = career.select_effects();

                // Continue to this marker's own destination (the recruit
                // station); the marker with no destination just records the
                // career. A missing dest loc uses the level's default spawn.
                let v_dest_level = world.borrow::<View<PropDestLevel>>().unwrap();
                if let Ok(dest_level) = v_dest_level.get(entity_id) {
                    let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
                    let dest_loc = v_dest_loc.get(entity_id).ok().map(|l| l.0);
                    effects.push(Effect::GlobalEffect(GlobalEffect::TransitionLevel {
                        level_file: format!("{}.mis", dest_level.0),
                        loc: dest_loc,
                        entities_to_trigger: vec![],
                    }));
                }

                Effect::Multiple(effects)
            }
            _ => Effect::NoEffect,
        }
    }
}
