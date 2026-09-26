//! An amp owns its quick-switch pair; the player owns knowledge and psi points.
use crate::psi::{GlobalPsiPowers, PlayerPsiKnownPowers, PsiPowerInfo};
use serde::{Deserialize, Serialize};
use shipyard::{Component, EntityId, Get, UniqueView, View, World};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpSelection {
    pub current: i32,
    pub alternate: Option<i32>,
}
impl AmpSelection {
    pub fn select(&mut self, template: i32) {
        if self.current != template {
            self.alternate = Some(self.current);
            self.current = template;
        }
    }
    pub fn swap(&mut self) {
        if let Some(alternate) = self.alternate {
            self.alternate = Some(self.current);
            self.current = alternate;
        }
    }
}
pub fn selection(world: &World, amp: EntityId) -> Option<AmpSelection> {
    if let Ok(v) = world.borrow::<View<AmpSelection>>() {
        if let Ok(value) = v.get(amp) {
            return Some(*value);
        }
    }
    let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().ok()?;
    let known = world.borrow::<UniqueView<PlayerPsiKnownPowers>>().ok();
    let trained = |p: &&PsiPowerInfo| known.as_ref().is_some_and(|k| k.0.contains(&p.template_id));
    let first = powers
        .0
        .iter()
        .filter(trained)
        .find(|p| p.template_id == crate::psi::CRYOKINESIS_TEMPLATE_ID)
        .or_else(|| powers.0.iter().find(trained))
        .or_else(|| {
            powers
                .0
                .iter()
                .find(|p| p.template_id == crate::psi::CRYOKINESIS_TEMPLATE_ID)
        })
        .or_else(|| powers.0.first())?;
    Some(AmpSelection {
        current: first.template_id,
        alternate: None,
    })
}
/// Freeze the initial choice when this amp first enters a hand. Learning more
/// powers later must not silently change an already configured amp.
pub fn initialize(world: &mut World, amp: EntityId) {
    if world
        .borrow::<View<AmpSelection>>()
        .is_ok_and(|v| v.get(amp).is_ok())
    {
        return;
    }
    if let Some(pair) = selection(world, amp) {
        world.add_component(amp, pair);
    }
}
pub fn selected_power(world: &World, amp: EntityId) -> Option<PsiPowerInfo> {
    let selected = selection(world, amp)?;
    world
        .borrow::<UniqueView<GlobalPsiPowers>>()
        .ok()?
        .0
        .iter()
        .find(|p| p.template_id == selected.current)
        .cloned()
}
/// Legacy keyboard/MFD controls use the ordinary right-then-left amp resolver.
pub use crate::wielded_weapon::wielded_psi_amp as target;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn learning_an_earlier_power_does_not_change_a_used_amp() {
        let mut world = World::new();
        let amp = world.add_entity(());
        let powers = [-1, -2].map(|template_id| PsiPowerInfo {
            template_id,
            name: String::new(),
            display_name: None,
            power: dark::properties::PropPsiPower {
                power_id: -template_id,
                activation_type: 0,
                psi_cost: 1,
                data: [0.0; 4],
            },
            projectiles: vec![],
            overloadable: false,
            duration: None,
        });
        world.add_unique(GlobalPsiPowers(powers.to_vec()));
        world.add_unique(PlayerPsiKnownPowers(std::collections::HashSet::from([-2])));
        initialize(&mut world, amp);
        world
            .borrow::<shipyard::UniqueViewMut<PlayerPsiKnownPowers>>()
            .unwrap()
            .0
            .insert(-1);
        initialize(&mut world, amp);
        assert_eq!(selection(&world, amp).unwrap().current, -2);
    }
    #[test]
    fn selecting_commits_previous_power_and_swapping_preserves_the_pair() {
        let mut a = AmpSelection {
            current: -1,
            alternate: None,
        };
        let b = a;
        a.select(-2);
        a.select(-2);
        assert_eq!(a.alternate, Some(-1));
        a.swap();
        assert_eq!(a.current, -1);
        assert_eq!(a.alternate, Some(-2));
        a.select(-3);
        assert_eq!(a.alternate, Some(-1));
        assert_eq!(b.current, -1);
        assert_eq!(b.alternate, None);
        assert_eq!(
            serde_json::from_str::<AmpSelection>(&serde_json::to_string(&a).unwrap()).unwrap(),
            a
        );
    }
}
