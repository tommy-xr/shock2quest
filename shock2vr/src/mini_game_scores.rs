//! Campaign score tables shared by the handheld games.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MiniGameScore {
    pub points: i32,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct MiniGameScores(Vec<MiniGameScore>);

impl MiniGameScores {
    /// A new tie ranks ahead of older entries; nonpositive empty slots are zero.
    pub fn rank(&self, points: i32) -> Option<usize> {
        let rank = self.0.partition_point(|entry| entry.points > points);
        (points >= 0 && rank < 10).then_some(rank)
    }

    pub fn record(&mut self, score: MiniGameScore) {
        if let Some(rank) = self.rank(score.points) {
            self.0.insert(rank, score);
            self.0.truncate(10);
        }
    }

    pub fn entries(&self) -> &[MiniGameScore] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;

    #[test]
    fn top_ten_orders_new_ties_first_and_rejects_lower_scores() {
        let mut scores = MiniGameScores::default();
        for points in 1..=11 {
            scores.record(MiniGameScore {
                points,
                text: points.to_string(),
            });
        }
        assert_eq!(
            scores
                .entries()
                .iter()
                .map(|e| e.points)
                .collect::<Vec<_>>(),
            (2..=11).rev().collect::<Vec<_>>()
        );
        assert_eq!(scores.rank(1), None);
        scores.record(MiniGameScore {
            points: 11,
            text: "new tie".into(),
        });
        assert_eq!(scores.entries()[0].text, "new tie");
        assert_eq!(scores.entries().len(), 10);
        assert_eq!(MiniGameScores::default().rank(-1), None);
        assert_eq!(MiniGameScores::default().rank(0), Some(0));
    }

    #[test]
    fn separate_game_tables_round_trip_with_campaign_state() {
        let mut quest = QuestInfo::new();
        quest
            .mini_game_scores
            .entry(5)
            .or_default()
            .record(MiniGameScore {
                points: 42,
                text: "42 Bacon Caught".into(),
            });
        quest
            .mini_game_scores
            .entry(10)
            .or_default()
            .record(MiniGameScore {
                points: 100,
                text: "100".into(),
            });
        let restored: QuestInfo =
            serde_json::from_str(&serde_json::to_string(&quest).unwrap()).unwrap();
        assert_eq!(restored.mini_game_scores, quest.mini_game_scores);
    }
}
