//! Campaign ownership of the adventure; commands are applied centrally with score and reward updates.
use crate::{
    mini_game_scores::MiniGameScore,
    overworld::{Action, Outcome, OverWorld, Phase},
    quest_info::QuestInfo,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const VICTORY_NANITES: i32 = 20;

#[derive(Clone, Debug)]
pub enum Command {
    Start,
    Restart,
    Act(Action),
    Advance { frame: Duration, elapsed: Duration },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Campaign {
    pub game: Option<OverWorld>,
    pub score_rank: Option<usize>,
    // Two held devices can display the same adventure; simulation time advances once per frame.
    #[serde(skip)]
    last_frame: Option<Duration>,
}
impl Campaign {
    fn update(&mut self, command: Command, hacked: bool, rng: &mut impl Rng) -> Option<Outcome> {
        match command {
            Command::Start if self.game.is_none() => self.game = Some(OverWorld::new(hacked, rng)),
            Command::Restart
                if self
                    .game
                    .as_ref()
                    .is_some_and(|game| game.phase != Phase::Playing) =>
            {
                self.game = Some(OverWorld::new(hacked, rng));
                self.score_rank = None;
                self.last_frame = None;
            }
            Command::Act(action) => {
                return self
                    .game
                    .as_mut()
                    .and_then(|game| game.act(action, hacked, rng));
            }
            Command::Advance { frame, elapsed } if self.last_frame != Some(frame) => {
                self.last_frame = Some(frame);
                return self
                    .game
                    .as_mut()
                    .and_then(|game| game.advance(elapsed.as_millis() as u64, rng));
            }
            _ => (),
        }
        None
    }
}

pub(crate) fn apply(
    quest: &mut QuestInfo,
    command: Command,
    hacked: bool,
    formats: [&str; 2],
    rng: &mut impl Rng,
) {
    if quest.mini_games & (1 << 4) == 0 {
        return;
    }
    if let Some(outcome) = quest.overworld.update(command, hacked, rng) {
        let game = quest.overworld.game.as_ref().unwrap();
        let turns = game.player.turns.min(i32::MAX as u32) as i32;
        let level = i32::from(game.player.level);
        let (format, args) = if outcome.won {
            (formats[1], [turns, level])
        } else {
            (formats[0], [level, turns])
        };
        let text = crate::scripts::gui::PanelText::format(format, &args);
        let scores = quest.mini_game_scores.entry(4).or_default();
        quest.overworld.score_rank = scores.rank(outcome.score);
        scores.record(MiniGameScore {
            points: outcome.score,
            text,
        });
        // Persist the terminal state, score, and payment in one effect application.
        if outcome.won {
            quest.player_stats_mut().award_nanites(VICTORY_NANITES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::{Kind, Position};
    use rand::{SeedableRng, rngs::StdRng};

    fn send(q: &mut QuestInfo, c: Command, r: &mut StdRng) {
        apply(q, c, true, ["level %d, %d turns", "won turn %d L%d"], r);
    }

    #[test]
    fn locked_game_does_not_start_and_opening_resumes_the_same_adventure() {
        let mut q = QuestInfo::new();
        let mut rng = StdRng::seed_from_u64(5);
        send(&mut q, Command::Start, &mut rng);
        assert!(q.overworld.game.is_none());
        q.mini_games = 1 << 4;
        send(&mut q, Command::Start, &mut rng);
        let before = q.overworld.game.clone();
        send(&mut q, Command::Start, &mut rng);
        send(&mut q, Command::Restart, &mut rng);
        assert_eq!(q.overworld.game, before);
        let mut restored: QuestInfo =
            serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        send(&mut restored, Command::Start, &mut rng);
        assert_eq!(restored.overworld.game, before);
    }

    #[test]
    fn duplicate_frames_do_not_advance_projectiles_twice() {
        let mut q = QuestInfo::new();
        let mut rng = StdRng::seed_from_u64(5);
        q.mini_games = 1 << 4;
        send(&mut q, Command::Start, &mut rng);
        send(&mut q, Command::Act(Action::Missile(1, 0)), &mut rng);
        let command = Command::Advance {
            frame: Duration::from_secs(1),
            elapsed: Duration::from_millis(100),
        };
        send(&mut q, command.clone(), &mut rng);
        let once = q.overworld.game.clone();
        send(&mut q, command, &mut rng);
        assert_eq!(q.overworld.game, once);
    }

    #[test]
    fn victory_pays_twenty_once_and_saving_reopening_or_restart_cannot_repay() {
        let mut q = QuestInfo::new();
        let mut rng = StdRng::seed_from_u64(7);
        q.mini_games = 1 << 4;
        send(&mut q, Command::Start, &mut rng);
        // A serialized late-game fixture exercises the same final attack used by the interface.
        let mut value = serde_json::to_value(q.overworld.game.as_ref().unwrap()).unwrap();
        value["actors"] = serde_json::json!(vec![Option::<u8>::None; 128]);
        value["actors"][1] = serde_json::json!({"kind":Kind::Dragon,"position":Position{x:11,y:10},"hp":1,"behavior":"Guard","home":null,"element":"Road"});
        value["player"]["position"] = serde_json::json!(Position { x: 10, y: 10 });
        value["player"]["dragons"] = 4.into();
        value["player"]["hit"] = serde_json::json!([0, 0, 100]);
        value["player"]["damage"] = serde_json::json!([0, 0, 100]);
        q.overworld.game = Some(serde_json::from_value(value).unwrap());
        let before = q.player_stats().nanites;
        send(&mut q, Command::Act(Action::Move(1, 0)), &mut rng);
        assert_eq!(q.player_stats().nanites, before + 20);
        assert_eq!(q.overworld.game.as_ref().unwrap().phase, Phase::Won);
        let mut q: QuestInfo = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        send(&mut q, Command::Start, &mut rng);
        send(&mut q, Command::Act(Action::Move(1, 0)), &mut rng);
        send(
            &mut q,
            Command::Advance {
                frame: Duration::from_secs(5),
                elapsed: Duration::from_secs(5),
            },
            &mut rng,
        );
        assert_eq!(q.player_stats().nanites, before + 20);
        assert_eq!(q.mini_game_scores[&4].entries().len(), 1);
        send(&mut q, Command::Restart, &mut rng);
        assert_eq!(q.player_stats().nanites, before + 20);
        assert_eq!(q.overworld.game.as_ref().unwrap().phase, Phase::Playing);
    }
}
