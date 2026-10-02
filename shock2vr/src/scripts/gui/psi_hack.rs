//! Psionic HRM uses the ordinary board/outcomes, with PSI terms and payment.
use super::keypad::{self, HackOutcomeEffects, HackPhase, HackState, HrmContext, KeyPadMsg};
use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    scripts::Effect,
};
use cgmath::{vec2, vec3};
use shipyard::{EntityId, Unique, UniqueView, World};

pub(crate) const POWER: i32 = -3159;
#[derive(Clone, Copy)]
pub(crate) struct Session {
    pub amp: EntityId,
    pub target: EntityId,
    pub psi: i32,
}
#[derive(Unique)]
pub(crate) struct PsiHack {
    pub host: EntityId,
    pub session: Option<Session>,
}
pub(crate) fn session(world: &World) -> Option<Session> {
    world.borrow::<UniqueView<PsiHack>>().ok()?.session
}

/// Dispatch to the exact eligibility and outcome functions used by paid boards.
/// No generic SetObjectState shortcut: consoles, turrets and authored circuits differ.
pub(crate) fn target(
    world: &World,
    entity: EntityId,
) -> Option<(dark::properties::PropHackDiff, HackOutcomeEffects)> {
    use super::{computer, hackable_crate, replicator};
    let has = |name| crate::scripts::script_util::entity_has_script(world, entity, name);
    let diff = keypad::hack_diff(world, entity)?;
    let pair = if has("HackableCrate") && hackable_crate::can_hack(world, entity) {
        (
            hackable_crate::crate_hack_success as fn(EntityId, &World) -> Effect,
            hackable_crate::crate_hack_critical_failure as fn(EntityId, &World) -> Effect,
        )
    } else if has("ReplicatorScript") && replicator::can_hack(world, entity) {
        (
            replicator::replicator_hack_success as fn(EntityId, &World) -> Effect,
            replicator::replicator_hack_critical_failure as fn(EntityId, &World) -> Effect,
        )
    } else if has("Keypad") && keypad::hack_diff_for_entity(world, entity).is_some() {
        (
            keypad::keypad_hack_success as fn(EntityId, &World) -> Effect,
            keypad::keypad_hack_critical_failure as fn(EntityId, &World) -> Effect,
        )
    } else if has("SecurityComputer") && computer::can_hack(world, entity) {
        (
            computer::security_hack_success as fn(EntityId, &World) -> Effect,
            computer::security_hack_critical_failure as fn(EntityId, &World) -> Effect,
        )
    } else if has("Turret")
        && computer::can_hack(world, entity)
        && crate::scripts::ai::ai_util::ai_team(world, entity) != dark::properties::AITeam::Good
    {
        (
            computer::turret_hack_success as fn(EntityId, &World) -> Effect,
            computer::computer_hack_critical_failure as fn(EntityId, &World) -> Effect,
        )
    } else if has("Computer") && computer::can_hack(world, entity) {
        (
            computer::computer_hack_success as fn(EntityId, &World) -> Effect,
            computer::computer_hack_critical_failure as fn(EntityId, &World) -> Effect,
        )
    } else {
        return None;
    };
    Some((
        diff,
        HackOutcomeEffects {
            success: pair.0,
            critical_failure: pair.1,
        },
    ))
}

