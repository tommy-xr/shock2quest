//! Cross six lanes of traffic, with campaign scores for completed crossings.
use super::{GamePigGuiMsg, score_rank, score_table, sprite};
use crate::{
    gui::{self, GuiComponent},
    mini_game_scores::MiniGameScore,
    scripts::{Effect, gui::PanelText},
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use rand::Rng;
use shipyard::World;
use std::time::Duration;

const SPACING: [f32; 3] = [70.0, 110.0, 85.0];
const SPEED: [f32; 3] = [10.0, 30.0, 20.0];
const STEP: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug)]
struct Lane {
    kind: usize,
    offset: f32,
    up: bool,
}

impl Lane {
    fn positions(&self) -> impl Iterator<Item = f32> + '_ {
        (0..5)
            .map(|i| {
                if self.up {
                    240.0 - self.offset - i as f32 * SPACING[self.kind]
                } else {
                    self.offset + i as f32 * SPACING[self.kind]
                }
            })
            .filter(|y| *y >= 0.0 && *y + 16.0 <= 256.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Play,
    Dead,
    Win,
    Scores,
}

#[derive(Clone, Debug)]
pub(super) struct StreetHog {
    lanes: [Lane; 6],
    column: i32,
    row: i32,
    wins: i32,
    lives: usize,
    hacked: bool,
    mode: Mode,
    pause: Duration,
    clock: Duration,
    debt: Duration,
    last_frame: Option<Duration>,
    recorded: bool,
    rank: Option<usize>,
}

impl StreetHog {
    pub fn new(hacked: bool) -> Self {
        let mut game = Self {
            lanes: [Lane {
                kind: 0,
                offset: 0.0,
                up: false,
            }; 6],
            column: 0,
            row: 8,
            wins: 0,
            lives: if hacked { 3 } else { 2 },
            hacked,
            mode: Mode::Play,
            pause: Duration::ZERO,
            clock: Duration::ZERO,
            debt: Duration::ZERO,
            last_frame: None,
            recorded: false,
            rank: None,
        };
        game.reset(&mut rand::thread_rng());
        game
    }

    fn reset(&mut self, rng: &mut impl Rng) {
        self.column = 0;
        self.row = 8;
        self.mode = Mode::Play;
        self.pause = Duration::ZERO;
        let mut previous = 3;
        for lane in &mut self.lanes {
            let mut kind = rng.gen_range(0..3);
            while kind == previous {
                kind = rng.gen_range(0..3);
            }
            *lane = Lane {
                kind,
                offset: rng.gen_range(0..16) as f32,
                up: rng.gen_bool(0.5),
            };
            previous = kind;
        }
    }

    pub fn restart(&mut self) {
        if self.mode == Mode::Scores {
            let last_frame = self.last_frame;
            *self = Self::new(self.hacked);
            self.last_frame = last_frame;
        }
    }

    pub fn click(&mut self, column: i32, row: i32) {
        self.navigate(column - self.column, row - self.row);
    }

    pub fn navigate(&mut self, x: i32, y: i32) {
        if self.mode != Mode::Play {
            return;
        }
        if x.abs() > y.abs() {
            self.column = (self.column + x.signum()).clamp(0, 7);
        } else {
            self.row = (self.row + y.signum()).clamp(0, 15);
        }
        // Check immediately: several clicks between frames must not skip a car.
        self.check_collision();
        if self.mode == Mode::Play && self.column == 7 {
            self.wins += 1;
            self.mode = Mode::Win;
            self.pause = Duration::ZERO;
        }
    }

    fn check_collision(&mut self) {
        if self.mode != Mode::Play || !(1..=6).contains(&self.column) {
            return;
        }
        let hog_y = self.row as f32 * 16.0;
        if self.lanes[self.column as usize - 1]
            .positions()
            .any(|y| y < hog_y + 16.0 && y + 16.0 > hog_y)
        {
            self.mode = Mode::Dead;
            self.pause = Duration::ZERO;
        }
    }

