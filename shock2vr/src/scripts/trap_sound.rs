use dark::properties::PropObjectSound;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessageOrigin, MessagePayload, Script, ScriptRestoreContext, ScriptState,
    ScriptStateError,
};

pub struct TrapSound {
    playing_sounds: Vec<(AudioHandle, MessageOrigin)>,
    stopped_through: Option<MessageOrigin>,
    spatial: bool,
}
impl TrapSound {
    pub fn new() -> TrapSound {
        TrapSound {
            playing_sounds: Vec::new(),
            stopped_through: None,
            spatial: true,
        }
    }

    /// `TrapSoundAmb`: plays at the listener, not the trap - e.g. a Xerxes
    /// announcement triggered from a button a floor away.
    pub fn ambient() -> TrapSound {
        TrapSound {
            spatial: false,
            ..Self::new()
        }
    }
}
impl Script for TrapSound {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        self.handle_message_with_origin(entity_id, world, physics, msg, MessageOrigin::new())
    }

    fn handle_message_with_origin(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
        origin: MessageOrigin,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                // A TurnOff invalidates older queued starts, not just live
                // handles. Equal origins are the same trigger's stop + start
                // fanout; that new narration must still play. Keep ambient
                // speech at the ears: positional workarounds broke Xerxes.
                if self.stopped_through.is_some_and(|stop| origin < stop) {
                    return Effect::NoEffect;
                }
                let v_sound = world.borrow::<View<PropObjectSound>>().unwrap();
                let maybe_trip_sound = v_sound.get(entity_id);
                let handle = AudioHandle::new();
                self.playing_sounds.push((handle.clone(), origin));
                if let Ok(sound) = maybe_trip_sound {
                    Effect::PlaySound {
                        handle,
                        name: sound.name.to_owned(),
                        source: Some(entity_id),
                        // Placement is independent of cancellation:
                        // TrapSound is positional; TrapSoundAmb stays at the ears.
                        spatial: self.spatial,
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::TurnOff { from: _ } => {
                self.stopped_through =
                    Some(self.stopped_through.map_or(origin, |old| old.max(origin)));
                let mut eff = Vec::new();
                self.playing_sounds.retain(|(handle, started)| {
                    // A delayed old TurnOff must not stop a newer activation.
                    if *started <= origin {
                        eff.push(Effect::StopSound {
                            handle: handle.clone(),
                        });
                        false
                    } else {
                        true
                    }
                });
                Effect::Combined { effects: eff }
            }
            _ => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some("shock2vr.trap_sound")
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        // Device handles are transient. Only cancellation must survive a
        // round-trip, alongside TrapDelay's saved pending activation IDs.
        ScriptState::encode(1, &self.stopped_through, "shock2vr.trap_sound")
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.stopped_through = state.decode(1, "shock2vr.trap_sound")?;
        self.playing_sounds.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_on(mut trap: TrapSound) -> Effect {
        let mut world = World::new();
        let entity = world.add_entity(PropObjectSound {
            name: "xxrmsg12".to_owned(),
        });
        trap.handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: entity },
        )
    }

    #[test]
    fn only_the_ambient_variant_plays_at_the_listener() {
        assert!(matches!(
            turn_on(TrapSound::new()),
            Effect::PlaySound { spatial: true, .. }
        ));
        assert!(matches!(
            turn_on(TrapSound::ambient()),
            Effect::PlaySound { spatial: false, .. }
        ));
    }

    #[test]
    fn late_stop_cannot_kill_newer_speech_and_a_fresh_activation_can_restart() {
        let mut world = World::new();
        let entity = world.add_entity(PropObjectSound {
            name: "briefing".into(),
        });
        let physics = PhysicsWorld::new();
        let mut trap = TrapSound::ambient();
        let old = MessageOrigin::new();
        let current = MessageOrigin::new();
        let on = MessagePayload::TurnOn { from: entity };
        let off = MessagePayload::TurnOff { from: entity };
        assert!(matches!(
            trap.handle_message_with_origin(entity, &world, &physics, &on, current),
            Effect::PlaySound { .. }
        ));
        assert!(
            Effect::flatten(vec![
                trap.handle_message_with_origin(entity, &world, &physics, &off, old)
            ])
            .is_empty()
        );
        assert_eq!(
            Effect::flatten(vec![
                trap.handle_message_with_origin(entity, &world, &physics, &off, current)
            ])
            .len(),
            1
        );
        assert!(matches!(
            trap.handle_message_with_origin(entity, &world, &physics, &on, old),
            Effect::NoEffect
        ));
        assert!(matches!(
            trap.handle_message_with_origin(entity, &world, &physics, &on, MessageOrigin::new()),
            Effect::PlaySound { .. }
        ));
    }
}
