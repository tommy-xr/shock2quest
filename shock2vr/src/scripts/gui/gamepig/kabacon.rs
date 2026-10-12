//! Falling bacon, three paddles, and campaign high scores.
use super::{GamePigGuiMsg, sprite};
use crate::{
    gui::{self, GuiComponent, GuiCursor},
    mini_game_scores::MiniGameScore,
    scripts::{Effect, gui::PanelText},
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use rand::Rng;
use shipyard::World;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default)]
struct Bacon {
    x: f32,
    y: f32,
    speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Play,
    Reset,
    Scores,
}

#[derive(Clone, Debug)]
pub(super) struct KaBacon {
    paddle: f32,
    paddles: usize,
    caught: i32,
    bomber: f32,
    bomber_speed: f32,
    drop_speed: f32,
    drop_rate_ms: i32,
    drop_timer: Duration,
    dropped: u32,
    bacon: [Bacon; 32],
    mode: Mode,
    score_time: Duration,
    rank: Option<usize>,
    last_frame: Option<Duration>,
    pub stick: Option<f32>,
}

impl Default for KaBacon {
    fn default() -> Self {
        Self {
            paddle: 69.0,
            paddles: 3,
            caught: 0,
            bomber: 69.0,
            bomber_speed: 0.0,
            drop_speed: 50.0,
            drop_rate_ms: 800,
            drop_timer: Duration::ZERO,
            dropped: 0,
            bacon: [Bacon::default(); 32],
            mode: Mode::Play,
            score_time: Duration::ZERO,
            rank: None,
            last_frame: None,
            stick: None,
        }
    }
}

impl KaBacon {
    pub fn activate(&mut self, hacked: bool) {
        if self.mode == Mode::Play && hacked {
            self.paddles = (self.paddles + 1).min(3);
        } else if self.mode == Mode::Scores && self.score_time > Duration::from_secs(3) {
            let last_frame = self.last_frame;
            *self = Self::default();
            self.last_frame = last_frame;
        }
    }

    pub fn advance(
        &mut self,
        time: &Time,
        cursor: Option<&GuiCursor>,
        controller: bool,
        world: &World,
    ) -> Effect {
        if self.last_frame == Some(time.total) {
            return Effect::NoEffect;
        }
        self.last_frame = Some(time.total);
        let axis = self.stick.take().unwrap_or(0.0);
        let speed = if controller {
            axis.clamp(-1.0, 1.0) * 240.0
        } else {
            cursor
                .filter(|cursor| cursor.position.x >= 0.0)
                .map_or(0.0, |cursor| (cursor.position.x - 13.0 - self.paddle) * 5.5)
        };
        let previous = self.mode;
        self.tick(time.elapsed, speed, &mut rand::thread_rng());
        if previous != Mode::Scores && self.mode == Mode::Scores {
            self.rank = super::score_rank(world, 5, self.caught);
            let format = PanelText::string(world, "minigame", "kb_score_form", "%d Bacon Caught");
            return Effect::RecordMiniGameScore {
                game: 5,
                score: MiniGameScore {
                    points: self.caught,
                    text: PanelText::format(&format, &[self.caught]),
                },
            };
        }
        Effect::NoEffect
    }

