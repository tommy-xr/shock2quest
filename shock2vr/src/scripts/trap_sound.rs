use dark::properties::PropObjectSound;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, GlobalEffect, MessagePayload, Script};

pub struct TrapSound {
    playing_sounds: Vec<AudioHandle>,
}
impl TrapSound {
    pub fn new() -> TrapSound {
        TrapSound {
            playing_sounds: Vec::new(),
        }
    }
}
impl Script for TrapSound {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_sound = world.borrow::<View<PropObjectSound>>().unwrap();
                let maybe_trip_sound = v_sound.get(entity_id);
                let handle = AudioHandle::new();
                self.playing_sounds.push(handle.clone());
                if let Ok(sound) = maybe_trip_sound {
                    Effect::Combined {
                        effects: vec![
                            Effect::PlaySound {
                                handle,
                                name: sound.name.to_owned(),
                                source: Some(entity_id),
                                // TrapSound(Amb) narrations are anchored at their
                                // authored station - a stale montage segment two rooms
                                // away should be distant, not at the player's ears.
                                spatial: true,
                            },
                            // The narration's transcript, when the install has
                            // one (see `GlobalEffect::ShowSubtitle`). The 25AE
                            // subtitle database covers exactly this script's
                            // voice-overs (the trg/brief narrations); ordinary
                            // sound traps simply miss the lookup.
                            Effect::GlobalEffect(GlobalEffect::ShowSubtitle {
                                audio_schema: sound.name.to_owned(),
                            }),
                        ],
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::TurnOff { from: _ } => {
                let mut eff = Vec::new();
                for handle in &self.playing_sounds {
                    eff.push(Effect::StopSound {
                        handle: handle.clone(),
                    });
                }
                self.playing_sounds.clear();
                Effect::Combined { effects: eff }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::World;

    #[test]
    fn turn_on_plays_the_sound_and_requests_its_subtitle() {
        let mut world = World::new();
        let entity_id = world.add_entity((PropObjectSound {
            name: "trg0001".to_owned(),
        },));
        let physics = PhysicsWorld::new();

        let effect = TrapSound::new().handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        let flattened = Effect::flatten(vec![effect]);
        assert!(
            flattened.iter().any(
                |effect| matches!(effect, Effect::PlaySound { name, .. } if name == "trg0001")
            )
        );
        assert!(flattened.iter().any(|effect| matches!(
            effect,
            Effect::GlobalEffect(GlobalEffect::ShowSubtitle { audio_schema }) if audio_schema == "trg0001"
        )));
    }
}
