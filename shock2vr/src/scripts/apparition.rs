//! Script `Apparition`: the ghostly crew replays scattered around the ship
//! (e.g. the Grassi apparition in medsci1). The entity is authored invisible
//! (`PropHasRefs(false)`, translucent ghost model); its scripted sequence
//! (AISignalResponse / AIWatchObj actions) brackets the performance with
//! ScriptMessage("ApparBegin") / ScriptMessage("ApparEnd"), which reach this
//! script as signals: materialize (+ play the authored apparition sound) and
//! vanish again.
use dark::properties::PropObjectSound;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct Apparition {
    audio_handle: AudioHandle,
}

impl Apparition {
    pub fn new() -> Apparition {
        Apparition {
            audio_handle: AudioHandle::new(),
        }
    }
}

impl Script for Apparition {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::Signal { name } = msg else {
            return Effect::NoEffect;
        };
        match name.as_str() {
            "ApparBegin" => {
                info!("apparition {:?} materializing", entity_id);
                let v_sound = world.borrow::<View<PropObjectSound>>().unwrap();
                let sound_effect = match v_sound.get(entity_id) {
                    Ok(sound) => Effect::PlaySound {
                        handle: self.audio_handle.clone(),
                        source: Some(entity_id),
                        name: sound.name.clone(),
                        // The remaster's global ND-apparitions.dml/ND_Apparition
                        // localizes ObjSoundName at the ghost, rather than the ears.
                        spatial: true,
                    },
                    Err(_) => Effect::NoEffect,
                };
                Effect::Combined {
                    effects: vec![
                        Effect::SetVisibility {
                            entity_id,
                            visible: true,
                        },
                        sound_effect,
                    ],
                }
            }
            "ApparEnd" => {
                info!("apparition {:?} vanishing", entity_id);
                Effect::SetVisibility {
                    entity_id,
                    visible: false,
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apparition_speech_is_spatial_at_its_owner_without_changing_visibility() {
        let mut world = World::new();
        let ghost = world.add_entity((PropObjectSound {
            name: "cs2201".into(),
        },));
        let mut script = Apparition::new();
        let physics = PhysicsWorld::new();
        let begin = script.handle_message(
            ghost,
            &world,
            &physics,
            &MessagePayload::Signal {
                name: "ApparBegin".into(),
            },
        );
        let effects = Effect::flatten(vec![begin]);
        assert_eq!(effects.len(), 2);
        assert!(
            matches!(&effects[0], Effect::SetVisibility { entity_id, visible: true } if *entity_id == ghost)
        );
        assert!(
            matches!(&effects[1], Effect::PlaySound { source: Some(source), name, spatial: true, .. } if *source == ghost && name == "cs2201")
        );
        assert!(
            matches!(script.handle_message(ghost, &world, &physics, &MessagePayload::Signal { name: "ApparEnd".into() }), Effect::SetVisibility { entity_id, visible: false } if entity_id == ghost)
        );
    }
}
