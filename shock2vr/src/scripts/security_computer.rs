use dark::properties::{Link, ObjectState, PropEcoState, PropEcology, PropHackTime, PropObjState};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::quest_info::QuestInfo;

use super::{Effect, Message, MessagePayload, script_util::get_all_links_of_type};

const ECOLOGY_STATE_ALERT: i32 = 2;

pub(crate) fn reset_active_alarms(world: &World, sender: EntityId) -> Effect {
    let Ok((ecologies, states)) = world.borrow::<(View<PropEcology>, View<PropEcoState>)>() else {
        return Effect::NoEffect;
    };
    Effect::combine(
        (&ecologies, &states)
            .iter()
            .with_id()
            .filter(|(_, (_, state))| state.0 == ECOLOGY_STATE_ALERT)
            .map(|(ecology, _)| Effect::Send {
                msg: Message {
                    to: ecology,
                    payload: MessagePayload::Reset { from: sender },
                },
            })
            .collect(),
    )
}

pub(crate) fn normal_frob(world: &World, computer: EntityId) -> Effect {
    let state = world
        .borrow::<View<PropObjState>>()
        .ok()
        .and_then(|states| states.get(computer).ok().map(|state| state.0))
        .unwrap_or(ObjectState::Normal);
    if state == ObjectState::Normal {
        reset_active_alarms(world, computer)
    } else {
        Effect::NoEffect
    }
}

pub(crate) fn successful_hack(computer: EntityId, world: &World) -> Effect {
    let authored_seconds = world
        .borrow::<View<PropHackTime>>()
        .ok()
        .and_then(|times| times.get(computer).ok().map(|time| time.0))
        .unwrap_or_default();
    let cyber = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .map(|quest| quest.player_stats().cyber_affinity)
        .unwrap_or(1);
    let duration_seconds = authored_seconds.saturating_mul(cyber).max(0) as f32 / 1_000.0;

    Effect::combine(vec![
        reset_active_alarms(world, computer),
        Effect::ActivateSecurityHack { duration_seconds },
    ])
}

pub(crate) fn critical_failure(computer: EntityId, world: &World) -> Effect {
    let mut effects = vec![Effect::SetObjectState {
        entity_id: computer,
        state: ObjectState::Broken,
    }];
    effects.extend(
        get_all_links_of_type(world, computer, Link::SwitchLink)
            .into_iter()
            .map(|to| Effect::Send {
                msg: Message {
                    to,
                    payload: MessagePayload::Alarm { from: computer },
                },
            }),
    );
    Effect::combine(effects)
}

pub(crate) fn security_devices_can_detect_player(world: &World) -> bool {
    world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|quest| !quest.security_hack_active())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use dark::properties::{
        Link, Links, PropEcoState, PropEcology, PropHackTime, ToLink, WrappedEntityId,
    };
    use shipyard::World;

    use super::*;
    use crate::scripts::MessagePayload;

    fn ecology() -> PropEcology {
        PropEcology {
            period_seconds: 15.0,
            min_count: [0; 3],
            max_count: [0; 3],
            recovery_seconds: [0.0, 0.0, 30.0],
            random_chance: [0; 3],
        }
    }

    #[test]
    fn global_alarm_reset_targets_only_alert_ecologies() {
        let mut world = World::new();
        let sender = world.add_entity(());
        let alert = world.add_entity((ecology(), PropEcoState(2)));
        let _normal = world.add_entity((ecology(), PropEcoState(0)));

        let effects = Effect::flatten(vec![reset_active_alarms(&world, sender)]);
        assert_eq!(effects.len(), 1);
        assert!(matches!(
            &effects[0],
            Effect::Send { msg }
                if msg.to == alert
                    && matches!(msg.payload, MessagePayload::Reset { from } if from == sender)
        ));
    }

    #[test]
    fn critical_failure_breaks_the_computer_and_alarms_authored_switch_links() {
        let mut world = World::new();
        let ecology = world.add_entity(());
        let computer = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 341,
                to_entity_id: Some(WrappedEntityId(ecology)),
                link: Link::SwitchLink,
            }],
        });

        let effects = Effect::flatten(vec![critical_failure(computer, &world)]);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetObjectState { entity_id, state: dark::properties::ObjectState::Broken }
                if *entity_id == computer
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Send { msg }
                if msg.to == ecology
                    && matches!(msg.payload, MessagePayload::Alarm { from } if from == computer)
        )));
    }

    #[test]
    fn successful_hack_scales_authored_seconds_by_cyber_and_resets_alarm() {
        let mut world = World::new();
        let mut quest = QuestInfo::new();
        quest.player_stats_mut().cyber_affinity = 3;
        world.add_unique(quest);
        let computer = world.add_entity(PropHackTime(30_000));
        let alert = world.add_entity((ecology(), PropEcoState(2)));

        let effects = Effect::flatten(vec![successful_hack(computer, &world)]);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::ActivateSecurityHack { duration_seconds } if *duration_seconds == 90.0
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Send { msg }
                if msg.to == alert
                    && matches!(msg.payload, MessagePayload::Reset { from } if from == computer)
        )));
    }

    #[test]
    fn active_security_hack_hides_player_from_security_devices() {
        let world = World::new();
        let mut quest = QuestInfo::new();
        assert!(security_devices_can_detect_player(&world));
        quest.activate_security_hack(10.0);
        world.add_unique(quest);
        assert!(!security_devices_can_detect_player(&world));
    }
}