    fn tick(&mut self, elapsed: Duration, rng: &mut impl Rng) {
        self.debt += elapsed;
        // Fixed simulation slices preserve collisions and pause transitions on long frames.
        while self.debt >= STEP {
            self.debt -= STEP;
            self.clock += STEP;
            if matches!(self.mode, Mode::Dead | Mode::Win) {
                self.pause += STEP;
                if self.pause >= Duration::from_secs(2) {
                    if self.mode == Mode::Dead && self.lives == 0 {
                        self.mode = Mode::Scores;
                    } else {
                        if self.mode == Mode::Dead {
                            self.lives -= 1;
                        }
                        self.reset(rng);
                    }
                }
            }
            let difficulty = 1.0 + self.wins as f32 * 0.05;
            for lane in &mut self.lanes {
                lane.offset = (lane.offset + SPEED[lane.kind] * difficulty * STEP.as_secs_f32())
                    % SPACING[lane.kind];
            }
            self.check_collision();
        }
    }

    pub fn advance(&mut self, time: &Time, world: &World) -> Effect {
        if self.last_frame == Some(time.total) {
            return Effect::NoEffect;
        }
        self.last_frame = Some(time.total);
        self.tick(time.elapsed, &mut rand::thread_rng());
        if self.mode == Mode::Scores && !self.recorded {
            self.recorded = true;
            self.rank = score_rank(world, 7, self.wins);
            let format = PanelText::string(world, "minigame", "HoggerScore", "Score: %d");
            return Effect::RecordMiniGameScore {
                game: 7,
                score: MiniGameScore {
                    points: self.wins,
                    text: PanelText::format(&format, &[self.wins]),
                },
            };
        }
        Effect::NoEffect
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        // Opaque keyed sprites retain the same brightness in flat and mounted views.
        let mut components = vec![
            gui::image("hogback.pcx")
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0)),
        ];
        let suffix = if (self.clock.as_millis() / 500).is_multiple_of(2) {
            ""
        } else {
            "2"
        };
        if self.mode == Mode::Scores {
            components.extend(score_table(
                world,
                7,
                self.rank,
                Rect::new(30.0, 21.0, 122.0, 120.0),
            ));
            for x in [32, 64, 96] {
                components.push(
                    sprite(&format!("hogdan{suffix}.pcx"), x + 8, 158, 16.0, 16.0).with_alpha(1.0),
                );
            }
            components.push(
                gui::button(GamePigGuiMsg::HogRestart)
                    .with_rect(Rect::new(13.0, 11.0, 140.0, 276.0))
                    .with_alpha(0.0)
                    .with_label("hog-restart"),
            );
            return components;
        }
        for (index, lane) in self.lanes.iter().enumerate() {
            let kind = ["tr", "ra", "do"][lane.kind];
            let direction = if lane.up { "u" } else { "d" };
            for y in lane.positions() {
                components.push(
                    sprite(
                        &format!("hog{kind}{direction}{suffix}.pcx"),
                        14 + (index as i32 + 1) * 16,
                        y as i32 + 8,
                        16.0,
                        16.0,
                    )
                    .with_alpha(1.0),
                );
            }
        }
        let hog = if self.mode == Mode::Dead {
            "hogsp"
        } else if self.mode == Mode::Win {
            "hogdan"
        } else {
            "hog"
        };
        components.push(
            sprite(
                &format!("{hog}{suffix}.pcx"),
                14 + self.column * 16,
                8 + self.row * 16,
                16.0,
                16.0,
            )
            .with_alpha(1.0),
        );
        for row in 0..16 {
            for column in 0..8 {
                components.push(
                    gui::button(GamePigGuiMsg::HogCell { column, row })
                        .with_rect(Rect::new(
                            19.0 + column as f32 * 16.0,
                            11.0 + row as f32 * 16.0,
                            16.0,
                            16.0,
                        ))
                        .with_alpha(0.0)
                        .with_label(&format!("hog-{column}-{row}")),
                );
            }
        }
        let score = PanelText::string(world, "minigame", "HoggerScore", "Score: %d");
        components.push(PanelText::text(
            &PanelText::format(&score, &[self.wins]),
            Rect::new(23.0, 267.0, 46.0, 15.0),
        ));
        let message = match self.mode {
            Mode::Dead => Some(("HoggerLose", "Splat!")),
            Mode::Win => Some(("HoggerWin", "Safe!")),
            _ => None,
        };
        if let Some((key, fallback)) = message {
            components.push(PanelText::text(
                &PanelText::string(world, "minigame", key, fallback),
                Rect::new(68.0, 267.0, 35.0, 15.0),
            ));
        }
        for i in 0..self.lives {
            components.push(
                sprite(
                    &format!("hog{suffix}.pcx"),
                    98 + i as i32 * 16,
                    264,
                    16.0,
                    16.0,
                )
                .with_alpha(1.0),
            );
        }
        components
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn movement_is_one_cell_dominant_axis_and_bounded() {
        let mut game = StreetHog::new(false);
        game.click(0, 0);
        assert_eq!((game.column, game.row), (0, 7));
        for _ in 0..20 {
            game.navigate(-1, 0);
            game.navigate(0, -1);
        }
        assert_eq!((game.column, game.row), (0, 0));
        for _ in 0..20 {
            game.navigate(0, 1);
        }
        assert_eq!(game.row, 15);
    }