    fn tick(&mut self, elapsed: Duration, paddle_speed: f32, rng: &mut impl Rng) {
        let dt = elapsed.as_secs_f32();
        if self.mode == Mode::Scores {
            self.score_time += elapsed;
        }
        if self.mode == Mode::Reset {
            self.bacon.fill(Bacon::default());
            self.drop_rate_ms += 32;
            self.drop_speed -= 8.0;
            self.drop_timer = Duration::ZERO;
            self.mode = Mode::Play;
        }
        for bacon in &mut self.bacon {
            if bacon.speed == 0.0 {
                continue;
            }
            let previous_y = bacon.y;
            bacon.y += bacon.speed * dt;
            let first_paddle = 200.0 + (3 - self.paddles) as f32 * 14.0;
            if bacon.y <= first_paddle {
                continue;
            }
            // Test the swept vertical interval before the loss boundary, so a
            // slow frame cannot jump over a paddle that the bacon crossed.
            if self.mode == Mode::Play
                && previous_y <= 236.0
                && (bacon.x - self.paddle).abs() < 12.0
            {
                bacon.speed = 0.0;
                self.caught += 1;
            } else if bacon.y > 236.0 {
                bacon.speed = 0.0;
                if self.mode == Mode::Play {
                    self.paddles -= 1;
                    self.mode = if self.paddles == 0 {
                        Mode::Scores
                    } else {
                        Mode::Reset
                    };
                }
            }
        }
        if self.mode != Mode::Play {
            return;
        }
        self.drop_timer += elapsed;
        while self.drop_timer > Duration::from_millis(self.drop_rate_ms as u64) {
            self.drop_timer -= Duration::from_millis(self.drop_rate_ms as u64);
            if let Some(bacon) = self.bacon.iter_mut().find(|bacon| bacon.speed == 0.0) {
                *bacon = Bacon {
                    x: self.bomber,
                    y: 56.0,
                    speed: self.drop_speed,
                };
                self.dropped += 1;
                let multiplier = if self.dropped < 30 {
                    2.0
                } else if self.dropped > 60 {
                    1.0
                } else {
                    2.0 - (self.dropped - 30) as f32 / 30.0
                };
                self.drop_speed += multiplier;
                if self.drop_rate_ms > 175 {
                    self.drop_rate_ms = (self.drop_rate_ms as f32 - 4.0 * multiplier) as i32;
                }
            }
        }
        if rng.gen_range(0..32) == 0 {
            match rng.gen_range(0..4) {
                0 => self.bomber_speed = -self.bomber_speed,
                1 => self.bomber_speed *= 3.0,
                2 => self.bomber_speed = 0.0,
                _ => self.bomber_speed /= 2.0,
            }
        }
        if rng.gen_range(0..16) != 0 {
            self.bomber_speed += (rng.gen_range(0.0..1.0) - 0.5) * self.drop_speed;
        }
        self.bomber += self.bomber_speed * dt;
        if self.bomber < 12.0 {
            self.bomber = 12.0;
            self.bomber_speed = 5.0;
        }
        if self.bomber > 127.0 {
            self.bomber = 127.0;
            self.bomber_speed = -5.0;
        }
        self.paddle = (self.paddle + paddle_speed * dt).clamp(12.0, 127.0);
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let bomber = sprite("kbbomber.pcx", self.bomber as i32, 32, 32.0, 32.0);
        let bomber_rect = bomber.rect();
        let mut components = vec![
            GuiComponent::Fill {
                position: vec2(13.0, 11.0),
                size: vec2(140.0, 276.0),
                color: [0, 0, 0],
                alpha: 1.0,
            },
            bomber,
        ];
        if self.mode != Mode::Scores {
            for index in 3 - self.paddles..3 {
                components.push(sprite(
                    &format!("kbpad_{index}.pcx"),
                    self.paddle as i32,
                    200 + index as i32 * 14,
                    24.0,
                    8.0,
                ));
            }
            components.push(
                gui::button(GamePigGuiMsg::BaconActivate)
                    .with_rect(bomber_rect)
                    .with_label("bacon-bomber")
                    .with_alpha(0.0),
            );
        }
        for bacon in self.bacon.iter().filter(|bacon| bacon.speed != 0.0) {
            components.push(sprite(
                "kbbacon.pcx",
                bacon.x as i32,
                bacon.y as i32,
                12.0,
                6.0,
            ));
        }
        let score = PanelText::string(world, "minigame", "kb_score", "Bacon Caught: %d");
        components.push(PanelText::text(
            &PanelText::format(&score, &[self.caught]),
            Rect::new(14.0, 12.0, 138.0, 15.0),
        ));
        if self.mode == Mode::Scores {
            components.extend(super::score_table(
                world,
                5,
                self.rank,
                Rect::new(30.0, 77.0, 122.0, 198.0),
            ));
            if self.score_time > Duration::from_secs(3) {
                let phase =
                    ((self.score_time - Duration::from_secs(3)).as_millis() / 8 % 440) as f32;
                let y = if phase > 220.0 { 440.0 - phase } else { phase };
                let text = PanelText::string(world, "minigame", "kb_gameover", "Game Over");
                components.push(PanelText::text(
                    &text,
                    Rect::new(60.0, 12.0 + y, 92.0, 15.0),
                ));
                components.push(
                    gui::button(GamePigGuiMsg::BaconActivate)
                        .with_position(vec2(13.0, 11.0))
                        .with_size(vec2(140.0, 276.0))
                        .with_label("bacon-restart")
                        .with_alpha(0.0),
                );
            }
        }
        components
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn long_frame_catches_bacon_crossing_the_last_paddle() {
        let mut game = KaBacon {
            paddles: 1,
            ..Default::default()
        };
        game.bacon[0] = Bacon {
            x: game.paddle,
            y: 227.0,
            speed: 50.0,
        };
        game.tick(
            Duration::from_millis(200),
            0.0,
            &mut StdRng::seed_from_u64(3),
        );
        assert_eq!(game.caught, 1);
        assert_eq!(game.paddles, 1);
        assert_eq!(game.mode, Mode::Play);
    }

    #[test]
    fn catches_at_live_paddles_but_edge_misses_remove_only_one_paddle() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut game = KaBacon::default();
        game.bacon[0] = Bacon {
            x: 69.0,
            y: 200.0,
            speed: 50.0,
        };
        game.tick(Duration::from_millis(16), 0.0, &mut rng);
        assert_eq!(game.caught, 1);
        assert_eq!(game.bacon[0].speed, 0.0);
        game.bacon[0] = Bacon {
            x: 81.0,
            y: 220.0,
            speed: 50.0,
        };
        game.tick(Duration::from_millis(16), 0.0, &mut rng);
        assert_eq!(
            game.caught, 1,
            "exact paddle edge is outside the catch interval"
        );
        game.bacon[0].y = 236.0;
        game.bacon[1] = game.bacon[0];
        game.tick(Duration::from_millis(16), 0.0, &mut rng);
        assert_eq!(
            game.paddles, 2,
            "simultaneous misses remove only one paddle"
        );
        assert_eq!(game.mode, Mode::Reset);
        game.tick(Duration::from_millis(16), 0.0, &mut rng);
        assert_eq!(game.mode, Mode::Play);
        assert!(game.bacon.iter().all(|bacon| bacon.speed == 0.0));
        assert_eq!(game.drop_rate_ms, 832);
        assert_eq!(game.drop_speed, 42.0);
        game.bacon[0] = Bacon {
            x: game.paddle,
            y: 201.0,
            speed: 50.0,
        };
        game.tick(Duration::from_millis(16), 0.0, &mut rng);
        assert_eq!(game.caught, 1, "removed top paddle cannot catch");
        game.bacon[0].y = 214.0;
        game.tick(Duration::from_millis(16), 0.0, &mut rng);
        assert_eq!(game.caught, 2);
    }

