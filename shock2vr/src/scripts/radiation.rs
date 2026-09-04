use dark::properties::{PropRadiationAbsorb, PropRadiationRecovery, ReceptronOptions};
use serde::{Deserialize, Serialize};
use shipyard::{Get, Unique, UniqueView, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld, quest_info::QuestInfo};

use super::{Effect, MessagePayload, Script};
use crate::scripts::gui::TRAIT_PHARMO_FRIENDLY;

/// Amount removed by retail `RadPatch`, recovered from the shipped
/// `allobjs` module. Pharmo-Friendly applies the same 20% item bonus as the
/// healing scripts, producing 7.2 instead of 6.0.
pub const RAD_PATCH_CLEAR_AMOUNT: f32 = 6.0;
pub const RAD_PATCH_PHARMO_CLEAR_AMOUNT: f32 = 7.2;

/// Retail schedules `RadDamage` every 6000 ms. The shipped implementation
/// converts the stored level to damage with a 0.25 multiplier before the
/// normal-difficulty scalar (shock2quest currently has no difficulty option).
pub const RADIATION_DAMAGE_INTERVAL_SECS: f32 = 6.0;
pub const RADIATION_CHECK_INTERVAL_SECS: f32 = 0.1;
const RADIATION_DAMAGE_PER_LEVEL: f32 = 0.25;
const DEFAULT_RADIATION_ABSORB: f32 = 0.05;
/// Fallback decontamination rate if `Rad Shield`'s authored `data[0]` is
/// missing, matching the shipped value.
const DEFAULT_DECONTAMINATION_PER_SEC: f32 = 5.0;
const DEFAULT_RADIATION_RECOVERY: f32 = 3.0;

/// Retail `RadPatch`: clear part of the player's accumulated radiation and
/// consume one patch only when radiation is actually present.
pub struct RadPatchScript;

impl RadPatchScript {
    fn pharmo_friendly(world: &World) -> bool {
        world
            .borrow::<UniqueView<QuestInfo>>()
            .is_ok_and(|quests| quests.player_stats().has_os_trait(TRAIT_PHARMO_FRIENDLY))
    }
}

impl Script for RadPatchScript {
    fn handle_message(
        &mut self,
        entity_id: shipyard::EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }

        let amount = if Self::pharmo_friendly(world) {
            RAD_PATCH_PHARMO_CLEAR_AMOUNT
        } else {
            RAD_PATCH_CLEAR_AMOUNT
        };
        Effect::UseRadiationPatch { entity_id, amount }
    }
}

fn default_damage_timer() -> f32 {
    RADIATION_DAMAGE_INTERVAL_SECS
}

fn default_check_timer() -> f32 {
    RADIATION_CHECK_INTERVAL_SECS
}

/// Player radiation survives after the source item or source effect is gone,
/// so it is global save state rather than script-private state. Radius sources
/// refresh `ambient_level` every frame; the retail 100 ms `RadCheck` timer then
/// raises stored level by the authored `RadAbsorb` step until ambient is met.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Unique)]
pub struct ActiveRadiation {
    level: f32,
    #[serde(default, skip)]
    ambient_level: f32,
    #[serde(default = "default_check_timer")]
    seconds_until_check: f32,
    #[serde(default = "default_damage_timer")]
    seconds_until_damage: f32,
}

impl Default for ActiveRadiation {
    fn default() -> Self {
        Self {
            level: 0.0,
            ambient_level: 0.0,
            seconds_until_check: RADIATION_CHECK_INTERVAL_SECS,
            seconds_until_damage: RADIATION_DAMAGE_INTERVAL_SECS,
        }
    }
}

impl ActiveRadiation {
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Refresh the final act/react ambient intensity after
    /// Amplify/Abort/Radiate. Multiple overlapping sources keep the strongest
    /// level for this frame, matching environmental exposure rather than
    /// summing a full source intensity into stored `RadLevel` every frame.
    pub fn observe_ambient(&mut self, amount: f32) -> bool {
        if !amount.is_finite() || amount <= 0.0 {
            return false;
        }
        self.ambient_level = self.ambient_level.max(amount);
        true
    }

