use std::time::Duration;

use cgmath::vec2;
use rand::Rng;
use shipyard::World;

use super::{GamePigGuiMsg, PanelText};
use crate::{
    gui::{self, GuiComponent, GuiCursor},
    mini_game_scores::MiniGameScore,
    scripts::Effect,
    time::Time,
    ui::Rect,
};

const WIDTH: usize = 11;
const HEIGHT: usize = 22;
const CELL: i32 = 12;
const SHAPES: [&[u8; 16]; 5] = [
    b".0...0...0...0..",
    b".1...11..1......",
    b".2...2...22.....",
    b"..3...3..33.....",
    b".....44..44.....",
];
const COLORS: [[u8; 3]; 5] = [
    [0, 0, 255],
    [0, 255, 0],
    [255, 255, 0],
    [0, 255, 255],
    [255, 0, 0],
];

#[derive(Clone, Debug)]
pub(super) struct PigStacker {
    board: [[Option<usize>; WIDTH]; HEIGHT],
    piece: [bool; 16],
    kind: usize,
    next: usize,
    x: i32,
    y: i32,
    direction: i32,
    stuck: u32,
    score: i32,
    level: i32,
    elapsed: Duration,
    pixels: f32,
    drop: bool,
    finished: bool,
    rank: Option<usize>,
    last_frame: Option<Duration>,
    pub stick: Option<f32>,
}

impl Default for PigStacker {
    fn default() -> Self {
        let mut game = Self {
            board: [[None; WIDTH]; HEIGHT],
            piece: [false; 16],
            kind: 0,
            next: rand::thread_rng().gen_range(0..SHAPES.len()),
            x: 36,
            y: -48,
            direction: 1,
            stuck: 0,
            score: 0,
            level: 1,
            elapsed: Duration::ZERO,
            pixels: 0.0,
            drop: false,
            finished: false,
            rank: None,
            last_frame: None,
            stick: None,
        };
        game.spawn();
        game
    }
}

impl PigStacker {
    fn spawn(&mut self) {
        self.kind = self.next;
        self.piece = SHAPES[self.kind].map(|cell| cell != b'.');
        self.next = rand::thread_rng().gen_range(0..SHAPES.len());
        self.x = 36;
        self.y = -48;
        self.direction = 1;
    }

    fn collision(&self, x: i32, y: i32, piece: &[bool; 16]) -> bool {
        piece.iter().enumerate().any(|(i, &filled)| {
            if !filled {
                return false;
            }
            let x = x + (i % 4) as i32;
            let y = y + (i / 4) as i32;
            x < 0
                || x >= WIDTH as i32
                || y >= HEIGHT as i32
                || (y >= 0 && self.board[y as usize][x as usize].is_some())
        })
    }

    pub fn primary(&mut self) {
        if self.finished {
            let last_frame = self.last_frame;
            *self = Self::default();
            self.last_frame = last_frame;
            return;
        }
        let rotated = std::array::from_fn(|i| self.piece[(3 - i % 4) * 4 + i / 4]);
        let x = self.x.div_euclid(CELL);
        let y = self.y.div_euclid(CELL);
        for dx in 0..=i32::from(self.x % CELL != 0) {
            for dy in 0..=i32::from(self.y % CELL != 0) {
                if self.collision(x + dx, y + dy, &rotated) {
                    return;
                }
            }
        }
        self.piece = rotated;
    }

    pub fn hard_drop(&mut self) {
        if !self.finished {
            self.drop = true;
        }
    }

    fn place(&mut self) {
        let x = self.x.div_euclid(CELL);
        let y = self.y.div_euclid(CELL);
        // The four-row piece envelope must have entered the board before it
        // locks; the top-out boundary does not depend on its rotation.
        self.finished = y < 0;
        for (i, &filled) in self.piece.iter().enumerate() {
            let px = x + (i % 4) as i32;
            let py = y + (i / 4) as i32;
            if filled && (0..WIDTH as i32).contains(&px) && (0..HEIGHT as i32).contains(&py) {
                self.board[py as usize][px as usize] = Some(self.kind);
            }
        }
        if self.finished {
            return;
        }
        self.score += self.level;
        let mut line_score = 10 * self.level;
        for row in 0..HEIGHT {
            if self.board[row].iter().all(Option::is_some) {
                self.score += line_score;
                line_score *= 2;
                self.board.copy_within(0..row, 1);
                self.board[0] = [None; WIDTH];
            }
        }
    }

