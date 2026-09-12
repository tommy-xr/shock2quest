//! Per-hand tap/hold disambiguation, reusing the Menu button's timing policy.
use crate::{
    Handedness,
    hand_buttons::HeldKind,
    input::{InputAction, InputActionState, MenuHold, MenuPress},
    scripts::Effect,
};
use shipyard::{EntityId, Unique, UniqueView, UniqueViewMut, World};
use std::time::Duration;

#[derive(Unique, Default)]
pub(crate) struct EjectProgress(pub Vec<(EntityId, f32)>);
impl EjectProgress {
    fn publish(world: &World, progress: Self) {
        if let Ok(mut current) = world.borrow::<UniqueViewMut<Self>>() {
            *current = progress;
        } else {
            world.add_unique(progress);
        }
    }
    pub fn for_weapon(world: &World, weapon: EntityId) -> Option<f32> {
        world
            .borrow::<UniqueView<Self>>()
            .ok()?
            .0
            .iter()
            .find_map(|(w, p)| (*w == weapon).then_some(*p))
    }
}

#[derive(Default)]
struct Press {
    weapon: Option<EntityId>,
    timer: MenuHold,
}
impl Press {
    fn update(
        &mut self,
        weapon: Option<EntityId>,
        pressed: bool,
        held: bool,
        dt: Duration,
    ) -> MenuPress {
        if weapon != self.weapon {
            self.timer.cancel();
            self.weapon = weapon;
        }
        if weapon.is_none() {
            self.timer.cancel();
            return MenuPress::None;
        }
        if pressed {
            self.timer.press();
        }
        if held {
            self.timer.tick(dt)
        } else {
            self.timer.release()
        }
    }
}

#[derive(Default)]
pub(crate) struct WeaponButtons {
    presses: [Press; 2],
}
impl WeaponButtons {
    pub fn cancel(&mut self, world: &World) {
        for press in &mut self.presses {
            press.timer.cancel();
            press.weapon = None;
        }
        EjectProgress::publish(world, EjectProgress::default());
    }
    pub fn update(
        &mut self,
        world: &World,
        actions: &mut InputActionState,
        dt: Duration,
        allowed: bool,
        free_camera: bool,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut progress = Vec::new();
        for (i, (hand, action)) in [
            (Handedness::Left, InputAction::LeftHandUpperButton),
            (Handedness::Right, InputAction::RightHandUpperButton),
        ]
        .into_iter()
        .enumerate()
        {
            let gun = (allowed
                && !(free_camera && i == 1)
                && crate::hand_buttons::held_kind_in_hand(world, hand) == HeldKind::Gun)
                .then(|| crate::wielded_weapon::weapon_in_hand(world, hand))
                .flatten();
            let result = self.presses[i].update(
                gun,
                actions.just_triggered(action),
                actions.is_held(action),
                dt,
            );
            if let Some(weapon) = gun {
                actions.consume_trigger(action);
                match result {
                    MenuPress::Short | MenuPress::Long => effects.push(Effect::HeldGunButton {
                        hand,
                        weapon,
                        long_press: result == MenuPress::Long,
                    }),
                    MenuPress::None => {}
                }
                if crate::mission::reload::world_clip_offer(world, weapon).is_some()
                    && let Some(p) = self.presses[i].timer.progress()
                {
                    progress.push((weapon, p));
                }
            }
        }
        EjectProgress::publish(world, EjectProgress(progress));
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tap_and_hold_are_exclusive_and_hold_fires_once() {
        let mut world = World::new();
        let gun = Some(world.add_entity(()));
        let mut p = Press::default();
        assert_eq!(p.update(gun, true, true, Duration::ZERO), MenuPress::None);
        assert_eq!(
            p.update(gun, false, false, Duration::ZERO),
            MenuPress::Short
        );
        for i in 0..7 {
            assert_eq!(
                p.update(gun, i == 0, true, Duration::from_millis(100)),
                if i == 4 {
                    MenuPress::Long
                } else {
                    MenuPress::None
                }
            );
        }
        assert_eq!(p.update(gun, false, false, Duration::ZERO), MenuPress::None);
    }
    #[test]
    fn cancellation_clears_both_hands_even_if_the_next_scene_reuses_ids() {
        let mut world = World::new();
        let gun = world.add_entity(());
        let mut buttons = WeaponButtons::default();
        for press in &mut buttons.presses {
            press.update(Some(gun), true, true, Duration::from_millis(100));
        }
        EjectProgress::publish(&world, EjectProgress(vec![(gun, 0.2)]));
        buttons.cancel(&world);
        assert_eq!(EjectProgress::for_weapon(&world, gun), None);
        for press in &mut buttons.presses {
            for _ in 0..8 {
                assert_eq!(
                    press.update(Some(gun), false, true, Duration::from_millis(100)),
                    MenuPress::None
                );
            }
            assert_eq!(
                press.update(Some(gun), false, false, Duration::ZERO),
                MenuPress::None
            );
        }
    }

    #[test]
    fn changing_weapon_or_context_cancels_without_rearming() {
        let mut world = World::new();
        let a = Some(world.add_entity(()));
        let b = Some(world.add_entity(()));
        let mut p = Press::default();
        p.update(a, true, true, Duration::from_millis(100));
        for _ in 0..8 {
            assert_eq!(
                p.update(b, false, true, Duration::from_millis(100)),
                MenuPress::None
            );
        }
        assert_eq!(p.update(b, false, false, Duration::ZERO), MenuPress::None);
        p.update(a, true, true, Duration::ZERO);
        p.update(None, false, true, Duration::ZERO);
        assert_eq!(p.update(a, false, false, Duration::ZERO), MenuPress::None);
    }
}
