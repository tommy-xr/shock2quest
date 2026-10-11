//! Retail world slot machines (not the PDA's separate slot minigame).
//!
//! allobjs.osm's SlotMachine::OnFrobWorldEnd at 0x1000d210 pays 2 nanites,
//! compares one float roll against the doubles 0.001 and 0.98, then awards
//! 400 / 0 / 4 nanites. It emits a message and schema immediately; no reels,
//! cooldown, or hack modifier are authored by this script.

use engine::audio::AudioHandle;
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    quest_info::QuestInfo,
    scripts::{Effect, gui::PanelText, script_util::debit_player_nanites},
};

const STAKE: i32 = 2;

/// Called only by the mission's effect applier. Debit and award happen here,
/// before another queued use can observe the balance; feedback stays effects.
pub(crate) fn play(world: &World, machine: EntityId, roll: f32) -> Effect {
    let Some(exhausted) = debit_player_nanites(world, STAKE) else {
        return Effect::ShowMessage {
            text: PanelText::format(
                &PanelText::string(
                    world,
                    "misc",
                    "SlotCantPay",
                    "Slot machine costs %d nanites to play.",
                ),
                &[STAKE],
            ),
        };
    };

    // Preserve retail's float-to-double comparison at the threshold edges.
    let (schema, payout, message_amount, fallback) = if f64::from(roll) <= 0.001 {
        ("SlotJackpot", 400, 400, "Jackpot!  You won %d nanites!")
    } else if f64::from(roll) <= 0.98 {
        ("SlotLose", 0, STAKE, "You lost %d nanites!")
    } else {
        ("SlotWin", 4, 4, "You won %d nanites!")
    };
    world
        .borrow::<UniqueViewMut<QuestInfo>>()
        .unwrap()
        .player_stats_mut()
        .award_nanites(payout);

    let mut effects: Vec<_> = exhausted
        .into_iter()
        .map(|entity_id| Effect::DestroyEntity { entity_id })
        .collect();
    effects.push(Effect::ShowMessage {
        text: PanelText::format(
            &PanelText::string(world, "misc", schema, fallback),
            &[message_amount],
        ),
    });
    effects.push(Effect::PlaySound {
        handle: AudioHandle::new(),
        name: schema.into(),
        source: Some(machine),
        spatial: true,
    });
    Effect::combine(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::script_util::player_nanite_total;

    #[test]
    fn retail_roll_boundaries_pay_the_stake_then_the_award() {
        for (roll, payout, schema, text) in [
            (0.0, 400, "SlotJackpot", "Jackpot!  You won 400 nanites!"),
            (
                f32::from_bits(0.001_f32.to_bits() - 1),
                400,
                "SlotJackpot",
                "Jackpot!  You won 400 nanites!",
            ),
            (0.001, 0, "SlotLose", "You lost 2 nanites!"),
            (
                f32::from_bits(0.98_f32.to_bits() - 1),
                0,
                "SlotLose",
                "You lost 2 nanites!",
            ),
            (0.98, 4, "SlotWin", "You won 4 nanites!"),
            (0.999, 4, "SlotWin", "You won 4 nanites!"),
        ] {
            let mut world = World::new();
            let machine = world.add_entity(());
            let mut quests = QuestInfo::new();
            quests.player_stats_mut().award_nanites(2);
            world.add_unique(quests);
            let effects = Effect::flatten(vec![play(&world, machine, roll)]);
            assert_eq!(player_nanite_total(&world), payout, "roll {roll}");
            assert_eq!(effects.len(), 2);
            assert!(matches!(&effects[0], Effect::ShowMessage { text: actual } if actual == text));
            assert!(
                matches!(&effects[1], Effect::PlaySound { name, source: Some(id), spatial: true, .. } if name == schema && *id == machine)
            );
        }
    }

    #[test]
    fn insufficient_balance_cannot_win_and_queued_uses_cannot_overdraw() {
        let mut world = World::new();
        let machine = world.add_entity(());
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().award_nanites(3);
        world.add_unique(quests);
        play(&world, machine, 0.5);
        assert_eq!(player_nanite_total(&world), 1);
        for _ in 0..2 {
            assert!(
                matches!(play(&world, machine, 0.0), Effect::ShowMessage { text } if text == "Slot machine costs 2 nanites to play.")
            );
            assert_eq!(player_nanite_total(&world), 1);
        }
    }
}
