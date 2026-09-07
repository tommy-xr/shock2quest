//! The psi amp: casts the player's selected psi power on trigger pull, with
//! hold-to-overload for the powers that support it.
//!
//! Unlike a gun, the amp itself carries no `Projectile` links - the selected
//! power template (see [`crate::psi`]) supplies the projectile, scaled to the
//! caster's PSI stat. Casting costs psi points (the power's tier); a cast
//! with insufficient points fizzles.
//!
//! Overloadable powers cast on trigger *release*: holding fills a charge bar
//! (faster for higher tiers - see [`crate::psi::charge_duration_secs`]).
//! Releasing in the end zone casts at +2 effective PSI; over-holding past a
//! full bar is a psi burnout - the cast fails, the points are spent, and the
//! player takes damage. Non-overloadable powers cast immediately on pull.
//!
//! Projectile ("shot"), sustained, and instant self-targeted powers cast so
//! far - the remaining kinds are logged and skipped without spending points.

use engine::{audio::AudioHandle, game_log};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    psi::{self, GlobalPsiPowers, PsiPowerInfo, PsiPowerSelection, charge_duration_secs},
    runtime_props::PsiChargePhase,
    time::Time,
};

use super::{
    Effect, MessagePayload, Script,
    script_util::play_environmental_sound,
    weapon_script::{create_muzzle_flash, create_projectile},
};

/// The caster's PSI stat from the character sheet, used to pick the power's
/// projectile (links are ordered by PSI level 1..8) and to scale sustained
/// durations. Falls back to the sheet's baseline when the scene has no
/// `QuestInfo` (a bare debug scene) - logged, because the same borrow also
/// fails if something else holds `QuestInfo` mutably, and a silent fallback
/// would cast at the wrong tier.
fn player_psi_stat(world: &World) -> i32 {
    match world.borrow::<UniqueView<crate::quest_info::QuestInfo>>() {
        Ok(quests) => quests
            .player_stats()
            .stat_level(crate::player_stats::Stat::PsionicAbility),
        Err(err) => {
            let baseline = crate::player_stats::PlayerStats::default().psionic_ability;
            game_log!(
                WARN,
                "No character sheet for the psi cast ({}); casting at PSI {}",
                err,
                baseline
            );
            baseline
        }
    }
}

/// The amp's charge/result state while the meter is on screen.
enum ChargeState {
    /// Trigger held on an overloadable power: the bar is filling.
    Charging {
        elapsed: f32,
        duration: f32,
        /// The power the charge was started for - if the selection changes
        /// mid-hold (CyclePsiPower), the charge cancels rather than casting
        /// a power it wasn't timed for.
        power_template_id: i32,
    },
    /// The charge resolved (cast or burnout); the result flashes briefly.
    ResultFlash { remaining: f32 },
}

pub struct PsiAmpScript {
    charge: Option<ChargeState>,
}

