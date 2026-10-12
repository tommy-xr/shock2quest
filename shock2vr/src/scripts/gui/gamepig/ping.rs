//! Paddle game with fixed 33 ms simulation ticks and an accelerating opponent.
use super::{GamePigGuiMsg, sprite};
use crate::{
    gui::{self, GuiComponent, GuiCursor},
    scripts::gui::PanelText,
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use rand::Rng;
use shipyard::World;
use std::time::Duration;

const WIDTH: i32 = 139;
const HEIGHT: i32 = 275;
const TICK: Duration = Duration::from_millis(33);

#[derive(Clone, Debug)]
pub(super) struct Ping {
    ball: [i32; 2],
    velocity: [i32; 2],
    player: i32,
    player_speed: i32,
    computer: i32,
    computer_speed: i32,
    score: [i32; 2],
    last_point: usize,
    pause: Duration,
    remainder: Duration,
    last_frame: Option<Duration>,
    pub stick: Option<f32>,
}

impl Default for Ping {
    fn default() -> Self {
        Self {
            ball: [WIDTH / 2, HEIGHT / 2],
            velocity: [0, 0],
            player: WIDTH / 2,
            player_speed: 0,
            computer: WIDTH / 2,
            computer_speed: 0,
            score: [0, 0],
            last_point: 0,
            pause: Duration::ZERO,
            remainder: Duration::ZERO,
            last_frame: None,
            stick: None,
        }
    }
}

impl Ping {
    pub fn won(&self) -> bool {
        self.score.contains(&7)
    }

    pub fn advance(&mut self, time: &Time, cursor: Option<&GuiCursor>, controller: bool) -> bool {
        if self.last_frame == Some(time.total) {
            return false;
        }
        self.last_frame = Some(time.total);
        let stick = self.stick.take().unwrap_or(0.0);
        if !self.pause.is_zero() {
            // A score pause must not replay pre-pause motion after release.
            self.player_speed = 0;
            self.pause = self.pause.saturating_sub(time.elapsed);
            return self.won() && self.pause.is_zero();
        }
        self.remainder += time.elapsed;
        let mut rng = rand::thread_rng();
        while self.remainder >= TICK {
            self.remainder -= TICK;
            self.tick(&mut rng);
            self.player_speed = if controller {
                let speed = (stick.clamp(-1.0, 1.0) * 8.0).round() as i32;
                // Thumbstick motion stays inside the visible paddle field.
                (self.player + speed).clamp(6, WIDTH - 7) - self.player
            } else {
                self.pointer_speed(cursor)
            };
            if !self.pause.is_zero() {
                self.remainder = Duration::ZERO;
                break;
            }
        }
        false
    }

    fn pointer_speed(&self, cursor: Option<&GuiCursor>) -> i32 {
        let Some(cursor) = cursor else { return 0 };
        let x = cursor.position.x as i32 - 13;
        let y = cursor.position.y as i32 - 11;
        if !(-10..WIDTH + 10).contains(&x) || !(-10..HEIGHT + 10).contains(&y) {
            return 0;
        }
        let distance = x - self.player;
        distance.signum() * (distance.abs() / 6 + 1).min(8)
    }

    fn point(&mut self, player: usize) {
        self.score[player] += 1;
        self.last_point = player;
        self.velocity = [0, 0];
        self.pause = Duration::from_secs(if self.won() { 4 } else { 2 });
    }

    fn tick(&mut self, rng: &mut impl Rng) {
        if self.velocity == [0, 0] {
            let speed = ((self.score[0] + self.score[1]) as f32 * 0.8 + 2.3) as i32;
            let direction = if rng.gen_range(0..3) == 2 { 1 } else { -1 };
            self.velocity = [0, direction * speed];
            self.ball = [
                if direction < 0 {
                    self.computer
                } else {
                    self.player
                },
                HEIGHT / 2,
            ];
        }
        let desired = if self.velocity[1] < 0 && (self.computer - self.ball[0]).abs() > 6 {
            (self.ball[0] - self.computer).signum()
                * (((self.ball[0] - self.computer).abs() >> 3) + 2)
        } else {
            0
        };
        self.computer_speed += (desired - self.computer_speed).signum();
        self.ball[0] += self.velocity[0];
        self.ball[1] += self.velocity[1];
        self.player += self.player_speed;
        if self.ball[0] < 3 {
            self.ball[0] = 6 - self.ball[0];
            self.velocity[0] = -self.velocity[0];
        }
        if self.ball[0] > WIDTH - 4 {
            self.ball[0] = 2 * (WIDTH - 4) - self.ball[0];
            self.velocity[0] = -self.velocity[0];
        }
        if self.velocity[1] >= 0 {
            if self.ball[1] - 2 > HEIGHT - 3 {
                self.point(1);
                return;
            }
            if self.ball[1] > HEIGHT - 9 && (self.ball[0] - self.player).abs() <= 9 {
                self.velocity[1] = -self.velocity[1];
                self.velocity[0] =
                    (bounce_angle(self.player, self.ball[0]) - self.velocity[0]) >> 1;
            }
        }
        self.computer += self.computer_speed;
        if self.velocity[1] < 0 {
            if self.ball[1] + 2 < 3 {
                self.point(0);
                return;
            }
            if self.ball[1] < 9 && (self.ball[0] - self.computer).abs() <= 9 {
                self.velocity[1] = -self.velocity[1];
                self.velocity[0] = ((bounce_angle(self.computer, self.ball[0]) - self.velocity[0])
                    >> 1)
                    + rng.gen_range(-1..=1);
            }
        }
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut components = vec![
            gui::image("pongback.pcx")
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0)),
            sprite("pongpadd.pcx", self.computer, 3, 12.0, 4.0),
            sprite("pongpadd.pcx", self.player, HEIGHT - 7, 12.0, 4.0),
            sprite("pongball.pcx", self.ball[0], self.ball[1], 6.0, 6.0),
        ];
        if !self.pause.is_zero() {
            let (key, fallback, x) = if self.last_point == 0 {
                ("YouHave", "You Have", 51.0)
            } else {
                ("ComputerHas", "Xerxes Has", 44.0)
            };
            let mut text = |key, fallback, x: f32, y: f32, score: Option<i32>| {
                let line = PanelText::string(world, "minigame", key, fallback);
                let line =
                    score.map_or_else(|| line.clone(), |value| PanelText::format(&line, &[value]));
                components.push(PanelText::text(
                    &line,
                    Rect::new(13.0 + x, 11.0 + y, 139.0 - x, 15.0),
                ));
            };
            text(key, fallback, x, 45.0, None);
            if self.won() {
                text("Won", "Won", 61.0, 60.0, None);
            } else {
                text("Scored", "Scored", 53.0, 60.0, None);
            }
            text("YouScore", "You: %d", 44.0, 135.0, Some(self.score[0]));
            text(
                "ComputerScore",
                "Xerxes: %d",
                44.0,
                150.0,
                Some(self.score[1]),
            );
        }
        components
    }
}

