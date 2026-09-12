use dark::properties::{PropRadiationAbsorb, PropRadiationRecovery, ReceptronOptions};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, Unique, UniqueView, UniqueViewMut, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld, quest_info::QuestInfo};

use super::{Effect, MessagePayload, Script};
use crate::scripts::gui::TRAIT_PHARMO_FRIENDLY;

/// Amount removed by retail `RadPatch`, recovered from the shipped
/// `allobjs` module. Pharmo-Friendly applies the same 20% item bonus as the
/// healing scripts, producing 7.2 instead of 6.0.
pub const RAD_PATCH_CLEAR_AMOUNT: f32 = 6.0;
pub const RAD_PATCH_PHARMO_CLEAR_AMOUNT: f32 = 7.2;

/// Retail schedules `RadDamage` every 6000 ms. The shipped implementation
/// multiplies stored level by the Endurance table and Metabolism modifier
/// before truncating to whole HP (allobjs 0x1001690d–0x100169c8).
pub const RADIATION_DAMAGE_INTERVAL_SECS: f64 = 6.0;
pub const RADIATION_CHECK_INTERVAL_SECS: f64 = 0.1;
const RADIATION_DAMAGE_PER_LEVEL: f32 = 1.0;
const DEFAULT_RADIATION_ABSORB: f32 = 0.05;
const DEFAULT_RADIATION_RECOVERY: f32 = 3.0;
const TOXIN_SHIELD_TEMPLATE_ID: i32 = -1113;
const RAD_SHIELD_TEMPLATE_ID: i32 = -1114;

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

fn default_toxin_timer() -> f64 {
    10.0
}

fn default_damage_timer() -> f64 {
    RADIATION_DAMAGE_INTERVAL_SECS
}

fn default_check_timer() -> f64 {
    RADIATION_CHECK_INTERVAL_SECS
}

/// Player radiation survives after the source item or source effect is gone,
/// so it is global save state rather than script-private state. Radius sources
/// refresh `ambient_level` every frame; the retail 100 ms `RadCheck` timer then
/// raises stored level by the authored `RadAbsorb` step until ambient is met.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Unique)]
pub struct ActiveRadiation {
    level: f32,
    #[serde(default)]
    toxin: f32,
    #[serde(skip)]
    last_radiation_damage: i32,
    #[serde(skip)]
    exposed_last_tick: bool,
    #[serde(default = "default_toxin_timer")]
    seconds_until_toxin: f64,
    #[serde(default, skip)]
    ambient_level: f32,
    #[serde(default = "default_check_timer")]
    seconds_until_check: f64,
    #[serde(default = "default_damage_timer")]
    seconds_until_damage: f64,
}

impl Default for ActiveRadiation {
    fn default() -> Self {
        Self {
            level: 0.0,
            toxin: 0.0,
            last_radiation_damage: 0,
            exposed_last_tick: false,
            seconds_until_toxin: 10.0,
            ambient_level: 0.0,
            seconds_until_check: default_check_timer(),
            seconds_until_damage: default_damage_timer(),
        }
    }
}

impl ActiveRadiation {
    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn toxin_level(&self) -> f32 {
        self.toxin
    }
    pub fn is_exposed(&self) -> bool {
        self.ambient_level > 0.0 || self.exposed_last_tick
    }

    /// Environmental ownership is recomputed on scene changes and reactor cleanup.
    pub fn reset_ambient(&mut self) {
        self.exposed_last_tick = false;
        self.ambient_level = 0.0;
    }

    /// Contact reactions replace weaker contamination; they are not ambient sources.
    pub fn expose(&mut self, toxin: bool, amount: f32) {
        if !amount.is_finite() || amount <= 0.0 {
            return;
        }
        let level = if toxin {
            &mut self.toxin
        } else {
            &mut self.level
        };
        *level = level.max(amount);
    }

