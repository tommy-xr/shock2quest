//! Optional off-hand Kinetic Redirection. The amp keeps its selected power.
use crate::{input_context::InputContext, scripts::Effect};
use cgmath::{InnerSpace, Quaternion, Rotation, Vector3};
use shipyard::{EntityId, UniqueView, World};

type Source = (EntityId, Vector3<f32>, Vector3<f32>);

pub(crate) struct FreeHandPull {
    pressed: [bool; 2],
    claimed: [bool; 2],
    sources: [Option<Source>; 2],
}

impl Default for FreeHandPull {
    fn default() -> Self {
        Self {
            pressed: [true; 2],
            claimed: [false; 2],
            sources: [None; 2],
        }
    }
}

impl FreeHandPull {
    pub fn source(&self, hand: usize, amp: EntityId) -> Option<(Vector3<f32>, Vector3<f32>)> {
        self.sources
            .get(hand)
            .copied()
            .flatten()
            .filter(|source| source.0 == amp)
            .map(|(_, position, forward)| (position, forward))
    }

    /// Consume only the eligible empty hand's trigger. Disabled contexts and
    /// tracking recovery require a release before another cast can start.
    pub fn update(
        &mut self,
        world: &World,
        input: &mut InputContext,
        held: [Option<EntityId>; 2],
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        enabled: bool,
        physical_pressed: [bool; 2],
        target_available: impl Fn(EntityId, Vector3<f32>, Vector3<f32>) -> bool,
    ) -> Option<Effect> {
        let trained = world
            .borrow::<UniqueView<crate::psi::PlayerPsiKnownPowers>>()
            .is_ok_and(|known| known.0.contains(&crate::psi_pull::POWER));
        let cost = world
            .borrow::<UniqueView<crate::psi::GlobalPsiPowers>>()
            .ok()
            .and_then(|powers| {
                powers
                    .0
                    .iter()
                    .find(|p| p.template_id == crate::psi_pull::POWER)
                    .map(|p| p.power.psi_cost)
            });
        self.sources = [None; 2];
        let mut effect = None;
        let tracking = input.pose_tracking;
        let poses_valid = [&input.left_hand, &input.right_hand].iter().all(|hand| {
            crate::vr_support::GripPose {
                position: hand.position,
                rotation: hand.rotation,
            }
            .is_tracked()
        });
        for (i, hand) in [&mut input.left_hand, &mut input.right_hand]
            .into_iter()
            .enumerate()
        {
            let pressed = physical_pressed[i];
            if !pressed {
                self.claimed[i] = false;
            }
            let amp = held[1 - i].filter(|amp| crate::wielded_weapon::is_psi_amp(world, *amp));
            let valid = enabled
                && trained
                && cost.is_some()
                && held[i].is_none()
                && amp.is_some()
                && tracking.is_none_or(|t| t.head && t.hands[0] && t.hands[1])
                && poses_valid;
            if valid {
                let amp = amp.unwrap();
                let position = crate::virtual_hand::hand_world_position(
                    player_pos,
                    player_rotation,
                    hand.position,
                );
                let forward = (player_rotation * hand.rotation)
                    .rotate_vector(Vector3::new(0.0, 0.0, -1.0))
                    .normalize();
                self.sources[i] = Some((amp, position, forward));
                if pressed
                    && hand.trigger_value > 0.5
                    && !self.pressed[i]
                    && hand.squeeze_value < 0.5
                    && target_available(amp, position, forward)
                {
                    self.claimed[i] = true;
                    effect = Some(Effect::PsiPull {
                        amp,
                        cost: cost.unwrap(),
                        hand: Some(i),
                    });
                }
            }
            // Keep ordinary frobbing when aiming at doors, panels or ineligible
            // props. Once a pull owns a press, swallow it until physical release.
            if self.claimed[i] {
                hand.trigger_value = 0.0;
            }
            self.pressed[i] = !valid || pressed;
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psi::{GlobalPsiPowers, PlayerPsiKnownPowers, PsiPowerInfo};
    use cgmath::{Zero, vec3};

    fn fixture() -> (World, EntityId, InputContext) {
        let mut world = World::new();
        let amp = world.add_entity(dark::properties::PropTemplateId { template_id: -247 });
        world.add_unique(crate::mission::mission_core::GlobalTemplateClassTags(
            std::collections::HashMap::from([(
                -247,
                std::collections::HashMap::from([("weapontype".into(), "psiamp".into())]),
            )]),
        ));
        world.add_unique(PlayerPsiKnownPowers(std::collections::HashSet::from([
            crate::psi_pull::POWER,
        ])));
        world.add_unique(GlobalPsiPowers(vec![PsiPowerInfo {
            template_id: crate::psi_pull::POWER,
            name: "PsiPull".into(),
            display_name: None,
            power: dark::properties::PropPsiPower {
                power_id: 1,
                activation_type: 1,
                psi_cost: 1,
                data: [0.0; 4],
            },
            projectiles: vec![],
            overloadable: false,
            duration: None,
        }]));
        let mut input = InputContext::default();
        input.left_hand.rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        input.right_hand.rotation = input.left_hand.rotation;
        (world, amp, input)
    }

    fn update(
        state: &mut FreeHandPull,
        world: &World,
        input: &InputContext,
        held: [Option<EntityId>; 2],
        enabled: bool,
    ) -> (Option<Effect>, InputContext) {
        let mut masked = input.clone();
        let effect = state.update(
            world,
            &mut masked,
            held,
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            enabled,
            [
                input.left_hand.trigger_value > 0.5,
                input.right_hand.trigger_value > 0.5,
            ],
            |_, _, _| true,
        );
        (effect, masked)
    }

    #[test]
    fn either_empty_hand_casts_once_without_changing_the_amp_hand() {
        for hand in 0..2 {
            let (world, amp, mut input) = fixture();
            let mut state = FreeHandPull::default();
            let mut held = [None; 2];
            held[1 - hand] = Some(amp);
            assert!(update(&mut state, &world, &input, held, true).0.is_none());
            [&mut input.left_hand, &mut input.right_hand][hand].trigger_value = 1.0;
            let (effect, masked) = update(&mut state, &world, &input, held, true);
            assert!(
                matches!(effect, Some(Effect::PsiPull { amp: a, cost: 1, hand: Some(h) }) if a == amp && h == hand)
            );
            assert_eq!(
                [masked.left_hand, masked.right_hand][hand].trigger_value,
                0.0
            );
            assert!(update(&mut state, &world, &input, held, true).0.is_none());
            assert!(state.source(hand, amp).is_some());
        }
    }

    #[test]
    fn disabled_untrained_occupied_and_recovered_hands_cannot_cast() {
        let (mut world, amp, mut input) = fixture();
        let item = world.add_entity(());
        let held = [None, Some(amp)];
        let mut state = FreeHandPull::default();
        update(&mut state, &world, &input, held, true);
        input.left_hand.trigger_value = 1.0;
        assert!(update(&mut state, &world, &input, held, false).0.is_none());
        assert!(update(&mut state, &world, &input, held, true).0.is_none());
        input.left_hand.trigger_value = 0.0;
        update(&mut state, &world, &input, held, true);
        input.left_hand.trigger_value = 1.0;
        assert!(
            update(&mut state, &world, &input, [Some(item), Some(amp)], true)
                .0
                .is_none()
        );
        input.left_hand.rotation = Quaternion::zero();
        assert!(update(&mut state, &world, &input, held, true).0.is_none());
        assert!(state.source(0, amp).is_none());
        input.left_hand.rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        assert!(update(&mut state, &world, &input, held, true).0.is_none());
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [false, true],
        });
        assert!(update(&mut state, &world, &input, held, true).0.is_none());
        input.pose_tracking = None;
        input.left_hand.trigger_value = 0.0;
        world
            .borrow::<shipyard::UniqueViewMut<PlayerPsiKnownPowers>>()
            .unwrap()
            .0
            .clear();
        update(&mut state, &world, &input, held, true);
        input.left_hand.trigger_value = 1.0;
        assert!(update(&mut state, &world, &input, held, true).0.is_none());
    }
    #[test]
    fn pull_reaches_a_waiting_grip_without_using_the_caught_item() {
        let (world, amp, mut input) = fixture();
        let held = [None, Some(amp)];
        let mut state = FreeHandPull::default();
        update(&mut state, &world, &input, held, true);
        input.left_hand.trigger_value = 1.0;
        assert!(update(&mut state, &world, &input, held, true).0.is_some());
        input.left_hand.squeeze_value = 1.0;
        let (effect, masked) = update(&mut state, &world, &input, held, true);
        assert!(effect.is_none());
        assert_eq!(masked.left_hand.trigger_value, 0.0);
        assert!(
            state.source(0, amp).is_some(),
            "preparing to catch must not cancel the flight"
        );
        let (_, masked) = update(&mut state, &world, &input, [Some(amp), None], true);
        assert_eq!(
            masked.left_hand.trigger_value, 0.0,
            "catching cannot fire the newly held item"
        );
        input.left_hand.trigger_value = 0.0;
        update(&mut state, &world, &input, [Some(amp), None], true);
        input.left_hand.trigger_value = 1.0;
        assert_eq!(
            update(&mut state, &world, &input, [Some(amp), None], true)
                .1
                .left_hand
                .trigger_value,
            1.0
        );
    }

