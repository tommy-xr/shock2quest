//! Glove feedback adapted from Fable #1373/#1376. Input and feedback use
//! the same resolved ray target; feedback never drives finger poses.
use crate::hand_glove::HandLight;
use cgmath::InnerSpace;
use shipyard::{EntityId, Get, View, World};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub enum HandAffordance {
    #[default]
    None,
    Grabbable,
    Frobbable,
    Blocked,
}

/// A resolved target's squeeze behavior and visible eligibility.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HandTarget {
    pub grabbable: bool,
    pub scripted: bool,
    pub affordance: HandAffordance,
}

impl HandTarget {
    pub fn resolve(world: &World, entity: EntityId, has_body: bool) -> Self {
        let scripted = crate::virtual_hand::uses_scripted_world_frob(world, entity);
        let grabbable = has_body
            && (crate::scripts::script_util::is_download_pickup(world, entity)
                || (!scripted && crate::virtual_hand::can_grab_item(world, entity)));
        let affordance = if crate::mission::personal_card::is_reader(world, entity) {
            HandAffordance::Blocked
        } else if grabbable {
            HandAffordance::Grabbable
        } else if scripted || is_frob_responsive(world, entity) {
            let door = world
                .borrow::<View<dark::properties::PropTranslatingDoor>>()
                .unwrap()
                .get(entity)
                .ok()
                .cloned();
            let blocked = if let Some(door) = door {
                if door.is_permanently_open() {
                    return Self::default();
                }
                // StdDoor publishes its destination as state 2/3 through effects.
                // Halted/authored poses use the same nearest-endpoint rule as
                // its initialization and target_is_open implementation.
                let target_open = match door.state {
                    2 => false,
                    3 => true,
                    _ => {
                        (door.initial_location() - door.base_open_location).magnitude2()
                            < (door.initial_location() - door.base_closed_location).magnitude2()
                    }
                };
                crate::scripts::player_door_frob_blocked(world, entity, target_open)
            } else {
                crate::scripts::script_util::is_entity_locked(world, entity)
            };
            if blocked {
                HandAffordance::Blocked
            } else {
                HandAffordance::Frobbable
            }
        } else {
            HandAffordance::None
        };
        Self {
            grabbable,
            scripted,
            affordance,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HandFeedback {
    pub observed: HandAffordance,
    failed_seconds: f32,
}

impl HandFeedback {
    /// Only an actual Frob attempt at a known blocked target pulses red.
    /// Hover clears immediately on occlusion/lost reach; it never promises a
    /// stale target. The failure pulse uses seconds, independent of refresh rate.
    pub fn update(&mut self, observed: HandAffordance, failed: bool, dt: f32) {
        self.observed = observed;
        self.failed_seconds = if failed {
            0.25
        } else {
            (self.failed_seconds - dt.max(0.0)).max(0.0)
        };
    }

    pub fn light(self) -> HandLight {
        if self.failed_seconds > 0.00001 {
            return HandLight::Red;
        }
        match self.observed {
            HandAffordance::None => HandLight::Off,
            HandAffordance::Grabbable | HandAffordance::Frobbable => HandLight::Green,
            HandAffordance::Blocked => HandLight::Amber,
        }
    }
}

fn is_frob_responsive(world: &World, entity_id: EntityId) -> bool {
    use dark::properties::{FrobFlag, PropFrobInfo, PropTranslatingDoor};

    let is_panel_widget = world
        .borrow::<View<crate::gui::GuiPropProxyEntity>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false);

    let authored = world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| {
            v.get(entity_id).is_ok_and(|frob_info| {
                !frob_info.world_action.is_empty()
                    && !frob_info.world_action.contains(FrobFlag::IGNORE)
            })
        })
        .unwrap_or(false);

    is_panel_widget
        || authored
        || world
            .borrow::<View<PropTranslatingDoor>>()
            .map(|v| v.get(entity_id).is_ok())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_pulse_has_the_same_duration_at_60_and_90_hz() {
        for rate in [60, 90] {
            let mut feedback = HandFeedback::default();
            feedback.update(HandAffordance::Blocked, true, 0.0);
            for _ in 0..(rate / 5) {
                feedback.update(HandAffordance::Blocked, false, 1.0 / rate as f32);
            }
            assert_eq!(feedback.light(), HandLight::Red);
            for _ in 0..(rate / 10) {
                feedback.update(HandAffordance::Blocked, false, 1.0 / rate as f32);
            }
            assert_eq!(feedback.light(), HandLight::Amber);
        }
    }

    #[test]
    fn hover_does_not_outlive_its_actionable_target() {
        let mut feedback = HandFeedback::default();
        feedback.update(HandAffordance::Grabbable, false, 0.0);
        assert_eq!(feedback.light(), HandLight::Green);
        feedback.update(HandAffordance::None, false, 0.0);
        assert_eq!(feedback.light(), HandLight::Off);
    }
    #[test]
    fn keypad_gate_and_open_locked_door_match_the_door_frob_rule() {
        use cgmath::vec3;
        use dark::properties::WrappedEntityId;
        use dark::properties::{
            Link, Links, PropKeypadCode, PropLocked, PropTranslatingDoor, ToLink,
        };
        let mut world = World::new();
        world.add_unique(crate::quest_info::QuestInfo::new());
        let mut door = PropTranslatingDoor {
            door_type: 0,
            closed: 0.0,
            open: 1.0,
            speed: 1.0,
            axis: 0,
            state: 0,
            base_closed_location: vec3(0.0, 0.0, 0.0),
            base_open_location: vec3(1.0, 0.0, 0.0),
            base_location: vec3(0.0, 0.0, 0.0),
        };
        let entity = world.add_entity((door.clone(), PropLocked(false)));
        world.add_entity((
            PropKeypadCode(1234),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(entity)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        assert_eq!(
            HandTarget::resolve(&world, entity, true).affordance,
            HandAffordance::Blocked
        );
        for state in [1, 3] {
            door.state = state;
            world.add_component(entity, (door.clone(), PropLocked(true)));
            assert_eq!(
                HandTarget::resolve(&world, entity, true).affordance,
                HandAffordance::Frobbable
            );
        }
        door.state = 2;
        world.add_component(entity, door);
        assert_eq!(
            HandTarget::resolve(&world, entity, true).affordance,
            HandAffordance::Blocked
        );
    }
}