    pub fn clear_toxin(&mut self, amount: f32) -> bool {
        if !amount.is_finite() || amount <= 0.0 || self.toxin <= 0.0 {
            return false;
        }
        self.toxin = (self.toxin - amount).max(0.0);
        true
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
    pub fn advance(
        &mut self,
        elapsed_secs: f32,
        absorb_per_check: f32,
        recovery_per_tick: f32,
    ) -> i32 {
        self.advance_resisted(elapsed_secs, absorb_per_check, recovery_per_tick, 1.0, 1.0)
    }

    fn advance_resisted(
        &mut self,
        elapsed_secs: f32,
        absorb_per_check: f32,
        recovery_per_tick: f32,
        resistance: f32,
        toxin_resistance: f32,
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
            || self.seconds_until_check <= 0.0
            || self.seconds_until_check > default_check_timer()
        {
            self.seconds_until_check = default_check_timer();
        }
        if !self.seconds_until_damage.is_finite()
            || self.seconds_until_damage <= 0.0
            || self.seconds_until_damage > default_damage_timer()
        {
            self.seconds_until_damage = default_damage_timer();
        }
        let absorb = if absorb_per_check.is_finite() && absorb_per_check >= 0.0 {
            absorb_per_check
        } else {
            DEFAULT_RADIATION_ABSORB
        };
        let recovery = if recovery_per_tick.is_finite() && recovery_per_tick >= 0.0 {
            recovery_per_tick
        } else {
            DEFAULT_RADIATION_RECOVERY
        };

        // Advance in chronological order: a large step must not apply all
        // absorption before the first damage pulse (or differ from 60 Hz).
        self.last_radiation_damage = 0;
        let mut remaining = f64::from(elapsed_secs);
        let mut damage = 0_i32;
        while remaining > 0.0 {
            let step = remaining
                .min(self.seconds_until_check.max(0.0))
                .min(self.seconds_until_damage.max(0.0));
            self.seconds_until_check -= step;
            self.seconds_until_damage -= step;
            remaining -= step;
            if self.seconds_until_check <= 0.000001 {
                if self.ambient_level > self.level {
                    self.level += absorb.min(self.ambient_level - self.level);
                }
                self.seconds_until_check = default_check_timer();
            }
            if self.seconds_until_damage <= 0.000001 {
                if self.level >= 1.0 {
                    let pulse =
                        (self.level * RADIATION_DAMAGE_PER_LEVEL * resistance).trunc() as i32;
                    damage = damage.saturating_add(pulse);
                    self.last_radiation_damage = self.last_radiation_damage.saturating_add(pulse);
                }
                if self.ambient_level <= 0.0 {
                    self.level = (self.level - recovery).max(0.0);
                    if self.level < 1.0 {
                        self.level = 0.0;
                    }
                }
                self.seconds_until_damage = default_damage_timer();
            }
        }
        if !self.toxin.is_finite() || self.toxin < 0.0 {
            self.toxin = 0.0;
        }
        if !self.seconds_until_toxin.is_finite()
            || self.seconds_until_toxin <= 0.0
            || self.seconds_until_toxin > 10.0
        {
            self.seconds_until_toxin = 10.0;
        }
        self.seconds_until_toxin -= f64::from(elapsed_secs);
        while self.seconds_until_toxin <= 0.000001 {
            if self.toxin > 0.0 && toxin_resistance > 0.0 {
                damage =
                    damage.saturating_add((self.toxin * toxin_resistance).max(1.0).trunc() as i32);
            }
            self.seconds_until_toxin += 10.0;
        }
        self.exposed_last_tick = self.ambient_level > 0.0;
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

/// Membership is rebuilt by room sensors, not serialized as stale entity IDs.
#[derive(Unique, Default)]
pub struct RadiationRooms(pub std::collections::HashMap<EntityId, (f32, f32)>);

#[derive(Unique)]
pub struct HazardResistance(pub [f32; 8]);

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
    if world
        .borrow::<View<dark::properties::PropHitPoints>>()
        .ok()
        .and_then(|hp| hp.get(player).ok().map(|v| v.hit_points <= 0))
        .unwrap_or(false)
    {
        return None;
    }
    let (endurance, metabolism) = world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|q| {
            (
                q.player_stats().endurance.clamp(1, 8) as usize,
                q.player_stats()
                    .has_os_trait(crate::scripts::gui::TRAIT_STRONG_METABOLISM),
            )
        })
        .unwrap_or((1, false));
    let resistance = world
        .borrow::<UniqueView<HazardResistance>>()
        .map(|r| r.0[endurance - 1])
        .unwrap_or(1.0);
    let room = world
        .borrow::<UniqueView<RadiationRooms>>()
        .ok()
        .and_then(|rooms| {
            rooms
                .0
                .values()
                .copied()
                .max_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)))
        });
    let mut absorb = absorb;
    if let Some((ambient, rate)) = room {
        if let Ok(mut radiation) = world.borrow::<UniqueViewMut<ActiveRadiation>>() {
            radiation.observe_ambient(ambient);
        }
        absorb = rate;
    }
    let wormheart = super::script_util::player_carried_items(world)
        .into_iter()
        .any(|id| {
            world
                .borrow::<View<crate::runtime_props::RuntimePropHazardEquipment>>()
                .is_ok_and(|v| v.get(id).is_ok())
                && world
                    .borrow::<View<dark::properties::PropImplantDesc>>()
                    .is_ok_and(|v| v.get(id).is_ok_and(|v| v.0 == 12))
        });
    let protection = hazard_protection(world);
    // Retail armor slows accumulation; it does not lower the room ceiling.
    absorb *= 1.0 - protection.radiation / 100.0;
    let psi_armor = psi_hazard_protection(world);
    if psi_armor.radiation > 0.0 {
        // RadShield authors a divisor (retail 5), unlike percentage suit armor.
        absorb /= psi_armor.radiation;
    }
    let damage = world
        .borrow::<shipyard::UniqueViewMut<ActiveRadiation>>()
        .ok()
        .map(|mut radiation| {
            radiation.advance_resisted(
                elapsed_secs,
                absorb,
                recovery,
                resistance * if metabolism { 0.75 } else { 1.0 },
                if wormheart {
                    0.0
                } else {
                    resistance * if metabolism { 0.5 } else { 1.0 }
                },
            )
        })
        .unwrap_or(0);
    if damage <= 0 {
        return None;
    }
    let rad_damage = world
        .borrow::<UniqueView<ActiveRadiation>>()
        .map(|s| s.last_radiation_damage)
        .unwrap_or(0);
    let mut effects = vec![Effect::AdjustHitPoints {
        entity_id: player,
        delta: -damage,
    }];
    if rad_damage > 0 {
        effects.push(Effect::GlobalEffect(
            super::GlobalEffect::PlayerRadiationHit {
                damage: rad_damage as f32,
            },
        ));
        effects.push(Effect::PlaySound {
            handle: engine::audio::AudioHandle::new(),
            name: "raddmg".to_owned(),
            source: Some(player),
            spatial: false,
        });
    }
    Some(Effect::combine(effects))
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
        assert_eq!(radiation.advance(0.1, 8.0, 3.0), 0);
        assert!(radiation.clear(6.0));
        assert_eq!(radiation.level(), 2.0);

        assert!(radiation.observe_ambient(6.0));
        assert_eq!(radiation.advance(0.1, 4.0, 3.0), 0);
        assert_eq!(radiation.level(), 6.0);
    }

    #[test]
    fn damage_and_recovery_follow_the_retail_six_second_clock() {
        let mut radiation = ActiveRadiation::default();
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.1, 8.0, 3.0), 0);
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(5.89, 0.05, 3.0), 0);
        assert!(radiation.observe_ambient(8.0));
        assert_eq!(radiation.advance(0.02, 0.05, 3.0), 8);
        assert_eq!(radiation.level(), 8.0, "active exposure delays recovery");
        assert_eq!(radiation.advance(6.0, 0.05, 3.0), 8);
        assert_eq!(radiation.level(), 5.0);
    }

    #[test]
    fn corrupt_restored_timers_are_normalized_without_looping() {
        let mut radiation: ActiveRadiation = serde_json::from_value(json!({
            "level": 8.0,
            "seconds_until_check": -1.0e30,
            "seconds_until_damage": -1.0e30
        }))
        .unwrap();

        assert_eq!(radiation.advance(1.0 / 60.0, 0.05, 3.0), 0);
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
                .advance(0.1, 0.05, 3.0),
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

/// Protection is evaluated at use time, so removing armor or expiring psi
/// immediately changes subsequent exposure without curing existing status.
pub fn hazard_protection(world: &World) -> dark::properties::PropArmor {
    let mut result = dark::properties::PropArmor::default();
    let carried = super::script_util::player_carried_items(world);
    if let Ok(armor) = world.borrow::<View<dark::properties::PropArmor>>() {
        if let Ok(equipped) =
            world.borrow::<View<crate::runtime_props::RuntimePropHazardEquipment>>()
        {
            for entity in carried {
                if equipped.get(entity).is_ok() {
                    if let Ok(value) = armor.get(entity) {
                        result = *value;
                        break;
                    }
                }
            }
        }
    }
    result.radiation = result.radiation.clamp(0.0, 100.0);
    result.toxic = result.toxic.clamp(0.0, 100.0);
    result
}

fn psi_hazard_protection(world: &World) -> dark::properties::PropArmor {
    let mut armor = dark::properties::PropArmor::default();
    let Ok(active) = world.borrow::<UniqueView<crate::psi::ActivePsiPowers>>() else {
        return armor;
    };
    let Ok(registry) = world.borrow::<UniqueView<crate::psi::GlobalPsiPowers>>() else {
        return armor;
    };
    let psi = world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|q| q.player_stats().psionic_ability)
        .unwrap_or(1);
    for power in registry
        .0
        .iter()
        .filter(|p| active.is_active(p.template_id))
    {
        let value = power.power.data[0] + power.power.data[1] * (psi - 1).max(0) as f32;
        match power.template_id {
            RAD_SHIELD_TEMPLATE_ID => armor.radiation = value,
            TOXIN_SHIELD_TEMPLATE_ID => armor.toxic = value,
            _ => {}
        }
    }
    armor
}