impl PsiAmpScript {
    pub fn new() -> PsiAmpScript {
        PsiAmpScript { charge: None }
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
            MessagePayload::TriggerPull => {
                let Some(power) = selected_power(world) else {
                    return Effect::NoEffect;
                };
                // Untrained powers can't be cast (selection gating should
                // make this unreachable, but don't start a charge - which
                // can burn out into damage - for a power the player doesn't
                // know).
                if !power_is_known(world, power.template_id) {
                    game_log!(INFO, "Psi power {} is not trained", power.name);
                    return Effect::NoEffect;
                }
                // Hold-to-overload runs only in flat presentation (the meter
                // renders on the flat HUD; a flat-aimed amp carries
                // RuntimePropFlatAim). In VR the amp keeps cast-on-pull until
                // a VR meter exists - charging blind would spend points with
                // no feedback.
                let is_flat = world
                    .borrow::<View<crate::runtime_props::RuntimePropFlatAim>>()
                    .map(|v| v.get(entity_id).is_ok())
                    .unwrap_or(false);
                if power.overloadable && is_flat {
                    // No points, no charge: gate up front so a broke caster
                    // can't charge into a burnout (which spends points and
                    // deals damage) they couldn't afford as a cast.
                    if player_psi_points(world) < power.power.psi_cost {
                        game_log!(
                            INFO,
                            "Not enough psi points for {} ({} < {})",
                            power.name,
                            player_psi_points(world),
                            power.power.psi_cost
                        );
                        return Effect::NoEffect;
                    }
                    // Nor charge a cast that would resolve to nothing (a heal
                    // at full health): over-holding it would burn out for a
                    // cast the release refuses anyway.
                    if instant_cast_is_futile(world, &power, player_psi_stat(world)) {
                        game_log!(INFO, "{} would do nothing right now", power.name);
                        return Effect::NoEffect;
                    }
                    // Cast happens on release; start the meter.
                    let duration = charge_duration_secs(power.power.psi_cost);
                    self.charge = Some(ChargeState::Charging {
                        elapsed: 0.0,
                        duration,
                        power_template_id: power.template_id,
                    });
                    Effect::SetPsiCharge {
                        entity_id,
                        fraction: 0.0,
                        phase: PsiChargePhase::Charging,
                    }
                } else {
                    cast_selected_power(world, entity_id, player_psi_stat(world))
                }
            }
            MessagePayload::TriggerRelease => {
                let Some(ChargeState::Charging {
                    elapsed,
                    duration,
                    power_template_id,
                }) = self.charge
                else {
                    return Effect::NoEffect;
                };
                // The selection changed mid-hold: the bar wasn't timed for
                // the now-selected power, so the charge fizzles. (A cycle
                // and a release landing on the SAME frame still cast the
                // held power - the selection step is an effect applied after
                // message dispatch - which matches the player's intent: they
                // charged that power the whole hold.)
                if selected_power(world).map(|p| p.template_id) != Some(power_template_id) {
                    self.charge = None;
                    return Effect::ClearPsiCharge { entity_id };
                }
                let fraction = elapsed / duration;
                let overload = fraction >= psi::OVERLOAD_ZONE_START;
                let effective_psi = psi::effective_psi_for_cast(player_psi_stat(world), overload);
                let cast = cast_selected_power(world, entity_id, effective_psi);
                // A successful overload flashes the success art briefly; a
                // normal cast - or a fizzle (no psi / power not implemented)
                // - just drops the meter.
                let meter = if overload && !matches!(cast, Effect::NoEffect) {
                    self.charge = Some(ChargeState::ResultFlash {
                        remaining: psi::CHARGE_RESULT_FLASH_SECS,
                    });
                    Effect::SetPsiCharge {
                        entity_id,
                        fraction: fraction.min(1.0),
                        phase: PsiChargePhase::Overloaded,
                    }
                } else {
                    self.charge = None;
                    Effect::ClearPsiCharge { entity_id }
                };
                Effect::Multiple(vec![meter, cast])
            }
            // Losing the amp mid-charge (weapon swap, drop, holster) cancels
            // the charge - otherwise the holstered amp would keep charging
            // and later "burn out" on its own.
            MessagePayload::Drop => {
                if self.charge.is_some() {
                    self.charge = None;
                    Effect::ClearPsiCharge { entity_id }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let dt = time.elapsed.as_secs_f32();
        match &mut self.charge {
            None => Effect::NoEffect,
            Some(ChargeState::Charging {
                elapsed,
                duration,
                power_template_id,
            }) => {
                let power_template_id = *power_template_id;
                *elapsed += dt;
                let fraction = *elapsed / *duration;
                if selected_power(world).map(|p| p.template_id) != Some(power_template_id) {
                    // Selection changed mid-hold: the charge fizzles.
                    self.charge = None;
                    Effect::ClearPsiCharge { entity_id }
                } else if fraction > 1.0 {
                    // Held past a full bar: psi burnout. The cast fails, the
                    // points are spent anyway, and the player takes damage.
                    self.charge = Some(ChargeState::ResultFlash {
                        remaining: psi::CHARGE_RESULT_FLASH_SECS,
                    });
                    burnout(world, entity_id)
                } else {
                    Effect::SetPsiCharge {
                        entity_id,
                        fraction,
                        phase: PsiChargePhase::Charging,
                    }
                }
            }
            Some(ChargeState::ResultFlash { remaining }) => {
                *remaining -= dt;
                if *remaining <= 0.0 {
                    self.charge = None;
                    Effect::ClearPsiCharge { entity_id }
                } else {
                    Effect::NoEffect
                }
            }
        }
    }
}

fn selected_power(world: &World) -> Option<PsiPowerInfo> {
    let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().ok()?;
    let selection = world.borrow::<UniqueView<PsiPowerSelection>>().ok()?;
    powers.0.get(selection.index).cloned()
}

/// The player's current psi points (0 when the player has no psi pool).
fn player_psi_points(world: &World) -> i32 {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let v_psi = world
        .borrow::<View<dark::properties::PropPsiState>>()
        .unwrap();
    v_psi
        .get(player_info.entity_id)
        .map(|p| p.psi_points)
        .unwrap_or(0)
}

/// Resolve a psi burnout: the power fails, its points are spent, and the
/// player takes damage (3 per tier - PSI/Endurance mitigation comes later
/// with player stats). The meter flashes red.
fn burnout(world: &World, amp_entity: EntityId) -> Effect {
    let Some(power) = selected_power(world) else {
        return Effect::ClearPsiCharge {
            entity_id: amp_entity,
        };
    };
    let player_entity = world.borrow::<UniqueView<PlayerInfo>>().unwrap().entity_id;
    let damage = psi::BURNOUT_DAMAGE_PER_TIER * power.power.psi_cost;
    game_log!(
        WARN,
        "Psi burnout! {} failed ({} damage)",
        power.name,
        damage
    );
    Effect::Multiple(vec![
        Effect::SetPsiCharge {
            entity_id: amp_entity,
            fraction: 1.0,
            phase: PsiChargePhase::Burnout,
        },
        Effect::SpendPsiPoints {
            amount: power.power.psi_cost,
        },
        // Direct HP adjustment: the player entity has no scripts, so a
        // Damage message would be dropped - AdjustHitPoints edits the
        // (template-seeded) PropHitPoints component directly.
        Effect::AdjustHitPoints {
            entity_id: player_entity,
            delta: -damage,
        },
    ])
}

/// Whether the player has been trained in the power. Selection gating
/// (`Effect::StepPsiSelection`) means an untrained power should never be
/// selected; this is the belt-and-braces check on the cast paths. Fails
/// closed: the unique is seeded unconditionally at mission load, so a
/// missing one is a setup bug - don't let it disable the gate.
fn power_is_known(world: &World, template_id: i32) -> bool {
    world
        .borrow::<UniqueView<crate::psi::PlayerPsiKnownPowers>>()
        .map(|known| known.0.contains(&template_id))
        .unwrap_or(false)
}

fn cast_selected_power(world: &World, amp_entity: EntityId, effective_psi: i32) -> Effect {
    let Some(power) = selected_power(world) else {
        return Effect::NoEffect;
    };
    if !power_is_known(world, power.template_id) {
        game_log!(INFO, "Psi power {} is not trained", power.name);
        return Effect::NoEffect;
    }

    // Gate on the player's psi pool: a cast costs the power's tier in points.
    // (The check here and the deduction - Effect::SpendPsiPoints - are split
    // across effect processing; that's safe because TriggerPull is rising-edge
    // only, so at most one cast enters a frame's effect batch.)
    let psi_points = player_psi_points(world);
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

    // Sustained (timed) powers activate a player status for a data-driven
    // duration instead of firing a projectile.
    if power.power.activation_type == psi::ACTIVATION_TYPE_SUSTAINED {
        return cast_sustained_power(world, amp_entity, &power, effective_psi);
    }

    // Instant (self-targeted) powers resolve immediately - no duration, no
    // projectile.
    if power.power.activation_type == psi::ACTIVATION_TYPE_INSTANT {
        return cast_instant_power(world, amp_entity, &power, effective_psi);
    }

    let Some(projectile_template) = power.projectile_for_psi_stat(effective_psi) else {
        game_log!(
            INFO,
            "Psi power {} (activation type {}) is not implemented yet",
            power.name,
            power.power.activation_type
        );
        return Effect::NoEffect;
    };

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
            // A psi bolt is not a gun shot: the amp's fire-setting record is
            // the editor's uninitialized one (every multiplier 0), so cast at
            // the projectile's own authored damage and speed.
            crate::runtime_props::RuntimePropShotModifiers::default(),
        ),
    ];
    effects.extend(amp_cast_flashes(world, amp_entity));

