use serde::{Deserialize, Serialize};
use shipyard::{Unique, UniqueView, World};

use crate::{physics::PhysicsWorld, quest_info::QuestInfo};

use super::{Effect, MessagePayload, Script};
use crate::scripts::gui::TRAIT_PHARMO_FRIENDLY;

/// Retail `MedPatchScript` / `MedKitScript` cadence recovered from the shipped
/// `allobjs.osm`: `DoHeal` first runs after 0.1s, then reschedules every 1.5s
/// until the item's healing budget has been spent.
pub const HEALING_FIRST_PULSE_SECS: f32 = 0.1;
pub const HEALING_PULSE_INTERVAL_SECS: f32 = 1.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealingItemKind {
    MedPatch,
    MedicalKit,
}

impl HealingItemKind {
    fn base_total(self) -> i32 {
        match self {
            Self::MedPatch => 10,
            Self::MedicalKit => 200,
        }
    }

    fn base_pulse(self) -> i32 {
        match self {
            Self::MedPatch => 2,
            Self::MedicalKit => 5,
        }
    }
}

/// The retail healing-item scripts share one implementation and override only
/// their healing budget and pulse size.
pub struct HealingItemScript {
    kind: HealingItemKind,
}

impl HealingItemScript {
    pub fn new(kind: HealingItemKind) -> Self {
        Self { kind }
    }

    fn pharmo_friendly(world: &World) -> bool {
        world
            .borrow::<UniqueView<QuestInfo>>()
            .is_ok_and(|quests| quests.player_stats().has_os_trait(TRAIT_PHARMO_FRIENDLY))
    }

    /// The original multiplies both values by 1.2 and converts back to an
    /// integer. Integer arithmetic preserves that truncation exactly.
    fn retail_amount(base: i32, pharmo_friendly: bool) -> i32 {
        if pharmo_friendly {
            base.saturating_mul(6) / 5
        } else {
            base
        }
    }
}

impl Script for HealingItemScript {
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