// shkreact.cpp applies player armor as a percentage here, including RadShield's
// value 5. The room RadCheck division above is a distinct retail script path.
pub fn protected_exposure(world: &World, toxin: bool, amount: f32) -> f32 {
    let armor = hazard_protection(world);
    let psi = psi_hazard_protection(world);
    let percentages = if toxin {
        [armor.toxic, psi.toxic]
    } else {
        [armor.radiation, psi.radiation]
    };
    percentages.into_iter().fold(amount, |value, pct| {
        value * (1.0 - pct.clamp(0.0, 100.0) / 100.0)
    })
}

#[cfg(test)]
mod hazard_regressions {
    use super::*;

    #[test]
    fn overlapping_rooms_choose_strongest_rate_and_exit_stops_exposure() {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        let slow_room = world.add_entity(());
        let fast_room = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(ActiveRadiation::default());
        world.add_unique(RadiationRooms(std::collections::HashMap::from([
            (slow_room, (35.0, 0.05)),
            (fast_room, (35.0, 0.1)),
        ])));
        assert!(tick_player_radiation(&world, 1.0).is_none());
        let level = world
            .borrow::<UniqueView<ActiveRadiation>>()
            .unwrap()
            .level();
        assert!((level - 1.0).abs() < 0.0001);
        assert!(
            world
                .borrow::<UniqueView<ActiveRadiation>>()
                .unwrap()
                .is_exposed()
        );
        world
            .borrow::<UniqueViewMut<RadiationRooms>>()
            .unwrap()
            .0
            .clear();
        assert!(tick_player_radiation(&world, 1.0).is_none());
        let status = world.borrow::<UniqueView<ActiveRadiation>>().unwrap();
        assert_eq!(status.level(), level);
        assert!(!status.is_exposed());
    }