fn bounce_angle(paddle: i32, ball: i32) -> i32 {
    let angle = (ball - paddle) / 2;
    if angle.abs() <= 2 {
        0
    } else if angle.abs() > 7 {
        angle.signum() * 7
    } else {
        angle - angle.signum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::point2;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn ping_wall_reflection_preserves_overshoot_and_paddle_reflects_angle() {
        let mut rng = StdRng::seed_from_u64(9);
        let mut game = Ping {
            ball: [4, 100],
            velocity: [-4, 2],
            ..Default::default()
        };
        game.tick(&mut rng);
        assert_eq!(game.ball[0], 6);
        assert_eq!(game.velocity[0], 4);
        game.ball = [WIDTH - 5, 100];
        game.velocity = [4, 2];
        game.tick(&mut rng);
        assert_eq!(game.ball[0], WIDTH - 7);
        assert_eq!(game.velocity[0], -4);
        game.ball = [game.player + 7, HEIGHT - 10];
        game.velocity = [0, 2];
        game.tick(&mut rng);
        assert_eq!(game.velocity, [1, -2]);
        assert_eq!(game.score, [0, 0]);
        assert_eq!(bounce_angle(50, 53), 0);
        assert_eq!(bounce_angle(50, 46), 0);
        assert_eq!(bounce_angle(50, 68), 7);
        assert_eq!(bounce_angle(50, 32), -7);
    }

    #[test]
    fn ping_misses_award_the_opponent_and_seventh_point_ends_after_four_seconds() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut game = Ping {
            ball: [20, HEIGHT - 1],
            velocity: [0, 2],
            ..Default::default()
        };
        game.tick(&mut rng);
        assert_eq!(game.score, [0, 1]);
        assert_eq!(game.velocity, [0, 0]);
        assert_eq!(game.pause, Duration::from_secs(2));
        game.score[0] = 6;
        game.ball = [20, 1];
        game.velocity = [0, -2];
        game.tick(&mut rng);
        assert_eq!(game.score, [7, 1]);
        assert!(game.won());
        assert_eq!(game.pause, Duration::from_secs(4));
        let ball = game.ball;
        assert!(!game.advance(
            &Time {
                total: Duration::from_millis(3999),
                elapsed: Duration::from_millis(3999)
            },
            None,
            false
        ));
        assert_eq!(game.ball, ball);
        assert!(game.advance(
            &Time {
                total: Duration::from_secs(4),
                elapsed: Duration::from_millis(1)
            },
            None,
            false
        ));
    }

    #[test]
    fn ping_opponent_accelerates_and_serves_speed_up_with_score() {
        let mut rng = StdRng::seed_from_u64(12);
        let mut game = Ping {
            ball: [110, 100],
            velocity: [0, -2],
            ..Default::default()
        };
        game.tick(&mut rng);
        assert_eq!(game.computer_speed, 1);
        game.tick(&mut rng);
        assert_eq!(game.computer_speed, 2);
        game.velocity[1] = 2;
        game.tick(&mut rng);
        assert_eq!(game.computer_speed, 1);
        game.score = [3, 2];
        game.velocity = [0, 0];
        game.tick(&mut rng);
        assert_eq!(game.velocity[0], 0);
        assert_eq!(game.velocity[1].abs(), 6);
    }

    #[test]
    fn ping_fixed_tick_remainders_match_chunking_and_duplicate_frames_do_not_advance() {
        let mut chunked = Ping {
            ball: [60, 130],
            velocity: [1, 2],
            ..Default::default()
        };
        let mut single = chunked.clone();
        for millis in 1..=1000 {
            chunked.advance(
                &Time {
                    total: Duration::from_millis(millis),
                    elapsed: Duration::from_millis(1),
                },
                None,
                false,
            );
        }
        let frame = Time {
            total: Duration::from_secs(1),
            elapsed: Duration::from_secs(1),
        };
        single.advance(&frame, None, false);
        assert_eq!(single.ball, chunked.ball);
        assert_eq!(single.remainder, chunked.remainder);
        let position = single.ball;
        single.advance(&frame, None, false);
        assert_eq!(single.ball, position);
    }

    #[test]
    fn ping_pointer_targets_field_coordinates_and_absent_input_stops() {
        let game = Ping::default();
        let cursor = |x, y| GuiCursor {
            position: point2(x, y),
            held_entity_id: None,
        };
        assert_eq!(game.pointer_speed(Some(&cursor(82.0, 100.0))), 0);
        assert_eq!(game.pointer_speed(Some(&cursor(20.0, 100.0))), -8);
        assert_eq!(game.pointer_speed(Some(&cursor(-20.0, 100.0))), 0);
        assert_eq!(game.pointer_speed(None), 0);
        let mut game = Ping {
            velocity: [0, 2],
            stick: Some(-1.0),
            ..Default::default()
        };
        for tick in 1..=3 {
            game.advance(
                &Time {
                    total: TICK * tick,
                    elapsed: TICK,
                },
                None,
                true,
            );
        }
        assert_eq!(
            game.player,
            WIDTH / 2 - 8,
            "one sampled axis cannot stay latched after release"
        );
        assert_eq!(game.player_speed, 0);
    }
    #[test]
    fn ping_releasing_during_a_score_pause_cannot_replay_the_old_speed() {
        let mut game = Ping {
            velocity: [0, 2],
            player_speed: -8,
            pause: Duration::from_secs(2),
            ..Default::default()
        };
        let player = game.player;
        game.advance(
            &Time {
                total: Duration::from_secs(2),
                elapsed: Duration::from_secs(2),
            },
            None,
            true,
        );
        game.advance(
            &Time {
                total: Duration::from_secs(2) + TICK,
                elapsed: TICK,
            },
            None,
            true,
        );
        assert_eq!(game.player, player);
        assert_eq!(game.player_speed, 0);
    }
}
