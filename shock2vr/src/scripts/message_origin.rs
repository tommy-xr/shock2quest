//! Causal ordering for switch chains, independent of frame or audio-device time.
//! A relay/delay keeps its initiating event's ID. Sound traps use it to reject
//! starts older than a stop, without cancelling the new start from that same
//! trigger (whose inverter and delay can arrive on different frames).
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Deserializer, Serialize};

static NEXT_ORIGIN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct MessageOrigin(u64);

impl MessageOrigin {
    pub fn new() -> Self {
        Self(NEXT_ORIGIN.fetch_add(1, Ordering::Relaxed))
    }
}

impl<'de> Deserialize<'de> for MessageOrigin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = u64::deserialize(deserializer)?;
        let next = id
            .checked_add(1)
            .ok_or_else(|| serde::de::Error::custom("message origin overflow"))?;
        // Loading in a fresh process must not put new activations before a
        // saved cancellation. Never move the allocator backwards on reload.
        NEXT_ORIGIN.fetch_max(next, Ordering::Relaxed);
        Ok(Self(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, time::Duration};

    use dark::properties::{Link, Links, PropDelayTime, PropObjectSound, ToLink, WrappedEntityId};
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        scripts::{Effect, Message, MessagePayload, ScriptWorld},
        time::Time,
    };

    #[test]
    fn restored_origins_precede_new_activations() {
        let restored: MessageOrigin = serde_json::from_str("1000000").unwrap();
        assert!(MessageOrigin::new() > restored);
        assert!(serde_json::from_str::<MessageOrigin>(&u64::MAX.to_string()).is_err());
    }

    #[test]
    fn update_fanout_keeps_one_activation_in_either_link_order() {
        use crate::scripts::{Script, script_util::send_to_all_switch_links};

        struct Once(bool);
        impl Script for Once {
            fn update(
                &mut self,
                entity: shipyard::EntityId,
                world: &World,
                _: &PhysicsWorld,
                _: &Time,
            ) -> Effect {
                if std::mem::replace(&mut self.0, false) {
                    send_to_all_switch_links(world, entity, MessagePayload::TurnOn { from: entity })
                } else {
                    Effect::NoEffect
                }
            }
        }

        for reversed in [false, true] {
            let mut world = World::new();
            let sound = world.add_entity(PropObjectSound {
                name: "briefing".into(),
            });
            let links = |targets: Vec<shipyard::EntityId>| Links {
                to_links: targets
                    .into_iter()
                    .map(|id| ToLink {
                        to_template_id: 1,
                        to_entity_id: Some(WrappedEntityId(id)),
                        link: Link::SwitchLink,
                    })
                    .collect(),
            };
            let delay = world.add_entity((
                PropDelayTime {
                    delay: Duration::from_secs(1),
                },
                links(vec![sound]),
            ));
            let inverter = world.add_entity(links(vec![sound]));
            let entry = world.add_entity(links(if reversed {
                vec![inverter, delay]
            } else {
                vec![delay, inverter]
            }));
            let mut scripts = ScriptWorld::new();
            scripts.add_entity(sound, "TrapSoundAmb");
            scripts.add_entity(delay, "TrapDelay");
            scripts.add_entity(inverter, "TrapInverter");
            scripts.add_entity2(entry, Box::new(Once(true)));
            let physics = PhysicsWorld::new();
            let mut plays = 0;
            for frame in 0..20 {
                plays += scripts
                    .update(
                        &world,
                        &physics,
                        &Time {
                            elapsed: Duration::from_millis(100),
                            total: Duration::from_millis(frame * 100),
                        },
                    )
                    .iter()
                    .filter(|effect| matches!(effect, Effect::PlaySound { .. }))
                    .count();
            }
            assert_eq!(
                plays, 1,
                "stop/start from one update must share an activation, reversed={reversed}"
            );
        }
    }

    #[test]
    fn stop_invalidates_old_delay_but_not_its_own_fanout_even_after_save_load() {
        for save_load in [false, true] {
            let mut world = World::new();
            let sound = world.add_entity(PropObjectSound {
                name: "briefing".into(),
            });
            let link = |targets: Vec<shipyard::EntityId>| Links {
                to_links: targets
                    .into_iter()
                    .map(|target| ToLink {
                        to_template_id: 1,
                        to_entity_id: Some(WrappedEntityId(target)),
                        link: Link::SwitchLink,
                    })
                    .collect(),
            };
            let old_delay = world.add_entity((
                PropDelayTime {
                    delay: Duration::from_secs(5),
                },
                link(vec![sound]),
            ));
            let new_delay = world.add_entity((
                PropDelayTime {
                    delay: Duration::from_secs(1),
                },
                link(vec![sound]),
            ));
            let inverter = world.add_entity(link(vec![sound]));
            let entry = world.add_entity((
                PropDelayTime {
                    delay: Duration::ZERO,
                },
                link(vec![inverter, new_delay]),
            ));
            let entities = [
                (sound, "TrapSoundAmb"),
                (old_delay, "TrapDelay"),
                (new_delay, "TrapDelay"),
                (inverter, "TrapInverter"),
                (entry, "TrapDelay"),
            ];
            let mut scripts = ScriptWorld::new();
            for (id, script) in entities {
                scripts.add_entity(id, script);
            }
            let physics = PhysicsWorld::new();
            let mut plays = Vec::new();
            for frame in 0..70 {
                if frame == 0 || frame == 1 {
                    scripts.dispatch(Message {
                        to: if frame == 0 { old_delay } else { entry },
                        // Reuse the same sender deliberately: entity identity
                        // cannot distinguish an old activation from a new one.
                        payload: MessagePayload::TurnOn { from: entry },
                    });
                }
                let effects = scripts.update(
                    &world,
                    &physics,
                    &Time {
                        elapsed: Duration::from_millis(100),
                        total: Duration::from_millis(frame * 100),
                    },
                );
                for effect in effects {
                    if let Effect::PlaySound { spatial, .. } = effect {
                        plays.push((frame, spatial));
                    }
                }
                if save_load && frame == 6 {
                    let bytes = serde_json::to_vec(&scripts.save_states().unwrap()).unwrap();
                    let saved: Vec<crate::scripts::SavedScriptState> =
                        serde_json::from_slice(&bytes).unwrap();
                    scripts = ScriptWorld::new();
                    for (id, script) in entities {
                        scripts.add_entity(id, script);
                    }
                    scripts
                        .restore_states(
                            &saved,
                            &entities
                                .map(|(id, _)| (id, id))
                                .into_iter()
                                .collect::<HashMap<_, _>>(),
                        )
                        .unwrap();
                }
            }
            assert_eq!(
                plays.len(),
                1,
                "only the newer briefing should play: {plays:?}, save/load={save_load}"
            );
            assert!(!plays[0].1, "ambient speech must stay listener-relative");
            assert!(
                plays[0].0 < 20,
                "the new briefing, not the stale 5-second timer, must play"
            );
        }
    }
}