    #[test]
    fn aligned_car_collision_latches_and_blocks_movement() {
        let mut game = StreetHog::new(false);
        game.row = 0;
        game.lanes[0] = Lane {
            kind: 0,
            offset: 0.0,
            up: false,
        };
        game.navigate(1, 0);
        assert_eq!(game.mode, Mode::Dead);
        game.navigate(1, 0);
        assert_eq!(game.column, 1);
        game.tick(Duration::from_millis(1990), &mut StdRng::seed_from_u64(1));
        assert_eq!(game.lives, 2);
        game.tick(STEP, &mut StdRng::seed_from_u64(1));
        assert_eq!((game.column, game.row, game.lives), (0, 8, 1));
    }

    #[test]
    fn long_frame_cannot_tunnel_and_matches_short_frames() {
        let mut game = StreetHog::new(false);
        game.column = 1;
        game.row = 4;
        game.lanes[0] = Lane {
            kind: 1,
            offset: 0.0,
            up: false,
        };
        let mut split = game.clone();
        game.tick(Duration::from_secs(2), &mut StdRng::seed_from_u64(2));
        let mut rng = StdRng::seed_from_u64(2);
        for _ in 0..200 {
            split.tick(STEP, &mut rng);
        }
        assert_eq!(game.mode, Mode::Dead);
        assert_eq!(game.mode, split.mode);
        assert_eq!(game.pause, split.pause);
        assert_eq!(game.lanes[0].offset, split.lanes[0].offset);
    }

    #[test]
    fn crossing_scores_once_then_regenerates_distinct_lanes() {
        let mut game = StreetHog::new(false);
        game.column = 6;
        game.navigate(1, 0);
        game.navigate(1, 0);
        assert_eq!(game.wins, 1);
        assert_eq!(game.mode, Mode::Win);
        game.tick(Duration::from_secs(2), &mut StdRng::seed_from_u64(3));
        assert_eq!((game.column, game.row, game.wins), (0, 8, 1));
        assert!(
            game.lanes
                .windows(2)
                .all(|pair| pair[0].kind != pair[1].kind)
        );
    }

    #[test]
    fn last_death_records_once_and_restart_keeps_hacked_extra_life() {
        let world = World::new();
        world.add_unique(crate::quest_info::QuestInfo::new());
        let mut game = StreetHog::new(true);
        assert_eq!(game.lives, 3);
        game.mode = Mode::Dead;
        game.lives = 0;
        game.wins = 4;
        let frame = Time {
            total: Duration::from_secs(2),
            elapsed: Duration::from_secs(2),
        };
        assert!(matches!(
            game.advance(&frame, &world),
            Effect::RecordMiniGameScore {
                game: 7,
                score: MiniGameScore { points: 4, .. }
            }
        ));
        assert!(matches!(game.advance(&frame, &world), Effect::NoEffect));
        assert!(
            game.components(&world)
                .iter()
                .any(|c| matches!(c, GuiComponent::Button { .. }))
        );
        game.restart();
        assert_eq!((game.lives, game.wins, game.mode), (3, 0, Mode::Play));
    }
}