    #[test]
    fn drops_accelerate_and_full_pool_does_not_increase_difficulty() {
        let mut rng = StdRng::seed_from_u64(2);
        let mut game = KaBacon::default();
        game.tick(Duration::from_millis(800), 0.0, &mut rng);
        assert_eq!(game.dropped, 0, "strict drop interval boundary");
        game.tick(Duration::from_millis(1), 0.0, &mut rng);
        assert_eq!(game.dropped, 1);
        assert_eq!(game.bacon[0].speed, 50.0);
        assert_eq!(game.drop_speed, 52.0);
        assert_eq!(game.drop_rate_ms, 792);
        game.bacon.fill(Bacon {
            x: 1.0,
            y: 1.0,
            speed: 1.0,
        });
        game.tick(Duration::from_millis(800), 0.0, &mut rng);
        assert_eq!(game.dropped, 1);
        game.bacon[0].speed = 0.0;
        game.dropped = 59;
        game.tick(Duration::from_millis(800), 0.0, &mut rng);
        assert_eq!(
            game.drop_speed, 53.0,
            "difficulty ramp has eased to one at drop sixty"
        );
    }

    #[test]
    fn game_over_records_once_and_requires_delay_to_restart() {
        let world = World::new();
        world.add_unique(QuestInfo::new());
        let mut game = KaBacon {
            paddles: 1,
            caught: 7,
            ..Default::default()
        };
        game.bacon[0] = Bacon {
            x: 1.0,
            y: 236.0,
            speed: 50.0,
        };
        let frame = Time {
            total: Duration::from_secs(1),
            elapsed: Duration::from_millis(16),
        };
        let effect = game.advance(&frame, None, true, &world);
        assert!(matches!(
            effect,
            Effect::RecordMiniGameScore {
                game: 5,
                score: MiniGameScore { points: 7, .. }
            }
        ));
        assert_eq!(game.mode, Mode::Scores);
        assert_eq!(game.rank, Some(0));
        assert!(matches!(
            game.advance(&frame, None, true, &world),
            Effect::NoEffect
        ));
        game.activate(true);
        assert_eq!(game.mode, Mode::Scores);
        let frame = Time {
            total: Duration::from_secs(4),
            elapsed: Duration::from_secs(3),
        };
        assert!(matches!(
            game.advance(&frame, None, true, &world),
            Effect::NoEffect
        ));
        game.activate(true);
        assert_eq!(
            game.mode,
            Mode::Scores,
            "restart threshold is strictly greater than three seconds"
        );
        game.advance(
            &Time {
                total: Duration::from_millis(4001),
                elapsed: Duration::from_millis(1),
            },
            None,
            true,
            &world,
        );
        game.activate(false);
        assert_eq!(game.mode, Mode::Play);
        assert_eq!(game.paddles, 3);
        assert_eq!(game.caught, 0);
    }