    game_log!(
        INFO,
        "Cast psi power: {} (tier {}, effective PSI {})",
        power.name,
        power.power.psi_cost,
        effective_psi
    );
    Effect::Multiple(effects)
}

/// Cast a sustained (activation type 1) power: spend the tier, activate the
/// player status for `duration_base + duration_per_psi × PSI` seconds (from
/// the power's `P$PsiShield` data), and play the amp's cast flash/sound like
/// a projectile cast. Re-casting an active power spends again and refreshes
/// the duration.
fn cast_sustained_power(
    world: &World,
    amp_entity: EntityId,
    power: &PsiPowerInfo,
    effective_psi: i32,
) -> Effect {
    let Some(duration) = &power.duration else {
        game_log!(
            INFO,
            "Sustained psi power {} has no duration data (P$PsiShield) - not implemented yet",
            power.name
        );
        return Effect::NoEffect;
    };
    let duration_secs = (duration.duration_base + duration.duration_per_psi * effective_psi) as f32;

    let mut effects = vec![
        play_environmental_sound(world, amp_entity, "shoot", vec![], AudioHandle::new()),
        Effect::SpendPsiPoints {
            amount: power.power.psi_cost,
        },
        Effect::ActivatePsiPower {
            template_id: power.template_id,
            name: power.name.clone(),
            duration_secs,
        },
    ];
    effects.extend(amp_cast_flashes(world, amp_entity));

    game_log!(
        INFO,
        "Cast psi power: {} (tier {}, sustained {}s)",
        power.name,
        power.power.psi_cost,
        duration_secs
    );
    Effect::Multiple(effects)
}

