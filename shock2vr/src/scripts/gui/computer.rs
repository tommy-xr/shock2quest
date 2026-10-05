use std::collections::HashMap;

use cgmath::{Vector2, Vector3};
use dark::{
    properties::{
        Link, Links, ObjectState, PropHackDiff, PropHackText, PropTemplateId, ToLink,
        WrappedEntityId,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::{
    gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor, PanelSidecar},
    scripts::{Effect, Message, MessagePayload, script_util::*},
    ui::Rect,
};

use super::PanelText;
use super::hrm_plug::{self, PlugKind, draw_plug, plug_sidecar};
use super::keypad::{
    HackOutcomeEffects, HackPhase, HackState, KeyPadMsg, draw_hack_panel, hack_diff,
    handle_hack_msg, object_state,
};

/// Xerxes: "Security system offline."
const SECURITY_HACKED_SCHEMA: &str = "xer01";

/// Older saves made before `P$HackText` / `L$HackingLi` were parsed cannot
/// contain those components. Restore them from the same authored object data
/// used for a fresh mission, so such a save retains Computer instructions and
/// the genuine downstream hacking circuit.
pub(crate) fn restore_authored_computer_data(
    world: &mut World,
    entity_info: &SystemShock2EntityInfo,
    template_to_entity_id: &HashMap<i32, WrappedEntityId>,
) {
    let computers = {
        let templates = world.borrow::<View<PropTemplateId>>().unwrap();
        let diffs = world.borrow::<View<PropHackDiff>>().unwrap();
        templates
            .iter()
            .with_id()
            .filter(|(entity_id, _)| diffs.get(*entity_id).is_ok())
            .map(|(entity_id, template)| (entity_id, template.template_id))
            .collect::<Vec<_>>()
    };

    for (entity_id, template_id) in computers {
        let has_text = world
            .borrow::<View<PropHackText>>()
            .unwrap()
            .get(entity_id)
            .is_ok();
        if !has_text
            && let Some(text) = hydrate_template_component::<PropHackText>(template_id, entity_info)
        {
            world.add_component(entity_id, text);
        }

        let has_hacking_link = world
            .borrow::<View<Links>>()
            .unwrap()
            .get(entity_id)
            .is_ok_and(|links| links.to_links.iter().any(|link| link.link == Link::Hacking));
        if has_hacking_link {
            continue;
        }

        let mut ancestors = dark::ss2_entity_info::get_ancestors(
            dark::ss2_entity_info::get_hierarchy(entity_info),
            &template_id,
        );
        ancestors.push(template_id);
        let authored_hacking_links = ancestors
            .into_iter()
            .filter_map(|ancestor| entity_info.template_to_links.get(&ancestor))
            .flat_map(|links| &links.to_links)
            .filter(|link| link.link == Link::Hacking)
            .map(|link| ToLink {
                to_template_id: link.to_template_id,
                to_entity_id: template_to_entity_id.get(&link.to_template_id).copied(),
                link: Link::Hacking,
            })
            .collect::<Vec<_>>();
        if authored_hacking_links.is_empty() {
            continue;
        }
        let mut links = world
            .borrow::<View<Links>>()
            .unwrap()
            .get(entity_id)
            .cloned()
            .unwrap_or_else(|_| Links::empty());
        links.to_links.extend(authored_hacking_links);
        world.add_component(entity_id, links);
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ComputerKind {
    #[default]
    Generic,
    Security,
    Turret,
}

#[derive(Default)]
pub struct ComputerGui {
    kind: ComputerKind,
}

impl ComputerGui {
    pub fn security() -> Self {
        Self {
            kind: ComputerKind::Security,
        }
    }
    pub fn turret() -> Self {
        Self {
            kind: ComputerKind::Turret,
        }
    }
    fn is_security(&self) -> bool {
        self.kind == ComputerKind::Security
    }
}

#[derive(Clone, Debug, Default)]
pub struct ComputerState {
    show_hack: bool,
    hack: HackState,
}

#[derive(Clone)]
pub enum ComputerMsg {
    OpenHack,
    Hack(KeyPadMsg),
}

pub(super) fn can_hack(world: &World, entity_id: EntityId) -> bool {
    !matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed | ObjectState::Hacked
    ) && hack_diff(world, entity_id).is_some()
}

pub(super) fn security_hack_success(entity_id: EntityId, world: &World) -> Effect {
    let milliseconds = world
        .borrow::<View<dark::properties::PropHackTime>>()
        .ok()
        .and_then(|times| times.get(entity_id).ok().map(|time| time.0))
        .unwrap_or(0);
    let cyber = crate::implants::effective_stats(world)
        .map(|stats| stats.cyber_affinity)
        .unwrap_or(1);
    let duration_seconds = milliseconds.saturating_mul(cyber).max(0) as f32 / 1000.0;
    Effect::combine(vec![
        Effect::ClearSecurityAlarm { from: entity_id },
        Effect::ActivateSecurityHack { duration_seconds },
        announce(entity_id, SECURITY_HACKED_SCHEMA),
    ])
}

pub(super) fn turret_hack_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetAITeam {
        entity_id,
        team: dark::properties::AITeam::Good,
    }
}

pub(super) fn computer_hack_success(entity_id: EntityId, world: &World) -> Effect {
    let mut effects = get_all_links_of_type(world, entity_id, Link::Hacking)
        .into_iter()
        .map(|to| Effect::Send {
            msg: Message {
                to,
                payload: MessagePayload::TurnOn { from: entity_id },
            },
        })
        .collect::<Vec<_>>();

    let hacked_replacement =
        get_first_link_with_template_and_data(world, entity_id, |link| match link {
            Link::Corpse(_) => Some(()),
            _ => None,
        })
        .map(|(template_id, ())| template_id);
    effects.push(match hacked_replacement {
        Some(template_id) => Effect::ReplaceEntity {
            entity_id,
            template_id,
        },
        None => Effect::SetObjectState {
            entity_id,
            state: ObjectState::Hacked,
        },
    });
    Effect::combine(effects)
}

pub(super) fn computer_hack_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

pub(super) fn security_hack_critical_failure(entity_id: EntityId, world: &World) -> Effect {
    let mut effects = vec![
        computer_hack_critical_failure(entity_id, world),
        Effect::RaiseSecurityAlarm {
            seconds: crate::security_alarm::authored_alarm_seconds(world, entity_id),
        },
    ];
    effects.extend(
        crate::scripts::script_util::get_all_switch_links(world, entity_id)
            .into_iter()
            .map(|to| Effect::Send {
                msg: Message {
                    to,
                    payload: MessagePayload::Alarm { from: entity_id },
                },
            }),
    );
    Effect::combine(effects)
}

impl Gui<ComputerState, ComputerMsg> for ComputerGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &ComputerState,
    ) -> Vec<GuiComponent<ComputerMsg>> {
        if self.is_security() && !state.show_hack {
            // Retail shkscomp.cpp: ALARMFD, AlarmState[0/1] at (18, 187),
            // and the shared HRM plug. Layout is resolved once for flat/VR.
            let mut components =
                vec![gui::image("alarmfd.pcx").with_rect(Rect::new(0.0, 0.0, 188.0, 296.0))];
            let (key, fallback) =
                if crate::security_alarm::security_devices_can_detect_player(world) {
                    (
                        "AlarmState1",
                        "Security system active....\n\nNo threats detected.",
                    )
                } else {
                    (
                        "AlarmState0",
                        "Security system disabled....\n\nCameras deactivated.",
                    )
                };
            let status = PanelText::string(world, "misc", key, fallback).replace("\\n", "\n");
            components.extend(PanelText::paragraph(
                world,
                &status,
                Rect::new(18.0, 187.0, 150.0, 100.0),
            ));
            if can_hack(world, entity_id) {
                components.extend(draw_plug(
                    PlugKind::Hack,
                    Some((ComputerMsg::OpenHack, "hack-security")),
                ));
            }
            return components;
        }
        let Some(diff) = hack_diff(world, entity_id) else {
            return Vec::new();
        };
        draw_hack_panel(
            world,
            entity_id,
            &state.hack,
            diff,
            self.is_security(),
            ComputerMsg::Hack,
        )
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -1.0),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn get_config_for(
        &self,
        _entity_id: EntityId,
        _world: &World,
        _state: &ComputerState,
    ) -> GuiConfig {
        let mut config = self.get_config();
        if self.is_security() {
            config.screen_size_in_pixels.x = hrm_plug::CANVAS_W;
        }
        config
    }

    fn sidecar(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &ComputerState,
    ) -> Option<PanelSidecar> {
        self.is_security()
            .then(|| plug_sidecar(!state.show_hack && can_hack(world, entity_id)))
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &ComputerState,
        msg: &ComputerMsg,
    ) -> (ComputerState, Effect) {
        if let ComputerMsg::OpenHack = msg {
            let mut next = state.clone();
            if self.is_security() && can_hack(world, entity_id) {
                next.show_hack = true;
            }
            return (next, Effect::NoEffect);
        }
        let ComputerMsg::Hack(msg) = msg else {
            unreachable!()
        };
        if self.is_security() && !state.show_hack {
            return (state.clone(), Effect::NoEffect);
        }
        let Some(diff) = hack_diff(world, entity_id) else {
            return (state.clone(), Effect::NoEffect);
        };
        let (hack, effect) = handle_hack_msg(
            entity_id,
            world,
            &state.hack,
            msg,
            diff,
            self.is_security(),
            HackOutcomeEffects {
                success: match self.kind {
                    ComputerKind::Security => security_hack_success,
                    ComputerKind::Turret => turret_hack_success,
                    ComputerKind::Generic => computer_hack_success,
                },
                critical_failure: if self.is_security() {
                    security_hack_critical_failure
                } else {
                    computer_hack_critical_failure
                },
            },
        );
        (
            ComputerState {
                hack,
                ..state.clone()
            },
            effect,
        )
    }

    fn on_frob(&self, entity_id: EntityId, world: &World) -> Effect {
        // Using a normal security console resets the alarm for free. Only a
        // successful paid hack grants the authored timed device suppression.
        if self.is_security() && object_state(world, entity_id) == ObjectState::Normal {
            Effect::ClearSecurityAlarm { from: entity_id }
        } else {
            Effect::NoEffect
        }
    }

    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        can_hack(world, entity_id)
            && (self.kind != ComputerKind::Turret
                || crate::scripts::ai::ai_util::ai_team(world, entity_id)
                    != dark::properties::AITeam::Good)
    }

    fn prepare_state_on_frob(&self, state: &mut ComputerState) {
        if state.hack.phase != HackPhase::Playing {
            *state = ComputerState::default();
        }
        // Reopening a station always shows its status first. An unfinished
        // paid board resumes when Hack is selected again.
        if self.is_security() {
            state.show_hack = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{CorpseOptions, Links, ToLink, WrappedEntityId};
    use shipyard::World;

    use super::*;
    use dark::properties::PropObjState;

    fn flatten(effect: &Effect) -> Vec<&Effect> {
        match effect {
            Effect::Combined { effects } => effects.iter().flat_map(flatten).collect(),
            other => vec![other],
        }
    }

    #[test]
    fn success_activates_hacking_links_and_uses_authored_replacement() {
        let mut world = World::new();
        let router = world.add_entity(());
        let computer = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 312,
                    to_entity_id: Some(WrappedEntityId(router)),
                    link: Link::Hacking,
                },
                ToLink {
                    to_template_id: 1269,
                    to_entity_id: None,
                    link: Link::Corpse(
                        serde_json::from_str::<CorpseOptions>(r#"{"propagate_scale":false}"#)
                            .unwrap(),
                    ),
                },
            ],
        });

        let effect = computer_hack_success(computer, &world);
        let effects = flatten(&effect);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Send {
                msg: Message {
                    to,
                    payload: MessagePayload::TurnOn { from },
                },
            } if *to == router && *from == computer
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::ReplaceEntity {
                entity_id,
                template_id: 1269,
            } if *entity_id == computer
        )));
    }

    #[test]
    fn success_without_corpse_persists_hacked_state() {
        let mut world = World::new();
        let computer = world.add_entity(Links::empty());
        let effects = computer_hack_success(computer, &world);
        assert!(flatten(&effects).iter().any(|effect| matches!(
            effect,
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Hacked,
            } if *entity_id == computer
        )));
    }

    #[test]
    fn critical_failure_persists_broken_state() {
        let mut world = World::new();
        let computer = world.add_entity(());
        assert!(matches!(
            computer_hack_critical_failure(computer, &world),
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            } if entity_id == computer
        ));
    }

    #[test]
    fn broken_and_hacked_computers_do_not_reopen() {
        let gui = ComputerGui::default();
        for state in [ObjectState::Broken, ObjectState::Hacked] {
            let mut world = World::new();
            let computer = world.add_entity((
                PropHackDiff {
                    success_chance: 20,
                    critical_chance: 10,
                    cost: 3.0,
                },
                PropObjState(state),
            ));
            assert!(!gui.opens_on_frob(computer, &world));
        }
    }

    #[test]
    fn reopening_preserves_only_an_in_progress_hack() {
        let gui = ComputerGui::default();
        let mut state = ComputerState {
            hack: HackState {
                phase: HackPhase::Playing,
                rng_state: 42,
                ..HackState::default()
            },
            ..ComputerState::default()
        };
        gui.prepare_state_on_frob(&mut state);
        assert_eq!(state.hack.phase, HackPhase::Playing);
        assert_eq!(state.hack.rng_state, 42);

        state.hack.phase = HackPhase::Won;
        gui.prepare_state_on_frob(&mut state);
        assert_eq!(state.hack.phase, HackPhase::Unpaid);
    }

    #[test]
    fn won_turret_hack_changes_team_and_stops_offering_the_board() {
        let mut world = World::new();
        let turret = world.add_entity(PropHackDiff {
            success_chance: 20,
            critical_chance: 5,
            cost: 5.0,
        });
        let gui = ComputerGui::turret();
        assert!(gui.opens_on_frob(turret, &world));
        assert!(
            matches!(turret_hack_success(turret, &world), Effect::SetAITeam { entity_id, team: dark::properties::AITeam::Good } if entity_id == turret)
        );
        world.add_component(
            turret,
            dark::properties::PropAITeam(dark::properties::AITeam::Good),
        );
        assert!(!gui.opens_on_frob(turret, &world));
    }

    #[test]
    fn security_opens_its_station_panel_before_the_paid_board() {
        let mut world = World::new();
        let computer = world.add_entity(PropHackDiff {
            success_chance: 20,
            critical_chance: 10,
            cost: 3.0,
        });
        let gui = ComputerGui::security();
        let components = gui.get_components(&None, computer, &world, &ComputerState::default());
        assert!(components.iter().any(|component| matches!(component,
            GuiComponent::Image { texture, .. } if texture == "alarmfd.pcx")));
        assert!(components.iter().any(|component| matches!(component,
            GuiComponent::Button { label, .. } if label.as_deref() == Some("hack-security"))));
        assert!(!components.iter().any(|component| matches!(component,
            GuiComponent::Button { label, .. } if label.as_deref() == Some("start-hack"))));

        let (mut state, effect) = gui.handle_msg(
            computer,
            &world,
            &ComputerState::default(),
            &ComputerMsg::OpenHack,
        );
        assert!(matches!(effect, Effect::NoEffect), "selecting Hack is free");
        let components = gui.get_components(&None, computer, &world, &state);
        assert!(components.iter().any(|component| matches!(component,
            GuiComponent::Button { label, .. } if label.as_deref() == Some("start-hack"))));
        assert!(
            gui.sidecar(computer, &world, &state)
                .unwrap()
                .rect
                .is_none()
        );

        state.hack.phase = HackPhase::Playing;
        state.hack.rng_state = 42;
        gui.prepare_state_on_frob(&mut state);
        assert!(!state.show_hack, "reopening returns to station status");
        let (resumed, _) = gui.handle_msg(computer, &world, &state, &ComputerMsg::OpenHack);
        assert_eq!(resumed.hack.phase, HackPhase::Playing);
        assert_eq!(
            resumed.hack.rng_state, 42,
            "an unfinished paid board is retained"
        );
    }

    #[test]
    fn security_station_status_tracks_the_timed_hack() {
        let mut world = World::new();
        let computer = world.add_entity(());
        let gui = ComputerGui::security();
        let mut quests = crate::quest_info::QuestInfo::new();
        quests.activate_security_hack(30.0);
        world.add_unique(quests);
        let components = gui.get_components(&None, computer, &world, &ComputerState::default());
        assert!(components.iter().any(|component| matches!(component,
            GuiComponent::Text { text, .. } if text.contains("Cameras deactivated"))));
        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .advance_security_hack(30.0);
        let components = gui.get_components(&None, computer, &world, &ComputerState::default());
        assert!(components.iter().any(|component| matches!(component,
            GuiComponent::Text { text, .. } if text.contains("No threats detected"))));
    }

    #[test]
    fn frobbing_a_normal_security_computer_only_clears_the_alarm() {
        let mut world = World::new();
        let computer = world.add_entity(PropObjState(ObjectState::Normal));
        assert!(matches!(
            ComputerGui::security().on_frob(computer, &world),
            Effect::ClearSecurityAlarm { from } if from == computer
        ));
    }

    #[test]
    fn frobbing_other_computers_or_unusable_security_does_not_clear_alarms() {
        let mut world = World::new();
        let normal = world.add_entity(PropObjState(ObjectState::Normal));
        for gui in [ComputerGui::default(), ComputerGui::turret()] {
            assert!(matches!(gui.on_frob(normal, &world), Effect::NoEffect));
        }
        for state in [
            ObjectState::Broken,
            ObjectState::Destroyed,
            ObjectState::Hacked,
        ] {
            let computer = world.add_entity(PropObjState(state));
            assert!(matches!(
                ComputerGui::security().on_frob(computer, &world),
                Effect::NoEffect
            ));
        }
    }

    #[test]
    fn security_hack_uses_authored_milliseconds_and_cyber() {
        let mut world = World::new();
        let mut quests = crate::quest_info::QuestInfo::new();
        quests.player_stats_mut().cyber_affinity = 3;
        world.add_unique(quests);
        let computer = world.add_entity(dark::properties::PropHackTime(30_000));
        let effects = security_hack_success(computer, &world);
        assert!(flatten(&effects).iter().any(|effect| matches!(effect,
            Effect::ActivateSecurityHack { duration_seconds } if *duration_seconds == 90.0)));
        assert!(flatten(&effects).iter().any(|effect| matches!(effect,
            Effect::ClearSecurityAlarm { from } if *from == computer)));
    }

    #[test]
    fn security_failure_breaks_the_console_and_alarms_its_link() {
        let mut world = World::new();
        let ecology = world.add_entity(());
        let computer = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 1,
                to_entity_id: Some(WrappedEntityId(ecology)),
                link: Link::SwitchLink,
            }],
        });
        let effects = security_hack_critical_failure(computer, &world);
        assert!(flatten(&effects).iter().any(|effect| matches!(effect,
            Effect::SetObjectState { entity_id, state: ObjectState::Broken } if *entity_id == computer)));
        assert!(flatten(&effects).iter().any(|effect| matches!(effect,
            Effect::Send { msg } if msg.to == ecology && matches!(msg.payload, MessagePayload::Alarm { from } if from == computer))));
    }

    #[test]
    fn a_hacked_security_computer_announces_security_offline() {
        let mut world = World::new();
        let computer = world.add_entity(());
        assert_eq!(
            announced(&security_hack_success(computer, &world)),
            ["xer01"]
        );
    }
}