    /// One authored pixel step, including the short lock delay. Returns true
    /// when this piece locks so a hard drop never advances the next piece.
    fn step(&mut self, direction: impl Fn(i32) -> i32) -> bool {
        if self.x % CELL == 0 {
            self.direction = direction(self.x.div_euclid(CELL));
            let x = self.x.div_euclid(CELL) + self.direction;
            let y = self.y.div_euclid(CELL);
            if self.collision(x, y, &self.piece)
                || (self.y % CELL != 0 && self.collision(x, y + 1, &self.piece))
            {
                self.direction = 0;
            }
        }
        self.x += self.direction;
        if self.y % CELL == 0
            && (0..=i32::from(self.x % CELL != 0)).any(|dx| {
                self.collision(
                    self.x.div_euclid(CELL) + dx,
                    self.y.div_euclid(CELL) + 1,
                    &self.piece,
                )
            })
        {
            self.stuck += 1;
        } else {
            self.stuck = 0;
            self.y += 1;
        }
        if self.stuck > 16 && self.x % CELL == 0 {
            self.drop = false;
            self.place();
            self.spawn();
            return true;
        }
        false
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
        let target = cursor
            .filter(|cursor| cursor.position.x >= 0.0)
            .map(|cursor| (cursor.position.x as i32 - 13 - 24) / CELL);
        let direction = |column: i32| {
            if controller {
                if axis.abs() >= 0.15 {
                    axis.signum() as i32
                } else {
                    0
                }
            } else {
                target.map_or(0, |target| (target - column).signum())
            }
        };
        self.elapsed += time.elapsed;
        if self.finished {
            return Effect::NoEffect;
        }
        self.level = ((self.elapsed.as_secs() / 60) as i32 + 1).min(10);
        self.pixels += (32.0 + 22.4 * (self.level - 1) as f32) * time.elapsed.as_secs_f32();
        if self.drop {
            while !self.step(direction) {}
        } else {
            while self.pixels >= 1.0 && !self.finished {
                self.step(direction);
                self.pixels -= 1.0;
            }
        }
        if self.finished {
            self.rank = super::score_rank(world, 10, self.score);
            let format = PanelText::string(
                world,
                "minigame",
                "TetrisScoreForm",
                "%d points at level %d",
            );
            return Effect::RecordMiniGameScore {
                game: 10,
                score: MiniGameScore {
                    points: self.score,
                    text: PanelText::format(&format, &[self.score, self.level]),
                },
            };
        }
        Effect::NoEffect
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let field = Rect::new(13.0, 11.0, 140.0, 276.0);
        let mut components = vec![GuiComponent::Fill {
            position: vec2(field.x, field.y),
            size: vec2(field.w, field.h),
            color: [0, 0, 0],
            alpha: 1.0,
        }];
        let mut block = |x: i32, y: i32, color: [u8; 3]| {
            let top = y.max(0);
            let bottom = (y + 11).min(276);
            let left = x.max(0);
            let right = (x + 11).min(140);
            if bottom > top && right > left {
                components.push(GuiComponent::Fill {
                    position: vec2(13.0 + left as f32, 11.0 + top as f32),
                    size: vec2((right - left) as f32, (bottom - top) as f32),
                    // Keep the final board visible behind the score readout.
                    color: color.map(|channel| if self.finished { channel / 4 } else { channel }),
                    alpha: 1.0,
                });
            }
        };
        for (y, row) in self.board.iter().enumerate() {
            for (x, kind) in row.iter().enumerate() {
                if let Some(kind) = kind {
                    block(x as i32 * CELL, y as i32 * CELL, COLORS[*kind]);
                }
            }
        }
        for (i, &filled) in self.piece.iter().enumerate() {
            if filled {
                block(
                    self.x + (i % 4) as i32 * CELL,
                    self.y + (i / 4) as i32 * CELL,
                    COLORS[self.kind].map(|channel| if channel == 0 { 0 } else { 128 }),
                );
            }
        }
        if self.finished {
            components.extend(super::score_table(
                world,
                10,
                self.rank,
                Rect::new(43.0, 36.0, 110.0, 220.0),
            ));
            if self.elapsed.as_secs() % 2 == 0 {
                let text = PanelText::string(world, "minigame", "TetrisGameOver", "Game Over");
                components.push(PanelText::centered(
                    &text,
                    Rect::new(13.0, 148.0, 139.0, 15.0),
                ));
            }
        }
        let format = PanelText::string(world, "minigame", "TetrisScore", "Level %d Score %d");
        components.push(PanelText::text(
            &PanelText::format(&format, &[self.level, self.score]),
            Rect::new(13.0, 11.0, 140.0, 15.0),
        ));
        components.push(
            gui::button(GamePigGuiMsg::StackerPrimary)
                .with_rect(field)
                .with_label(if self.finished {
                    "stacker-restart"
                } else {
                    "stacker-play"
                })
                .with_alpha(0.0),
        );
        components
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;

    fn world() -> World {
        let world = World::new();
        world.add_unique(QuestInfo::new());
        world
    }

    fn piece(kind: usize) -> PigStacker {
        PigStacker {
            piece: SHAPES[kind].map(|cell| cell != b'.'),
            kind,
            ..Default::default()
        }
    }

    #[test]
    fn five_shapes_rotate_without_wall_kicks() {
        for kind in 0..5 {
            let mut game = piece(kind);
            assert_eq!(game.piece.iter().filter(|&&cell| cell).count(), 4);
            game.y = 24;
            let original = game.piece;
            game.primary();
            if kind == 0 {
                assert_eq!(
                    game.piece,
                    b"....0000........".map(|cell| cell != b'.'),
                    "vertical line rotates into the second row"
                );
            } else if kind < 4 {
                assert_ne!(game.piece, original, "asymmetric pieces visibly rotate");
            }
            for _ in 0..3 {
                game.primary();
            }
            assert_eq!(game.piece, original);
        }
        let mut game = piece(0);
        game.x = -12;
        game.y = 24;
        let original = game.piece;
        game.primary();
        assert_eq!(game.piece, original, "wall collision rejects rotation");
        assert_eq!(game.x, -12, "no wall kick");
        assert!(!game.collision(-1, 2, &game.piece));
        assert!(
            game.collision(-2, -4, &game.piece),
            "offscreen spawn movement stays bounded"
        );
    }

    #[test]
    fn negative_fractional_x_rotation_respects_the_left_wall() {
        let mut game = piece(0);
        game.x = -11;
        game.y = 24;
        let original = game.piece;
        game.primary();
        assert_eq!(
            game.piece, original,
            "visible rotation cannot extend left of the board"
        );
    }

    #[test]
    fn negative_fractional_y_rotation_checks_the_visible_top_row() {
        let mut game = piece(0);
        game.y = -11;
        game.board[0][3] = Some(1);
        let original = game.piece;
        game.primary();
        assert_eq!(
            game.piece, original,
            "partly entering piece overlaps the top row"
        );
    }

    #[test]
    fn fractional_horizontal_overlap_blocks_descent() {
        let mut game = piece(4);
        game.x = 36;
        game.y = 56;
        game.board[8][6] = Some(1);
        for _ in 0..12 {
            game.step(|_| 1);
        }
        assert!(
            game.y <= 60,
            "the square must stop above the occupied row, got {}",
            game.y
        );
    }

    #[test]
    fn pixel_steps_lock_after_seventeen_blocked_ticks_and_drop_only_one_piece() {
        let mut game = piece(0);
        game.y = 18 * CELL;
        for _ in 0..16 {
            assert!(!game.step(|_| 0));
        }
        assert_eq!(game.score, 0);
        assert!(game.step(|_| 0));
        assert_eq!(game.score, 1);
        assert_eq!(
            game.board
                .iter()
                .flatten()
                .filter(|cell| cell.is_some())
                .count(),
            4
        );
        assert_eq!(game.y, -48);
        game.hard_drop();
        game.advance(&Time::default(), None, true, &world());
        assert_eq!(game.score, 2);
        assert!(!game.drop);
        assert_eq!(
            game.y, -48,
            "new piece does not descend in the hard-drop loop"
        );
    }

    #[test]
    fn clearing_rows_doubles_each_line_bonus_and_shifts_the_board() {
        let mut game = piece(0);
        game.level = 3;
        game.x = 36;
        game.y = 18 * CELL;
        for row in 18..HEIGHT {
            game.board[row].fill(Some(1));
            game.board[row][4] = None;
        }
        game.board[17][0] = Some(4);
        game.place();
        assert_eq!(game.score, 3 + 30 + 60 + 120 + 240);
        assert_eq!(game.board[21][0], Some(4));
        assert_eq!(
            game.board
                .iter()
                .flatten()
                .filter(|cell| cell.is_some())
                .count(),
            1
        );
    }

    #[test]
    fn fractional_pixel_debt_level_clock_and_frame_dedup_are_preserved() {
        let world = world();
        let mut game = piece(0);
        for ms in 1..=31 {
            game.advance(
                &Time {
                    elapsed: Duration::from_millis(1),
                    total: Duration::from_millis(ms),
                },
                None,
                true,
                &world,
            );
        }
        assert_eq!(game.y, -48);
        let frame = Time {
            elapsed: Duration::from_millis(1),
            total: Duration::from_millis(32),
        };
        game.advance(&frame, None, true, &world);
        assert_eq!(game.y, -47);
        let pixels = game.pixels;
        game.advance(&frame, None, true, &world);
        assert_eq!(game.pixels, pixels);
        assert_eq!(game.x, 36, "neutral controller input must not move right");
        game.elapsed = Duration::from_millis(59_999);
        game.advance(
            &Time {
                elapsed: Duration::from_millis(1),
                total: Duration::from_secs(60),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.level, 2);
        game.elapsed = Duration::from_secs(1000);
        game.advance(
            &Time {
                elapsed: Duration::from_millis(1),
                total: Duration::from_secs(1001),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.level, 10);
    }

    #[test]
    fn top_out_records_once_and_restart_preserves_the_frame_guard() {
        let world = world();
        let mut game = piece(0);
        game.y = -12;
        game.board[3][4] = Some(1);
        game.score = 42;
        game.hard_drop();
        let frame = Time {
            elapsed: Duration::from_millis(16),
            total: Duration::from_secs(1),
        };
        let effect = game.advance(&frame, None, true, &world);
        assert!(
            matches!(effect, Effect::RecordMiniGameScore { game: 10, score } if score.points == 42)
        );
        assert!(game.finished);
        game.hard_drop();
        assert!(!game.drop);
        assert!(matches!(
            game.advance(
                &Time {
                    total: Duration::from_secs(2),
                    ..frame.clone()
                },
                None,
                true,
                &world
            ),
            Effect::NoEffect
        ));
        game.primary();
        assert!(!game.finished);
        assert_eq!(game.score, 0);
        assert_eq!(game.last_frame, Some(Duration::from_secs(2)));
    }

    #[test]
    fn neutral_samples_finish_only_the_current_horizontal_cell() {
        let world = world();
        let mut game = piece(0);
        game.stick = Some(1.0);
        game.advance(
            &Time {
                elapsed: Duration::from_millis(125),
                total: Duration::from_millis(125),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.x, 40);
        game.advance(
            &Time {
                elapsed: Duration::from_secs(1),
                total: Duration::from_millis(1125),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.x, 48, "neutral finishes alignment, then stops");
        game.advance(
            &Time {
                elapsed: Duration::from_secs(1),
                total: Duration::from_millis(2125),
            },
            None,
            true,
            &world,
        );
        assert_eq!(game.x, 48);
    }
}