    /// Remove one patch's amount. There is deliberately no protection timer:
    /// retail subtracts from `RadLevel` immediately, and later exposure starts
    /// accumulating again from the remaining level.
    pub fn clear(&mut self, amount: f32) -> bool {
        if !amount.is_finite() || amount <= 0.0 || !self.level.is_finite() || self.level <= 0.0 {
            return false;
        }
        self.level = (self.level - amount).max(0.0);
        true
    }

    /// Advance the retail 6-second damage/recovery clock and return ordinary
    /// hit-point damage for MissionCore's central HP effect handler.
    /// `decontamination_per_sec` is the active Neural Decontamination
    /// (`Rad Shield`) purge rate, or 0 when the power is not active.
    pub fn advance(
        &mut self,
        elapsed_secs: f32,
        absorb_per_check: f32,
        recovery_per_tick: f32,
        decontamination_per_sec: f32,
    ) -> i32 {
        if !elapsed_secs.is_finite() || elapsed_secs <= 0.0 {
            return 0;
        }
        if !self.level.is_finite() || self.level < 0.0 {
            self.level = 0.0;
        }
        if !self.ambient_level.is_finite() || self.ambient_level < 0.0 {
            self.ambient_level = 0.0;
        }
        if !self.seconds_until_check.is_finite()
            || self.seconds_until_check < -RADIATION_CHECK_INTERVAL_SECS
        {
            self.seconds_until_check = RADIATION_CHECK_INTERVAL_SECS;
        }
        if !self.seconds_until_damage.is_finite()
            || self.seconds_until_damage < -RADIATION_DAMAGE_INTERVAL_SECS
        {
            self.seconds_until_damage = RADIATION_DAMAGE_INTERVAL_SECS;
        }
        let absorb = if absorb_per_check.is_finite() && absorb_per_check > 0.0 {
            absorb_per_check
        } else {
            DEFAULT_RADIATION_ABSORB
        };
        let recovery = if recovery_per_tick.is_finite() && recovery_per_tick > 0.0 {
            recovery_per_tick
        } else {
            DEFAULT_RADIATION_RECOVERY
        };

        // Neural Decontamination: while the sustained power is active the
        // caster is sealed off - ambient exposure never reaches stored level,
        // the stored level bleeds off, and the 6 s clock deals no damage.
        // ASSUMPTION: the power's authored `data[0]` (5.0) reads as a
        // per-second purge rate; retail could instead mean a resistance
        // percentage. At 5/s any realistic level clears within ~2 s, which is
        // what "clears accumulated radiation" is asking for either way.
        let shielded = decontamination_per_sec.is_finite() && decontamination_per_sec > 0.0;
        if shielded {
            self.ambient_level = 0.0;
            self.clear(decontamination_per_sec * elapsed_secs);
        }

        self.seconds_until_check -= elapsed_secs;
        while self.seconds_until_check <= 0.0 {
            if self.ambient_level > self.level {
                self.level += absorb.min(self.ambient_level - self.level);
            }
            self.seconds_until_check += RADIATION_CHECK_INTERVAL_SECS;
        }

        self.seconds_until_damage -= elapsed_secs;
        let mut damage = 0_i32;
        while self.seconds_until_damage <= 0.0 {
            if !shielded && self.level >= 1.0 {
                let pulse = (self.level * RADIATION_DAMAGE_PER_LEVEL)
                    .trunc()
                    .clamp(0.0, i32::MAX as f32) as i32;
                damage = damage.saturating_add(pulse);
            }
            if self.ambient_level <= 0.0 {
                self.level = (self.level - recovery).max(0.0);
            }
            self.seconds_until_damage += RADIATION_DAMAGE_INTERVAL_SECS;
        }
        // Radius sources refresh this after the effect pass. Clearing it here
        // lets the following frame faithfully represent leaving the source.
        self.ambient_level = 0.0;
        damage
    }
}