    #[test]
    fn toxin_persists_and_weaker_attacks_do_not_stack() {
        let mut status = ActiveRadiation::default();
        status.expose(true, 4.0);
        status.expose(true, 2.0);
        assert_eq!(status.toxin_level(), 4.0);
        assert_eq!(status.advance(9.0, 0.05, 3.0), 0);
        assert_eq!(status.advance(1.0, 0.05, 3.0), 4);
        assert_eq!(status.advance(10.0, 0.05, 3.0), 4);
        assert_eq!(status.toxin_level(), 4.0);
        assert!(status.clear_toxin(2.0));
        assert_eq!(status.toxin_level(), 2.0);
        assert!(status.clear_toxin(2.4));
        assert_eq!(status.toxin_level(), 0.0);
        assert!(!status.clear_toxin(2.0));
    }

    #[test]
    fn toxin_minimum_damage_does_not_turn_endurance_into_immunity() {
        let mut status = ActiveRadiation::default();
        status.expose(true, 1.0);
        assert_eq!(status.advance_resisted(10.0, 0.05, 3.0, 0.01, 0.01), 1);
        // WormHeart suppresses damage while equipped, without removing poison.
        assert_eq!(status.advance_resisted(10.0, 0.05, 3.0, 0.01, 0.0), 0);
        assert_eq!(status.toxin_level(), 1.0);
    }

    #[test]
    fn radiation_checks_and_damage_are_independent_of_step_partition() {
        let mut large = ActiveRadiation::default();
        large.observe_ambient(35.0);
        let large_damage = large.advance(30.0, 0.05, 3.0);
        let mut small = ActiveRadiation::default();
        let mut small_damage = 0;
        for _ in 0..1800 {
            small.observe_ambient(35.0);
            small_damage += small.advance(1.0 / 60.0, 0.05, 3.0);
        }
        assert_eq!(large_damage, small_damage);
        assert!((large.level() - small.level()).abs() < 0.001);
    }

    #[test]
    fn pause_and_save_restore_preserve_tick_phase() {
        let mut status = ActiveRadiation::default();
        status.expose(true, 3.0);
        status.advance(9.0, 0.05, 3.0);
        let saved = serde_json::to_string(&status).unwrap();
        assert_eq!(status.advance(0.0, 0.05, 3.0), 0);
        assert_eq!(serde_json::to_string(&status).unwrap(), saved);
        let mut restored: ActiveRadiation = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.advance(1.0, 0.05, 3.0), 3);
        assert_eq!(restored.toxin_level(), 3.0);
    }

    #[test]
    fn full_protection_and_zero_recovery_are_respected() {
        let mut status = ActiveRadiation::default();
        status.observe_ambient(35.0);
        assert_eq!(status.advance(6.0, 0.0, 0.0), 0);
        assert_eq!(status.level(), 0.0);
        status.expose(false, 12.0);
        assert_eq!(status.advance_resisted(6.0, 0.0, 0.0, 0.5, 1.0), 6);
        assert_eq!(status.level(), 12.0);
    }
}
