//! Photonic Redirection's local visual feedback. Gameplay sight checks remain
//! separate: this fades only the caster's hands and held items, never the HUD.
use engine::scene::SceneObject;
use shipyard::{UniqueView, World};

pub(crate) fn transparency(world: &World) -> Option<f32> {
    let active = world
        .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
        .ok()?;
    let power = active
        .0
        .iter()
        .find(|p| p.template_id == crate::psi::INVISO_TEMPLATE_ID)?;
    fade(power.remaining_secs)
}

fn fade(remaining: f32) -> Option<f32> {
    // A readable ghost while active, returning smoothly during the last 3s.
    (remaining > 0.0).then(|| 0.78 * (remaining / 3.0).clamp(0.0, 1.0))
}

pub(crate) fn apply(object: &mut SceneObject, transparency: Option<f32>) {
    if let Some(alpha) = transparency {
        let authored = object.effective_transparency().unwrap_or(0.0);
        object.set_transparency(Some(authored.max(alpha)));
        object.set_depth_write(false);
    }
}

/// Only an attack by a weapon in the player's hands reveals the player.
/// NPC fire, dropped props, empty triggers and grip adjustments are excluded.
pub(crate) fn attack_effect(world: &World, weapon: shipyard::EntityId) -> crate::scripts::Effect {
    let held = world
        .borrow::<UniqueView<crate::mission::PlayerInfo>>()
        .is_ok_and(|p| {
            p.left_hand_entity_id == Some(weapon) || p.right_hand_entity_id == Some(weapon)
        });
    if held && transparency(world).is_some() {
        crate::scripts::Effect::DeactivatePsiPower {
            template_id: crate::psi::INVISO_TEMPLATE_ID,
        }
    } else {
        crate::scripts::Effect::NoEffect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_players_held_weapon_reveals_invisibility() {
        let mut world = World::new();
        let player = world.add_entity(());
        let held = world.add_entity(());
        let other = world.add_entity(());
        world.add_unique(crate::mission::PlayerInfo {
            pos: cgmath::Vector3::new(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: Some(held),
        });
        world.add_unique(crate::psi::ActivePsiPowers(vec![
            crate::psi::ActivePsiPower {
                template_id: crate::psi::INVISO_TEMPLATE_ID,
                name: "Inviso".into(),
                remaining_secs: 20.0,
            },
        ]));
        assert!(matches!(
            attack_effect(&world, other),
            crate::scripts::Effect::NoEffect
        ));
        assert!(matches!(
            attack_effect(&world, held),
            crate::scripts::Effect::DeactivatePsiPower {
                template_id: crate::psi::INVISO_TEMPLATE_ID
            }
        ));
        world
            .borrow::<shipyard::UniqueViewMut<crate::psi::ActivePsiPowers>>()
            .unwrap()
            .0
            .clear();
        assert!(matches!(
            attack_effect(&world, held),
            crate::scripts::Effect::NoEffect
        ));
    }
}