    #[test]
    fn doors_keep_their_trigger_and_mid_press_aim_cannot_start_a_pull() {
        let (world, amp, mut input) = fixture();
        let mut state = FreeHandPull::default();
        update(&mut state, &world, &input, [None, Some(amp)], true);
        input.left_hand.trigger_value = 1.0;
        let effect = state.update(
            &world,
            &mut input,
            [None, Some(amp)],
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            true,
            [true, false],
            |_, _, _| false,
        );
        assert!(effect.is_none());
        assert_eq!(input.left_hand.trigger_value, 1.0);
        assert!(
            update(&mut state, &world, &input, [None, Some(amp)], true)
                .0
                .is_none()
        );
    }

    #[test]
    fn interface_masking_cannot_release_a_physically_held_trigger() {
        let (world, amp, mut input) = fixture();
        let mut state = FreeHandPull::default();
        update(&mut state, &world, &input, [None, Some(amp)], true);
        input.left_hand.trigger_value = 1.0;
        update(&mut state, &world, &input, [None, Some(amp)], true);
        let mut masked = input.clone();
        masked.left_hand.trigger_value = 0.0;
        state.update(
            &world,
            &mut masked,
            [None, Some(amp)],
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            false,
            [true, false],
            |_, _, _| false,
        );
        assert_eq!(
            update(&mut state, &world, &input, [Some(amp), None], true)
                .1
                .left_hand
                .trigger_value,
            0.0
        );
    }
}
