use dark::properties::PropMaxHitPoints;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::{get_first_entity_by_name, send_to_all_switch_links},
};

const STATE_KEY: &str = "shock2vr.shodan_head";

/// Retail `ShodanHead` Damage handler (25th Anniversary allobjs DLL,
/// 0x180024d00): strict 0.66/0.33 HP ratios, independent Boom1/Boom2 latches,
/// and TurnOn broadcasts on the named screen routers' SwitchLinks. The
/// authored delay/tweq/destroy chains own what happens to each screen.
#[derive(Default, Serialize, Deserialize)]
pub struct ShodanHead {
    fired: [bool; 2],
}

impl Script for ShodanHead {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::HitPointsChanged { previous, current } = msg else {
            return Effect::NoEffect;
        };
        if current >= previous {
            return Effect::NoEffect;
        }
        let maximums = world.borrow::<View<PropMaxHitPoints>>().unwrap();
        let Ok(maximum) = maximums.get(entity_id) else {
            return Effect::NoEffect;
        };
        if maximum.hit_points == 0 {
            return Effect::NoEffect;
        }
        let ratio = *current as f32 / maximum.hit_points as f32;
        let mut effects = Vec::new();
        for (index, (threshold, name)) in [(0.66, "Screen1Trap"), (0.33, "Screen2Trap")]
            .into_iter()
            .enumerate()
        {
            if !self.fired[index] && ratio < threshold {
                // Retail consumes BoomN even if the named router is absent.
                self.fired[index] = true;
                if let Some(router) = get_first_entity_by_name(world, name) {
                    effects.push(send_to_all_switch_links(
                        world,
                        router,
                        MessagePayload::TurnOn { from: router },
                    ));
                }
            }
        }
        Effect::combine(effects)
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = state.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use dark::properties::{Link, Links, PropSymName, ToLink, WrappedEntityId};

    use super::*;

    fn fixture(maximum: u32) -> (World, EntityId, [EntityId; 2]) {
        let mut world = World::new();
        let head = world.add_entity((PropMaxHitPoints {
            hit_points: maximum,
        },));
        let targets = [world.add_entity(()), world.add_entity(())];
        for (name, target) in ["Screen1Trap", "Screen2Trap"].into_iter().zip(targets) {
            world.add_entity((
                PropSymName(name.to_owned()),
                Links {
                    to_links: vec![ToLink {
                        to_template_id: 99,
                        to_entity_id: Some(WrappedEntityId(target)),
                        link: Link::SwitchLink,
                    }],
                },
            ));
        }
        (world, head, targets)
    }

    fn hit(
        script: &mut ShodanHead,
        world: &World,
        head: EntityId,
        from: i32,
        to: i32,
    ) -> Vec<EntityId> {
        let effect = script.handle_message(
            head,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::HitPointsChanged {
                previous: from,
                current: to,
            },
        );
        Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg } if matches!(msg.payload, MessagePayload::TurnOn { .. }) => {
                    Some(msg.to)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn authored_125_hp_strict_thresholds_and_once_only() {
        let (world, head, targets) = fixture(125);
        let mut script = ShodanHead::default();
        assert!(hit(&mut script, &world, head, 125, 83).is_empty());
        assert_eq!(hit(&mut script, &world, head, 83, 82), vec![targets[0]]);
        assert!(hit(&mut script, &world, head, 82, 42).is_empty());
        assert_eq!(hit(&mut script, &world, head, 42, 41), vec![targets[1]]);
        assert!(hit(&mut script, &world, head, 41, 0).is_empty());
    }

    #[test]
    fn exact_ratios_do_not_fire_but_one_lethal_hit_fires_both() {
        let (world, head, targets) = fixture(100);
        let mut script = ShodanHead::default();
        assert!(hit(&mut script, &world, head, 100, 66).is_empty());
        assert_eq!(hit(&mut script, &world, head, 66, 33), vec![targets[0]]);
        assert_eq!(hit(&mut script, &world, head, 33, 32), vec![targets[1]]);
        let mut script = ShodanHead::default();
        assert_eq!(hit(&mut script, &world, head, 100, 0), targets);
    }

    #[test]
    fn requested_damage_and_healing_do_not_predict_a_threshold() {
        let (world, head, targets) = fixture(125);
        let mut script = ShodanHead::default();
        assert!(matches!(
            script.handle_message(
                head,
                &world,
                &PhysicsWorld::new(),
                &MessagePayload::Damage {
                    amount: 1000.0,
                    impact: None
                }
            ),
            Effect::NoEffect
        ));
        assert!(hit(&mut script, &world, head, 125, 125).is_empty());
        assert!(hit(&mut script, &world, head, 0, 20).is_empty());
        assert_eq!(hit(&mut script, &world, head, 125, 0), targets);
    }

    #[test]
    fn first_latch_survives_save_round_trip_without_entity_references() {
        let (world, head, targets) = fixture(125);
        let mut script = ShodanHead::default();
        assert_eq!(hit(&mut script, &world, head, 125, 75), vec![targets[0]]);
        let state = script.save_state().unwrap();
        let mut restored = ShodanHead::default();
        restored
            .restore_state(&state, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();
        assert!(hit(&mut restored, &world, head, 75, 65).is_empty());
        assert_eq!(hit(&mut restored, &world, head, 65, 35), vec![targets[1]]);
    }
}