/// Cast an instant (activation type 2) power: it resolves on the spot, with
/// no duration and no projectile. Dispatch is by template id so the remaining
/// instant powers (Major Heal, SomaDrain, ForceWall, CyberHack) slot in
/// beside this one.
fn cast_instant_power(
    world: &World,
    amp_entity: EntityId,
    power: &PsiPowerInfo,
    effective_psi: i32,
) -> Effect {
    match power.template_id {
        // Cerebro-stimulated Regeneration: restores the caster's health.
        psi::PSI_HEAL_TEMPLATE_ID => cast_self_heal(world, amp_entity, power, effective_psi),
        _ => {
            game_log!(
                INFO,
                "Instant psi power {} is not implemented yet",
                power.name
            );
            Effect::NoEffect
        }
    }
}

/// Heal the caster by [`self_heal_amount`], spending the power's tier. A cast
/// at full health is refused and spends nothing, matching the empty-pool
/// guard - the player keeps their points rather than burning them on a cast
/// that could do nothing.
fn cast_self_heal(
    world: &World,
    amp_entity: EntityId,
    power: &PsiPowerInfo,
    effective_psi: i32,
) -> Effect {
    let Some((player_entity, current_hp, max_hp)) = super::script_util::player_hit_points(world)
    else {
        game_log!(WARN, "No player hit points for {}", power.name);
        return Effect::NoEffect;
    };
    let amount = self_heal_amount(&power.power.data, effective_psi);
    if amount <= 0 {
        game_log!(INFO, "{} has no heal data (P$PsiPower)", power.name);
        return Effect::NoEffect;
    }
    let heal = clamped_self_heal(&power.power.data, effective_psi, current_hp, max_hp);
    if heal <= 0 {
        game_log!(
            INFO,
            "{} would heal nothing (already at full health)",
            power.name
        );
        return Effect::NoEffect;
    }

    let mut effects = vec![
        play_environmental_sound(world, amp_entity, "shoot", vec![], AudioHandle::new()),
        Effect::SpendPsiPoints {
            amount: power.power.psi_cost,
        },
        // PlayerScript handles only Damage - there is no heal message - so
        // adjust HP directly. The applier does not clamp to the maximum,
        // hence the clamp above.
        Effect::AdjustHitPoints {
            entity_id: player_entity,
            delta: heal,
        },
    ];
    effects.extend(amp_cast_flashes(world, amp_entity));

    game_log!(
        INFO,
        "Cast psi power: {} (tier {}, healed {} HP at effective PSI {})",
        power.name,
        power.power.psi_cost,
        heal,
        effective_psi
    );
    Effect::Multiple(effects)
}