pub(crate) struct PsiHackGui;
impl Gui<HackState, KeyPadMsg> for PsiHackGui {
    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: vec3(0., 0., 0.),
            screen_size_in_pixels: vec2(188., 296.),
        }
    }
    fn resets_state_on_frob(&self) -> bool {
        true
    }
    fn get_components(
        &self,
        _: &Option<GuiCursor>,
        _: EntityId,
        world: &World,
        state: &HackState,
    ) -> Vec<GuiComponent<KeyPadMsg>> {
        let Some(s) = session(world) else {
            return vec![];
        };
        let Some((diff, _)) = target(world, s.target) else {
            return vec![];
        };
        let context = HrmContext::PsiHack { psi: s.psi };
        let mut components = keypad::draw_hack_board(state, diff, context, |m| m);
        components.extend(keypad::draw_hrm_text(
            world,
            &keypad::hack_goal_text(world, s.target),
            diff,
            context,
        ));
        components.push(GuiComponent::Fill {
            position: vec2(84., 161.),
            size: vec2(40., 14.),
            color: [0, 12, 12],
            alpha: 1.,
        });
        components.push(super::PanelText::centered(
            "PSI",
            crate::ui::Rect::new(84., 161., 40., 14.),
        ));
        if state.phase == HackPhase::InsufficientNanites {
            components.push(GuiComponent::Fill {
                position: vec2(15., 12.),
                size: vec2(137., 34.),
                color: [0, 12, 12],
                alpha: 1.,
            });
            components.push(super::PanelText::centered(
                "Not enough PSI",
                crate::ui::Rect::new(15., 12., 137., 34.),
            ));
        }
        components
    }
    fn handle_msg(
        &self,
        _: EntityId,
        world: &World,
        state: &HackState,
        msg: &KeyPadMsg,
    ) -> (HackState, Effect) {
        let Some(s) = session(world) else {
            return (state.clone(), Effect::NoEffect);
        };
        let Some((diff, outcomes)) = target(world, s.target) else {
            return (state.clone(), Effect::EndPsiHack);
        };
        let (next, effect) = keypad::handle_hrm_msg(
            s.target,
            world,
            state,
            msg,
            diff,
            HrmContext::PsiHack { psi: s.psi },
            outcomes,
        );
        let effect = if matches!(next.phase, HackPhase::Won | HackPhase::Lost) {
            Effect::combine(vec![effect, Effect::EndPsiHack])
        } else {
            effect
        };
        (next, effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::Effect;
    use dark::properties::{ObjectState, PropHackDiff, PropObjState, PropPsiState, PropScripts};

    fn fixture(points: i32) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let player = world.add_entity(PropPsiState {
            psi_points: points,
            max_psi_points: 50,
            unknown: 0,
        });
        world.add_unique(crate::mission::PlayerInfo {
            pos: vec3(0., 0., 0.),
            rotation: cgmath::Quaternion::new(1., 0., 0., 0.),
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
        });
        let target = world.add_entity((
            PropScripts {
                scripts: vec!["HackableCrate".into()],
                inherits: false,
            },
            PropObjState(ObjectState::Locked),
            PropHackDiff {
                success_chance: 40,
                critical_chance: 2,
                cost: 5.0,
            },
        ));
        let host = world.add_entity(());
        world.add_unique(PsiHack {
            host,
            session: Some(Session {
                amp: player,
                target,
                psi: 5,
            }),
        });
        (world, target, host)
    }

    #[test]
    fn psionic_start_uses_full_authored_psi_cost_and_refuses_an_empty_pool() {
        for (points, accepted) in [(4, false), (5, true)] {
            let (world, _, host) = fixture(points);
            let (state, effect) =
                PsiHackGui.handle_msg(host, &world, &HackState::default(), &KeyPadMsg::StartHack);
            assert_eq!(
                state.phase,
                if accepted {
                    HackPhase::Playing
                } else {
                    HackPhase::InsufficientNanites
                }
            );
            let effects = Effect::flatten(vec![effect]);
            assert_eq!(
                effects
                    .iter()
                    .filter(|e| matches!(e, Effect::SpendPsiPoints { amount: 5 }))
                    .count(),
                usize::from(accepted)
            );
            assert!(
                !effects
                    .iter()
                    .any(|e| matches!(e, Effect::SetObjectState { .. })),
                "paying is never a hack win"
            );
        }
    }

    #[test]
    fn remote_crate_outcome_is_owned_by_target_and_terminal_states_are_refused() {
        let (mut world, crate_id, host) = fixture(20);
        let (_, callbacks) = target(&world, crate_id).unwrap();
        assert!(
            matches!((callbacks.success)(crate_id, &world), Effect::SetObjectState { entity_id, state: ObjectState::Hacked } if entity_id == crate_id && entity_id != host)
        );
        assert!(
            matches!((callbacks.critical_failure)(crate_id, &world), Effect::SetObjectState { entity_id, state: ObjectState::Broken } if entity_id == crate_id)
        );
        for state in [
            ObjectState::Hacked,
            ObjectState::Broken,
            ObjectState::Destroyed,
        ] {
            world.add_component(crate_id, PropObjState(state));
            assert!(target(&world, crate_id).is_none());
        }
        assert!(target(&world, host).is_none());
    }
}