    #[test]
    fn fresh_stick_samples_stop_and_hack_restores_only_during_play() {
        let world = World::new();
        world.add_unique(QuestInfo::new());
        let mut game = KaBacon {
            stick: Some(-1.0),
            ..Default::default()
        };
        game.advance(
            &Time {
                total: Duration::from_millis(100),
                elapsed: Duration::from_millis(100),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.paddle, 45.0);
        game.advance(
            &Time {
                total: Duration::from_millis(200),
                elapsed: Duration::from_millis(100),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.paddle, 45.0);
        game.paddles = 1;
        game.activate(false);
        assert_eq!(game.paddles, 1);
        game.activate(true);
        assert_eq!(game.paddles, 2);
        game.activate(true);
        game.activate(true);
        assert_eq!(game.paddles, 3);
        game.mode = Mode::Reset;
        game.paddles = 2;
        game.activate(true);
        assert_eq!(game.paddles, 2);
    }
}

#[cfg(test)]
mod score_tests {
    use super::*;
    use crate::quest_info::QuestInfo;

    #[test]
    fn unfilled_score_slots_do_not_emit_empty_text_meshes() {
        let world = World::new();
        world.add_unique(QuestInfo::new());
        let game = KaBacon {
            mode: Mode::Scores,
            ..Default::default()
        };
        let components = game.components(&world);
        assert!(components.iter().all(|component| !matches!(component, GuiComponent::Text { text, .. } if text.trim().is_empty())));
        assert!(components.iter().any(
            |component| matches!(component, GuiComponent::Text { text, .. } if text == "10:")
        ));
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .mini_game_scores
            .entry(5)
            .or_default()
            .record(MiniGameScore {
                points: 3,
                text: "3 Bacon Caught".into(),
            });
        assert!(game.components(&world).iter().all(|component| !matches!(component, GuiComponent::Text { text, .. } if text.trim().is_empty())));
    }

    #[test]
    fn full_table_rejection_preserves_the_loss_notification() {
        let world = World::new();
        let mut quest = QuestInfo::new();
        for points in 10..20 {
            quest
                .mini_game_scores
                .entry(5)
                .or_default()
                .record(MiniGameScore {
                    points,
                    text: points.to_string(),
                });
        }
        world.add_unique(quest);
        let mut game = KaBacon {
            paddles: 1,
            caught: 2,
            ..Default::default()
        };
        game.bacon[0] = Bacon {
            x: 1.0,
            y: 236.0,
            speed: 50.0,
        };
        game.advance(
            &Time {
                total: Duration::from_millis(16),
                elapsed: Duration::from_millis(16),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.rank, None);
        assert!(game.components(&world).iter().any(|component| matches!(component, GuiComponent::Text { text, .. } if text == "Score not in top 10.")));
    }
}