/// Feed one target's authored radiation reaction into the live player status.
/// Production calls this only while applying `Effect::RadiusBlast`, keeping
/// mutation at the mission effect choke point.
pub fn apply_player_stimulus(
    world: &World,
    target: shipyard::EntityId,
    receptrons: &[(i32, ReceptronOptions)],
    stim_template_id: i32,
    intensity: f32,
) -> bool {
    let is_player = world
        .borrow::<UniqueView<PlayerInfo>>()
        .is_ok_and(|player| player.entity_id == target);
    if !is_player {
        return false;
    }
    let Some(amount) = crate::mission::stim_response::resolve_stim_radiation(
        receptrons,
        stim_template_id,
        intensity,
    ) else {
        return false;
    };
    world
        .borrow::<shipyard::UniqueViewMut<ActiveRadiation>>()
        .is_ok_and(|mut radiation| radiation.observe_ambient(amount))
}

/// The purge rate of an active Neural Decontamination (`Rad Shield`), read
/// from the power's authored `data[0]`, or 0 when it is not active. Kept in
/// the radiation tick rather than the psi code so it composes with the
/// anti-rad patch and any future radiation-absorb path.
fn active_decontamination_rate(world: &World) -> f32 {
    let active = world
        .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
        .is_ok_and(|active| active.is_active(crate::psi::RAD_SHIELD_TEMPLATE_ID));
    if !active {
        return 0.0;
    }
    world
        .borrow::<UniqueView<crate::psi::GlobalPsiPowers>>()
        .ok()
        .and_then(|powers| {
            powers
                .0
                .iter()
                .find(|p| p.template_id == crate::psi::RAD_SHIELD_TEMPLATE_ID)
                .map(|p| p.power.data[0])
        })
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .unwrap_or(DEFAULT_DECONTAMINATION_PER_SEC)
}

pub fn tick_player_radiation(world: &World, elapsed_secs: f32) -> Option<Effect> {
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?.entity_id;
    let recovery = world
        .borrow::<View<PropRadiationRecovery>>()
        .ok()
        .and_then(|values| values.get(player).ok().map(|value| value.0))
        .unwrap_or(DEFAULT_RADIATION_RECOVERY);
    let absorb = world
        .borrow::<View<PropRadiationAbsorb>>()
        .ok()
        .and_then(|values| values.get(player).ok().map(|value| value.0))
        .unwrap_or(DEFAULT_RADIATION_ABSORB);
    let decontamination = active_decontamination_rate(world);
    let damage = world
        .borrow::<shipyard::UniqueViewMut<ActiveRadiation>>()
        .ok()
        .map(|mut radiation| radiation.advance(elapsed_secs, absorb, recovery, decontamination))
        .unwrap_or(0);
    (damage > 0).then_some(Effect::AdjustHitPoints {
        entity_id: player,
        delta: -damage,
    })
}

#[cfg(test)]
mod tests {
    use dark::properties::{ReceptronEffect, ReceptronOptions};
    use shipyard::{UniqueView, World};

    use crate::{
        mission::PlayerInfo,
        physics::PhysicsWorld,
        quest_info::QuestInfo,
        scripts::{Effect, MessagePayload, Script},
    };
    use cgmath::{Quaternion, vec3};
    use serde_json::json;

    use super::{
        ActiveRadiation, RAD_PATCH_CLEAR_AMOUNT, RAD_PATCH_PHARMO_CLEAR_AMOUNT, RadPatchScript,
        apply_player_stimulus,
    };

    #[test]
    fn frob_routes_rad_patch_through_a_real_use_effect() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let patch = world.add_entity(());