        let pharmo_friendly = Self::pharmo_friendly(world);
        let mut total = Self::retail_amount(self.kind.base_total(), pharmo_friendly);
        // MedPatchScript boosts the total budget on Easy, after Pharmo-Friendly.
        // Its pulse size/cadence and MedicalKit's budget are unchanged.
        if self.kind == HealingItemKind::MedPatch
            && world
                .borrow::<UniqueView<QuestInfo>>()
                .is_ok_and(|q| q.difficulty() == dark::gamesys::Difficulty::Easy)
        {
            total = total * 3 / 2;
        }
        Effect::UseHealingItem {
            entity_id,
            total,
            pulse: Self::retail_amount(self.kind.base_pulse(), pharmo_friendly),
            first_pulse_secs: HEALING_FIRST_PULSE_SECS,
            pulse_interval_secs: HEALING_PULSE_INTERVAL_SECS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct HealingDose {
    remaining: i32,
    pulse: i32,
    seconds_until_pulse: f32,
    pulse_interval_secs: f32,
}

/// Timed healing already applied to the player. This intentionally lives in
/// global save data rather than the source item's script namespace: retail
/// consumes the source as soon as it starts the timer course, so no item
/// remains to own the continuing state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Unique)]
pub struct ActiveHealing {
    doses: Vec<HealingDose>,
}

impl ActiveHealing {
    pub fn queue(
        &mut self,
        total: i32,
        pulse: i32,
        first_pulse_secs: f32,
        pulse_interval_secs: f32,
    ) -> bool {
        if total <= 0
            || pulse <= 0
            || !first_pulse_secs.is_finite()
            || first_pulse_secs < 0.0
            || !pulse_interval_secs.is_finite()
            || pulse_interval_secs <= 0.0
        {
            return false;
        }
        self.doses.push(HealingDose {
            remaining: total,
            pulse,
            seconds_until_pulse: first_pulse_secs,
            pulse_interval_secs,
        });
        true
    }

    fn prune_invalid(&mut self) {
        self.doses.retain(|dose| {
            dose.remaining > 0
                && dose.pulse > 0
                && dose.seconds_until_pulse.is_finite()
                && dose.pulse_interval_secs.is_finite()
                && dose.pulse_interval_secs > 0.0
        });
    }

    /// Advance every active course and return the HP delta to send through the
    /// mission's central `AdjustHitPoints` handler. Pulse budgets are spent at
    /// the maximum too, so later damage cannot resurrect healing that retail
    /// already capped away.
    pub fn advance(&mut self, elapsed_secs: f32, current_hp: i32, maximum_hp: i32) -> i32 {
        // Saves are user-editable and older experimental builds wrote this
        // global state. Reject zero pulses/intervals and non-finite timers
        // before entering the pulse loop so corrupt data cannot hang a load.
        self.prune_invalid();
        if !elapsed_secs.is_finite() || elapsed_secs <= 0.0 {
            return 0;
        }

        let maximum_hp = maximum_hp.max(0);
        let mut live_hp = current_hp.min(maximum_hp);
        for dose in &mut self.doses {
            dose.seconds_until_pulse -= elapsed_secs;
            while dose.remaining > 0 && dose.seconds_until_pulse <= 0.0 {
                let budget = dose.pulse.min(dose.remaining);
                let missing = (maximum_hp - live_hp).max(0);
                live_hp += budget.min(missing);
                dose.remaining -= budget;
                dose.seconds_until_pulse += dose.pulse_interval_secs;
            }
        }
        self.doses.retain(|dose| dose.remaining > 0);
        live_hp - current_hp
    }
}

/// Advance the live player's retail course and produce one ordinary health
/// effect. `MissionCore` remains the only place that mutates HP, preserving its
/// player-death guard and the shared HP trace.
pub fn tick_player_healing(world: &World, elapsed_secs: f32) -> Option<Effect> {
    let (player, current, maximum) = crate::scripts::script_util::player_hit_points(world)?;
    let delta = world
        .borrow::<shipyard::UniqueViewMut<ActiveHealing>>()
        .ok()
        .map(|mut active| active.advance(elapsed_secs, current, maximum))
        .unwrap_or(0);
    (delta > 0).then_some(Effect::AdjustHitPoints {
        entity_id: player,
        delta,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        quest_info::QuestInfo,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::{
        ActiveHealing, HEALING_FIRST_PULSE_SECS, HEALING_PULSE_INTERVAL_SECS, HealingItemKind,
        HealingItemScript,
    };

    #[test]
    fn easy_medpatch_budget_applies_after_trait_without_changing_pulses() {
        for difficulty in dark::gamesys::Difficulty::ALL {
            for pharmo in [false, true] {
                let mut world = World::new();
                let item = world.add_entity(());
                let mut quests = QuestInfo::with_difficulty(difficulty);
                if pharmo {
                    quests
                        .player_stats_mut()
                        .add_os_trait(crate::scripts::gui::TRAIT_PHARMO_FRIENDLY);
                }
                world.add_unique(quests);
                for kind in [HealingItemKind::MedPatch, HealingItemKind::MedicalKit] {
                    let effect = HealingItemScript::new(kind).handle_message(
                        item,
                        &world,
                        &PhysicsWorld::new(),
                        &MessagePayload::Frob,
                    );
                    let mut expected = if kind == HealingItemKind::MedPatch {
                        10
                    } else {
                        200
                    };
                    if pharmo {
                        expected = expected * 6 / 5;
                    }
                    if kind == HealingItemKind::MedPatch
                        && difficulty == dark::gamesys::Difficulty::Easy
                    {
                        expected = expected * 3 / 2;
                    }
                    match effect {
                        Effect::UseHealingItem {
                            total,
                            pulse,
                            first_pulse_secs,
                            pulse_interval_secs,
                            ..
                        } => {
                            assert_eq!(total, expected);
                            assert_eq!(
                                pulse,
                                if kind == HealingItemKind::MedPatch {
                                    2
                                } else if pharmo {
                                    6
                                } else {
                                    5
                                }
                            );
                            assert_eq!(first_pulse_secs, 0.1);
                            assert_eq!(pulse_interval_secs, 1.5);
                        }
                        _ => panic!("expected healing use"),
                    }
                }
            }
        }
    }

    #[test]
    fn med_patch_frob_uses_retail_budget_and_cadence() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let patch = world.add_entity(());
        let effect = HealingItemScript::new(HealingItemKind::MedPatch).handle_message(
            patch,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(
            effect,
            Effect::UseHealingItem {
                entity_id,
                total: 10,
                pulse: 2,
                first_pulse_secs: HEALING_FIRST_PULSE_SECS,
                pulse_interval_secs: HEALING_PULSE_INTERVAL_SECS,
            } if entity_id == patch
        ));
    }

    #[test]
    fn medical_kit_uses_retail_override() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let kit = world.add_entity(());
        let effect = HealingItemScript::new(HealingItemKind::MedicalKit).handle_message(
            kit,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(
            effect,
            Effect::UseHealingItem {
                entity_id,
                total: 200,
                pulse: 5,
                ..
            } if entity_id == kit
        ));
    }

    #[test]
    fn pharmo_friendly_uses_the_retail_twenty_percent_integer_bonus() {
        let mut world = World::new();
        let mut quests = QuestInfo::new();
        assert!(quests.player_stats_mut().add_os_trait(2));
        world.add_unique(quests);
        let kit = world.add_entity(());
        let effect = HealingItemScript::new(HealingItemKind::MedicalKit).handle_message(
            kit,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(
            effect,
            Effect::UseHealingItem {
                total: 240,
                pulse: 6,
                ..
            }
        ));

        let patch = world.add_entity(());
        let effect = HealingItemScript::new(HealingItemKind::MedPatch).handle_message(
            patch,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(
            effect,
            Effect::UseHealingItem {
                total: 12,
                // 2 * 1.2 truncates back to 2 in the original.
                pulse: 2,
                ..
            }
        ));
    }

    #[test]
    fn active_healing_obeys_initial_delay_interval_total_and_cap() {
        let mut active = ActiveHealing::default();
        assert!(active.queue(10, 2, 0.1, 1.5));
        assert_eq!(active.advance(0.099, 8, 30), 0);
        assert_eq!(active.advance(0.002, 8, 30), 2);
        assert_eq!(active.advance(6.0, 10, 30), 8);

        assert!(active.queue(200, 5, 0.1, 1.5));
        assert_eq!(active.advance(8.0, 1, 30), 29);
    }

    #[test]
    fn queue_rejects_non_finite_timing() {
        let mut active = ActiveHealing::default();
        assert!(!active.queue(10, 2, f32::NAN, 1.5));
        assert!(!active.queue(10, 2, 0.1, f32::INFINITY));
        assert_eq!(active, ActiveHealing::default());
    }

    #[test]
    fn corrupt_restored_doses_are_pruned_without_looping() {
        let mut active: ActiveHealing = serde_json::from_value(json!({
            "doses": [
                { "remaining": 10, "pulse": 0, "seconds_until_pulse": -1.0, "pulse_interval_secs": 0.0 },
                { "remaining": 10, "pulse": 2, "seconds_until_pulse": 0.0, "pulse_interval_secs": 0.0 }
            ]
        }))
        .unwrap();
        assert_eq!(active.advance(60.0, 8, 30), 0);
        assert_eq!(active, ActiveHealing::default());
    }
}
