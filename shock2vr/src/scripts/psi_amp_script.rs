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
//! Only projectile ("shot") powers actually fire so far - sustained, shield,
//! and cursor-targeted powers are logged and skipped without spending points.

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

/// The caster's effective PSI stat, used to pick the power's projectile
/// (links are ordered by PSI level 1..8) - a mid-range placeholder until
/// player stats are tracked (P$BaseStats authors all stats at 1, pending
/// character creation/training).
const EFFECTIVE_PSI_STAT: i32 = 5;

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
                    cast_selected_power(world, entity_id, EFFECTIVE_PSI_STAT)
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
                // held power - CyclePsiPower is an effect applied after
                // message dispatch - which matches the player's intent: they
                // charged that power the whole hold.)
                if selected_power(world).map(|p| p.template_id) != Some(power_template_id) {
                    self.charge = None;
                    return Effect::ClearPsiCharge { entity_id };
                }
                let fraction = elapsed / duration;
                let overload = fraction >= psi::OVERLOAD_ZONE_START;
                let effective_psi = if overload {
                    (EFFECTIVE_PSI_STAT + psi::OVERLOAD_PSI_BONUS)
                        .min(psi::OVERLOAD_MAX_EFFECTIVE_PSI)
                } else {
                    EFFECTIVE_PSI_STAT
                };
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
/// (`Effect::CyclePsiPower`) means an untrained power should never be
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

    let Some(projectile_template) = power.projectile_for_psi_stat(effective_psi) else {
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
        "Cast psi power: {} (tier {}, effective PSI {})",
        power.name,
        power.power.psi_cost,
        effective_psi
    );
    Effect::Multiple(effects)
}
