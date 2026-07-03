//! The psi amp: casts the player's selected psi power on trigger pull.
//!
//! Unlike a gun, the amp itself carries no `Projectile` links - the selected
//! power template (see [`crate::psi`]) supplies the projectile, scaled to the
//! caster's PSI stat. Casting costs psi points (the power's tier); a cast
//! with insufficient points fizzles.
//!
//! Only projectile ("shot") powers actually fire so far - sustained, shield,
//! and cursor-targeted powers are logged and skipped without spending points.

use engine::{audio::AudioHandle, game_log};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    psi::{GlobalPsiPowers, PsiPowerSelection},
};

use super::{
    Effect, MessagePayload, Script,
    script_util::play_environmental_sound,
    weapon_script::{create_muzzle_flash, create_projectile},
};

/// The caster's effective PSI stat, used to pick the power's projectile
/// (links are ordered by PSI level 1..8) - a mid-range placeholder until
/// player stats are tracked (P$BaseStats authors all stats at 1, pending
/// character creation/training).
const EFFECTIVE_PSI_STAT: i32 = 5;

pub struct PsiAmpScript;

impl PsiAmpScript {
    pub fn new() -> PsiAmpScript {
        PsiAmpScript
    }
}

impl Script for PsiAmpScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TriggerPull => cast_selected_power(world, entity_id),
            _ => Effect::NoEffect,
        }
    }
}

fn cast_selected_power(world: &World, amp_entity: EntityId) -> Effect {
    let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().unwrap();
    let selection = world.borrow::<UniqueView<PsiPowerSelection>>().unwrap();
    let Some(power) = powers.0.get(selection.index) else {
        return Effect::NoEffect;
    };

    // Gate on the player's psi pool: a cast costs the power's tier in points.
    // (The check here and the deduction - Effect::SpendPsiPoints - are split
    // across effect processing; that's safe because TriggerPull is rising-edge
    // only, so at most one cast enters a frame's effect batch.)
    let psi_points = {
        let player_info = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_psi = world
            .borrow::<View<dark::properties::PropPsiState>>()
            .unwrap();
        v_psi
            .get(player_info.entity_id)
            .map(|p| p.psi_points)
            .unwrap_or(0)
    };
    if psi_points < power.power.psi_cost {
        game_log!(
            INFO,
            "Not enough psi points for {} ({} < {})",
            power.name,
            psi_points,
            power.power.psi_cost
        );
        return Effect::NoEffect;
    }

    let Some(projectile_template) = power.projectile_for_psi_stat(EFFECTIVE_PSI_STAT) else {
        game_log!(
            INFO,
            "Psi power {} (activation type {}) is not implemented yet",
            power.name,
            power.power.activation_type
        );
        return Effect::NoEffect;
    };

    // The amp's GunFlash links supply the cast visual (Spinning Psi Ring).
    let flashes =
        super::script_util::get_all_links_with_template(world, amp_entity, |link| match link {
            dark::properties::Link::GunFlash(data) => Some(*data),
            _ => None,
        });

    let mut effects = vec![
        play_environmental_sound(world, amp_entity, "shoot", vec![], AudioHandle::new()),
        Effect::SpendPsiPoints {
            amount: power.power.psi_cost,
        },
        create_projectile(
            world,
            amp_entity,
            projectile_template,
            // Options are unused by projectile creation (they select the
            // link, which projectile_for_psi_stat already did).
            &dark::properties::ProjectileOptions {
                order: 0,
                setting: 0,
            },
        ),
    ];
    effects.extend(flashes.into_iter().map(|(template_id, options)| {
        create_muzzle_flash(world, amp_entity, template_id, &options)
    }));

    game_log!(
        INFO,
        "Cast psi power: {} (tier {})",
        power.name,
        power.power.psi_cost
    );
    Effect::Multiple(effects)
}