/// The HP an instant self-heal restores at full strength (the caller clamps
/// it to the caster's missing health): `data[0] + data[1] x effective PSI`.
/// 0 for a power with no (or unusable) heal data.
///
/// Assumption: the two floats split base / per-PSI the way the sustained
/// powers' `P$PsiShield` duration data does. PsiHeal's `[0, 2]` therefore
/// reads as 2 HP per point of PSI (Major Heal's `[5, 5]` as 5 + 5 x PSI).
fn self_heal_amount(data: &[f32; 4], effective_psi: i32) -> i32 {
    let amount = data[0] + data[1] * effective_psi as f32;
    if !amount.is_finite() || amount <= 0.0 {
        return 0;
    }
    amount.floor().min(i32::MAX as f32) as i32
}

/// Whether an instant cast would resolve to nothing right now (a heal with
/// no health missing). Checked before a charge starts so a pointless cast
/// cannot be over-held into a burnout, which spends points and deals damage.
fn instant_cast_is_futile(world: &World, power: &PsiPowerInfo, effective_psi: i32) -> bool {
    if power.template_id != psi::PSI_HEAL_TEMPLATE_ID {
        return false;
    }
    let Some((_, current_hp, max_hp)) = super::script_util::player_hit_points(world) else {
        return false;
    };
    clamped_self_heal(&power.power.data, effective_psi, current_hp, max_hp) <= 0
}

/// The HP a cast actually restores: [`self_heal_amount`] clamped to the
/// caster's missing health.
fn clamped_self_heal(data: &[f32; 4], effective_psi: i32, current_hp: i32, max_hp: i32) -> i32 {
    self_heal_amount(data, effective_psi).min((max_hp - current_hp).max(0))
}

/// The amp's `GunFlash` links supply the cast visual (Spinning Psi Ring).
fn amp_cast_flashes(world: &World, amp_entity: EntityId) -> Vec<Effect> {
    super::script_util::get_all_links_with_template(world, amp_entity, |link| match link {
        dark::properties::Link::GunFlash(data) => Some(*data),
        _ => None,
    })
    .into_iter()
    .map(|(template_id, options)| create_muzzle_flash(world, amp_entity, template_id, &options))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;

    #[test]
    fn self_heal_scales_with_psi() {
        // PsiHeal's authored data: 0 base + 2 HP per PSI point.
        assert_eq!(self_heal_amount(&[0.0, 2.0, 0.0, 0.0], 5), 10);
        assert_eq!(self_heal_amount(&[0.0, 2.0, 0.0, 0.0], 7), 14);
        // A base term adds on top of the per-PSI term (Major Heal's shape).
        assert_eq!(self_heal_amount(&[5.0, 5.0, 0.0, 0.0], 5), 30);
    }

    #[test]
    fn self_heal_ignores_powers_with_no_heal_data() {
        assert_eq!(self_heal_amount(&[0.0, 0.0, 0.0, 0.0], 5), 0);
        assert_eq!(self_heal_amount(&[f32::NAN, 2.0, 0.0, 0.0], 5), 0);
        assert_eq!(self_heal_amount(&[-4.0, 0.0, 0.0, 0.0], 5), 0);
    }

    /// The applier does not clamp, so an overshoot would push HP past the
    /// maximum - the clamp lives here instead.
    #[test]
    fn the_heal_is_clamped_to_missing_health() {
        // 2 x PSI 5 = 10, but only 4 HP are missing.
        assert_eq!(clamped_self_heal(&[0.0, 2.0, 0.0, 0.0], 5, 96, 100), 4);
        // At full health the heal is nothing (the cast is refused).
        assert_eq!(clamped_self_heal(&[0.0, 2.0, 0.0, 0.0], 5, 100, 100), 0);
        // Over-healed (or a bogus maximum) never produces a negative delta.
        assert_eq!(clamped_self_heal(&[0.0, 2.0, 0.0, 0.0], 5, 120, 100), 0);
    }

    #[test]
    fn psi_stat_comes_from_the_character_sheet() {
        let world = World::new();
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().psionic_ability = 6;
        world.add_unique(quests);

        assert_eq!(player_psi_stat(&world), 6);
    }

    #[test]
    fn psi_stat_falls_back_to_the_sheet_baseline_without_quest_info() {
        let world = World::new();

        assert_eq!(
            player_psi_stat(&world),
            crate::player_stats::PlayerStats::default().psionic_ability
        );
    }
}
