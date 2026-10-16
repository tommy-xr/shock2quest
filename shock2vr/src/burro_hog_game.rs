//! Fixed-step ladder arcade game; coordinates are pixels on the 140×276 canvas.
use rand::Rng;
use std::time::Duration;

mod boards;
pub use boards::BOARDS;
pub const RIGHT: u8 = 1;
pub const LEFT: u8 = 2;
pub const LADDER: u8 = 4;
pub const TOP: u8 = 8;
pub const BOTTOM: u8 = 16;
pub const WIN: u8 = 32;
pub const DRAIN: u8 = 64;
const STEP: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Playing,
    Died,
    Won,
    GameOver,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Walking,
    Falling,
    Climbing,
    Jumping,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Ham {
    pub x: i32,
    pub y: i32,
    pub direction: i32,
    jump_side: i32,
    crossed: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub level: u32,
    pub score: i32,
    pub lives: u8,
    pub mode: Mode,
    pub x: i32,
    pub y: i32,
    pub facing: i32,
    pub motion: Motion,
    pub hams: Vec<Ham>,
    pub frame: u32,
    pub kong_frame: usize,
    pub pause: Duration,
    input: [i32; 2],
    climb_target: i32,
    jump_tick: usize,
    jump_dx: i32,
    debt: Duration,
    throw_ticks: u32,
}
impl Default for Game {
    fn default() -> Self {
        let mut game = Self {
            level: 0,
            score: 0,
            lives: 3,
            mode: Mode::Playing,
            x: 8,
            y: 248,
            facing: 1,
            motion: Motion::Walking,
            hams: Vec::new(),
            frame: 0,
            kong_frame: 0,
            pause: Duration::ZERO,
            input: [0, 0],
            climb_target: 0,
            jump_tick: 0,
            jump_dx: 0,
            debt: Duration::ZERO,
            throw_ticks: 60,
        };
        game.reset_board();
        game
    }
}
impl Game {
    pub fn board(&self) -> &'static [[u8; 17]; 69] {
        &BOARDS[self.level as usize % 2]
    }
    pub fn steer(&mut self, x: i32, y: i32) {
        self.input = [x.signum(), y.signum()];
    }
    pub fn jump(&mut self) {
        if self.mode != Mode::Playing || self.motion != Motion::Walking {
            return;
        }
        self.motion = Motion::Jumping;
        self.jump_tick = 0;
        self.jump_dx = self.input[0] * 2;
        for ham in &mut self.hams {
            ham.jump_side = (ham.x - self.x).signum();
            ham.crossed = false;
        }
    }
    pub fn restart(&mut self) {
        if self.mode == Mode::GameOver && self.pause >= Duration::from_secs(5) {
            *self = Self::default();
        }
    }
    fn reset_board(&mut self) {
        let odd = self.level % 2 == 1;
        self.x = if odd { 127 } else { 8 };
        self.y = 248;
        self.facing = if odd { -1 } else { 1 };
        self.motion = Motion::Falling;
        self.input = [0, 0];
        self.hams.clear();
        self.throw_ticks = 60;
        self.kong_frame = 0;
        self.pause = Duration::ZERO;
        self.mode = Mode::Playing;
    }
    pub fn advance(&mut self, elapsed: Duration, hacked: bool, rng: &mut impl Rng) {
        self.debt += elapsed;
        while self.debt >= STEP {
            self.debt -= STEP;
            self.tick(hacked, rng);
        }
    }
    fn tick(&mut self, hacked: bool, rng: &mut impl Rng) {
        self.frame = self.frame.wrapping_add(1);
        if self.mode != Mode::Playing {
            self.pause += STEP;
            match self.mode {
                Mode::Died if self.pause >= Duration::from_secs(3) => self.reset_board(),
                Mode::Won if self.pause >= Duration::from_secs(4) => {
                    self.level += 1;
                    self.reset_board();
                }
                _ => {}
            }
            return;
        }
        self.move_player();
        if self.mode != Mode::Playing {
            return;
        }
        self.throw_ticks -= 1;
        self.kong_frame = if self.throw_ticks < 20 {
            2
        } else if self.throw_ticks < 40 {
            1
        } else {
            0
        };
        if self.throw_ticks == 0 {
            if self.hams.len() < 32 {
                self.hams.push(Ham {
                    x: 33,
                    y: 35,
                    direction: 1,
                    jump_side: 0,
                    crossed: false,
                });
            }
            self.throw_ticks = rng.gen_range(60..=100);
        }
        self.move_hams(rng);
        // Match the visible footprint: standing art has transparent side margins.
        let margin = if self.motion == Motion::Walking && self.input[0] == 0 {
            3
        } else {
            0
        };
        let y = self.y - if self.motion == Motion::Jumping { 3 } else { 0 };
        let mut hit = false;
        self.hams.retain(|ham| {
            let overlap = ham.x <= self.x + 12 - margin
                && ham.x + 8 >= self.x + margin
                && ham.y <= y + 16
                && ham.y + 8 >= y;
            if overlap && !hacked {
                hit = true;
            }
            !(overlap && hacked)
        });
        if hit || self.y > 276 {
            self.lives -= 1;
            self.mode = if self.lives == 0 {
                Mode::GameOver
            } else {
                Mode::Died
            };
            self.pause = Duration::ZERO;
        }
    }
    fn move_player(&mut self) {
        let board = self.board();
        match self.motion {
            Motion::Climbing => {
                if self.input[1] == 0 {
                    return;
                }
                // A climb ends on its connected platform, never on an intermediate ladder tile.
                if (self.climb_target - (self.y + 16)).signum() != self.input[1] {
                    let row = (self.y + 16) / 4;
                    if let Some(target) = ladder_end(board, (self.x + 6) / 8, row, self.input[1]) {
                        self.climb_target = target;
                    }
                }
                self.y += self.input[1] * 2;
                if (self.y + 16 - self.climb_target).abs() <= 1 {
                    self.y = self.climb_target - 16;
                    self.motion = Motion::Walking;
                }
            }
            Motion::Jumping => {
                const DY: [i32; 12] = [-4, -3, -2, -1, 0, 1, 0, 1, 2, 3, 4, 4];
                let old = self.y + 16;
                self.x = (self.x + self.jump_dx).clamp(0, 127);
                self.y += DY[self.jump_tick];
                self.jump_tick += 1;
                for ham in &mut self.hams {
                    if ham.jump_side != 0 && (ham.x - self.x).signum() != ham.jump_side {
                        ham.crossed = true;
                    }
                }
                if self.y + 16 >= old && self.land(old) {
                    self.finish_jump();
                } else if self.jump_tick == 12 {
                    self.finish_jump();
                    self.motion = Motion::Falling;
                }
            }
            Motion::Falling => {
                let old = self.y + 16;
                self.y += 2;
                self.land(old);
            }
            Motion::Walking => {
                if self.input[1] != 0 {
                    let flag = if self.input[1] < 0 { BOTTOM } else { TOP };
                    for dx in [6, 12, 0] {
                        let col = (self.x + dx) / 8;
                        let row = (self.y + 16) / 4;
                        if tile(board, col, row) & flag != 0 {
                            if let Some(target) = ladder_end(board, col, row, self.input[1]) {
                                self.x = col * 8 - 2;
                                self.climb_target = target;
                                self.motion = Motion::Climbing;
                                return;
                            }
                        }
                    }
                }
                let dx = self.input[0] * 3;
                if dx != 0 {
                    self.facing = dx.signum();
                }
                self.x = (self.x + dx).clamp(0, 127);
                let foot = self.y + 16;
                // Slopes are one tile high; snap to the highest support under either foot.
                let support = (foot - 4..=foot + 4).filter(|y| y % 4 == 0).find(|y| {
                    [3, 6, 9]
                        .iter()
                        .any(|dx| tile(board, (self.x + dx) / 8, y / 4) & 3 != 0)
                });
                if let Some(y) = support {
                    self.y = y - 16;
                } else {
                    self.motion = Motion::Falling;
                }
            }
        }
        if self.motion == Motion::Walking
            && [3, 6, 9]
                .iter()
                .any(|dx| tile(board, (self.x + dx) / 8, (self.y + 16) / 4) & WIN != 0)
        {
            self.mode = Mode::Won;
            self.score += 500;
            self.pause = Duration::ZERO;
        }
    }
    fn land(&mut self, old_foot: i32) -> bool {
        for foot in old_foot..=self.y + 16 {
            if foot % 4 == 0
                && [3, 6, 9]
                    .iter()
                    .any(|dx| tile(self.board(), (self.x + dx) / 8, foot / 4) & 3 != 0)
            {
                self.y = foot - 16;
                self.motion = Motion::Walking;
                return true;
            }
        }
        false
    }
    fn finish_jump(&mut self) {
        for ham in &mut self.hams {
            if ham.crossed && (ham.y + 8 - self.y - 16).abs() < 12 {
                self.score += 100;
            }
            ham.crossed = false;
            ham.jump_side = 0;
        }
    }
    fn move_hams(&mut self, rng: &mut impl Rng) {
        let board = self.board();
        let speed = (3 + self.level).min(5) as i32;
        let fall = (3 + self.level).min(4) as i32;
        for ham in &mut self.hams {
            let mut t = tile(board, ham.x / 8, (ham.y + 9) / 4);
            if t == 0 && ham.direction < 0 {
                t = tile(board, (ham.x + 8) / 8, (ham.y + 9) / 4);
            }
            if t & DRAIN != 0 {
                ham.y = 300;
                continue;
            }
            if t == 0 || t == LADDER || (t & TOP != 0 && rng.gen_bool(0.15)) {
                ham.y += fall;
            } else if t & 3 != 0 {
                ham.direction = if t & RIGHT != 0 { 1 } else { -1 };
                ham.x += ham.direction * speed;
            }
            ham.x = ham.x.clamp(0, 131);
        }
        self.hams.retain(|ham| ham.y < 276);
    }
}
fn tile(board: &[[u8; 17]; 69], x: i32, y: i32) -> u8 {
    if !(0..17).contains(&x) || !(0..69).contains(&y) {
        0
    } else {
        board[y as usize][x as usize]
    }
}
fn ladder_end(board: &[[u8; 17]; 69], col: i32, row: i32, direction: i32) -> Option<i32> {
    let mut y = row + direction;
    while (0..69).contains(&y) {
        let t = tile(board, col, y);
        if t & 3 != 0 {
            return Some(y * 4);
        }
        if t & LADDER == 0 {
            return None;
        }
        y += direction;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    fn tick(game: &mut Game, n: usize) {
        let mut rng = StdRng::seed_from_u64(7);
        game.advance(STEP * n as u32, true, &mut rng);
    }
    fn walk_to(game: &mut Game, x: i32) {
        for _ in 0..200 {
            if (game.x - x).abs() <= 2 && game.motion == Motion::Walking {
                game.steer(0, 0);
                return;
            }
            game.steer((x - game.x).signum(), 0);
            tick(game, 1);
        }
        panic!("could not reach x={x}: {game:?}");
    }
    fn climb(game: &mut Game, col: i32) {
        walk_to(game, col * 8 - 2);
        let y = game.y;
        game.steer(0, -1);
        for _ in 0..100 {
            tick(game, 1);
            if game.y < y && game.motion == Motion::Walking {
                game.steer(0, 0);
                return;
            }
        }
        panic!("could not climb column {col}: {game:?}");
    }
    #[test]
    fn both_courses_are_winnable_through_walking_and_connected_ladders() {
        let mut game = Game::default();
        for cols in [&[12, 3, 12, 9, 12][..], &[3, 9, 3, 13, 7][..]] {
            for col in cols {
                climb(&mut game, *col);
            }
            walk_to(&mut game, 38);
            assert_eq!(game.mode, Mode::Won);
            let score = game.score;
            tick(&mut game, 79);
            assert_eq!(game.score, score);
            tick(&mut game, 1);
            assert_eq!(game.mode, Mode::Playing);
        }
        assert_eq!(game.level, 2);
        assert!(game.score >= 1000);
    }
    #[test]
    fn stopping_and_reversing_on_ladder_preserves_connection() {
        let mut game = Game::default();
        walk_to(&mut game, 94);
        game.steer(0, -1);
        tick(&mut game, 6);
        assert_eq!(game.motion, Motion::Climbing);
        let y = game.y;
        game.steer(0, 0);
        tick(&mut game, 5);
        assert_eq!(game.y, y);
        game.steer(0, 1);
        tick(&mut game, 5);
        assert_eq!(game.motion, Motion::Walking);
        assert_eq!(game.y, 240);
    }
    #[test]
    fn jump_is_bounded_and_airborne_inputs_cannot_double_jump() {
        let mut game = Game::default();
        tick(&mut game, 3);
        let start = game.y;
        game.jump();
        tick(&mut game, 4);
        assert!(game.y < start);
        let step = game.jump_tick;
        game.jump();
        assert_eq!(game.jump_tick, step);
        tick(&mut game, 20);
        assert_eq!(game.motion, Motion::Walking);
        assert_eq!(game.y, start);
    }
    #[test]
    fn collision_costs_one_life_and_hacking_removes_ham() {
        let mut game = Game::default();
        let mut rng = StdRng::seed_from_u64(1);
        tick(&mut game, 4);
        for remaining in [2, 1, 0] {
            game.hams.push(Ham {
                x: game.x,
                y: game.y + 8,
                direction: 1,
                jump_side: 0,
                crossed: false,
            });
            game.advance(STEP, false, &mut rng);
            assert_eq!(game.lives, remaining);
            assert_eq!(
                game.mode,
                if remaining == 0 {
                    Mode::GameOver
                } else {
                    Mode::Died
                }
            );
            tick(&mut game, 60);
        }
        game.restart();
        assert_eq!(game.mode, Mode::GameOver);
        tick(&mut game, 40);
        game.restart();
        assert_eq!(game.lives, 3);
        tick(&mut game, 4);
        game.hams.push(Ham {
            x: game.x,
            y: game.y + 8,
            direction: 1,
            jump_side: 0,
            crossed: false,
        });
        tick(&mut game, 1);
        assert_eq!(game.mode, Mode::Playing);
        assert!(game.hams.is_empty());
    }
    #[test]
    fn jumped_ham_scores_only_once_after_landing() {
        let mut game = Game::default();
        tick(&mut game, 4);
        game.jump();
        game.hams.push(Ham {
            x: game.x + 20,
            y: game.y + 8,
            direction: 1,
            jump_side: -1,
            crossed: true,
        });
        game.finish_jump();
        assert_eq!(game.score, 100);
        game.finish_jump();
        assert_eq!(game.score, 100);
    }
    #[test]
    fn fixed_steps_match_batched_time_and_bound_ham_pool() {
        let mut a = Game::default();
        let mut b = a.clone();
        let mut ra = StdRng::seed_from_u64(9);
        let mut rb = ra.clone();
        a.advance(Duration::from_secs(120), true, &mut ra);
        for _ in 0..7200 {
            b.advance(Duration::from_nanos(16_666_666), true, &mut rb);
        }
        b.advance(Duration::from_nanos(4800), true, &mut rb);
        assert_eq!(a, b);
        assert!(a.hams.len() <= 32);
    }
}