        let effect = RadPatchScript.handle_message(
            patch,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(
            effect,
            Effect::UseRadiationPatch { entity_id, amount }
                if entity_id == patch && amount == RAD_PATCH_CLEAR_AMOUNT
        ));
    }

    #[test]
    fn pharmo_friendly_uses_retail_twenty_percent_bonus() {
        let mut world = World::new();
        let mut quests = QuestInfo::new();
        assert!(quests.player_stats_mut().add_os_trait(2));
        world.add_unique(quests);
        let patch = world.add_entity(());

        let effect = RadPatchScript.handle_message(
            patch,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(
            effect,
            Effect::UseRadiationPatch { amount, .. }
                if amount == RAD_PATCH_PHARMO_CLEAR_AMOUNT
        ));
    }

    #[test]
    fn patch_clears_level_without_granting_future_protection() {
        let mut radiation = ActiveRadiation::default();
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.1, 8.0, 3.0, 0.0), 0);
        assert!(radiation.clear(6.0));
        assert_eq!(radiation.level(), 2.0);

        assert!(radiation.observe_ambient(6.0));
        assert_eq!(radiation.advance(0.1, 4.0, 3.0, 0.0), 0);
        assert_eq!(radiation.level(), 6.0);
    }

    #[test]
    fn damage_and_recovery_follow_the_retail_six_second_clock() {
        let mut radiation = ActiveRadiation::default();
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.1, 8.0, 3.0, 0.0), 0);
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(5.89, 0.05, 3.0, 0.0), 0);
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.02, 0.05, 3.0, 0.0), 2);
        assert_eq!(radiation.level(), 8.0, "active exposure delays recovery");
        assert_eq!(radiation.advance(6.0, 0.05, 3.0, 0.0), 2);
        assert_eq!(radiation.level(), 5.0);
    }

    #[test]
    fn rad_shield_purges_and_blocks_while_active() {
        let mut radiation = ActiveRadiation::default();
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.1, 8.0, 3.0, 0.0), 0);
        assert_eq!(radiation.level(), 8.0);

        // One second of shielded exposure: ambient is ignored and 5.0 bleeds
        // off, so a level that would otherwise hold at 8 falls to 3.
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(1.0, 8.0, 3.0, 5.0), 0);
        assert_eq!(radiation.level(), 3.0);

        // ...and it reaches zero and stays there while the shield holds.
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(1.0, 8.0, 3.0, 5.0), 0);
        assert_eq!(radiation.level(), 0.0);

        // Expiry: exposure accumulates again from the shipped absorb step.
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.1, 0.05, 3.0, 0.0), 0);
        assert_eq!(radiation.level(), 0.05);
    }

    #[test]
    fn rad_shield_suppresses_the_six_second_damage_pulse() {
        let mut radiation = ActiveRadiation::default();
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.1, 8.0, 3.0, 0.0), 0);

        // Unshielded this tick deals 2 damage (see the retail-clock test).
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(6.0, 0.05, 3.0, 5.0), 0);
    }

    #[test]
    fn corrupt_restored_timers_are_normalized_without_looping() {
        let mut radiation: ActiveRadiation = serde_json::from_value(json!({
            "level": 8.0,
            "seconds_until_check": -1.0e30,
            "seconds_until_damage": -1.0e30
        }))
        .unwrap();

        assert_eq!(radiation.advance(1.0 / 60.0, 0.05, 3.0, 0.0), 0);
        assert_eq!(radiation.level(), 8.0);
    }

    #[test]
    fn authored_radiate_receptron_updates_only_the_player_status() {
        const RADIATION_STIM: i32 = -386;
        let mut world = World::new();
        let player = world.add_entity(());
        let other = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(ActiveRadiation::default());
        let receptrons = vec![(
            RADIATION_STIM,
            ReceptronOptions {
                order: 69,
                effect: ReceptronEffect::Radiate { multiplier: 1.0 },
            },
        )];

        assert!(!apply_player_stimulus(
            &world,
            other,
            &receptrons,
            RADIATION_STIM,
            8.0,
        ));
        assert!(apply_player_stimulus(
            &world,
            player,
            &receptrons,
            RADIATION_STIM,
            8.0,
        ));
        assert_eq!(
            world
                .borrow::<shipyard::UniqueViewMut<ActiveRadiation>>()
                .unwrap()
                .advance(0.1, 0.05, 3.0, 0.0),
            0
        );
        assert_eq!(
            world
                .borrow::<UniqueView<ActiveRadiation>>()
                .unwrap()
                .level(),
            0.05
        );
    }
}
