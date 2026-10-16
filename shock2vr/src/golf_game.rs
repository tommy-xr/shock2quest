//! Nine-hole golf with indexed terrain and a three-click swing.
use rand::Rng;
use std::{sync::Arc, time::Duration};
pub const WIDTH: usize = 140;
pub const HEIGHT: usize = 252;
pub const PAR: [u8; 9] = [4, 5, 4, 3, 4, 4, 4, 5, 3];
const STEP: Duration = Duration::from_millis(10);
const METERS: f32 = 2.2;
const MAX_METER: f32 = 122.0 / 104.0;
const MIN_METER: f32 = -18.0 / 104.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Course {
    pixels: Vec<u8>,
    pub tee: [f32; 2],
    pub hole: [f32; 2],
    pub wind_marker: [f32; 2],
    pub score_marker: [f32; 2],
    pub three_lines: bool,
}
impl Course {
    pub fn from_pixels(pixels: Vec<u8>) -> Option<Self> {
        if pixels.len() != WIDTH * HEIGHT {
            return None;
        }
        let find = |value| {
            pixels
                .iter()
                .position(|p| *p == value)
                .map(|i| [(i % WIDTH) as f32, (i / WIDTH) as f32])
        };
        Some(Self {
            tee: find(32)?,
            hole: find(183)?,
            wind_marker: find(14).unwrap_or([115.0, 207.0]),
            score_marker: find(8).or_else(|| find(11)).unwrap_or([2.0, 2.0]),
            three_lines: find(8).is_some(),
            pixels,
        })
    }
    pub fn terrain(&self, x: f32, y: f32) -> u8 {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= WIDTH as f32
            || y >= HEIGHT as f32
        {
            return 0;
        }
        self.pixels[y as usize * WIDTH + x as usize]
    }
    fn at_hole(&self, ball: [f32; 3]) -> bool {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| self.terrain(ball[0] + dx as f32, ball[1] + dy as f32) == 183)
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Aim,
    Windup,
    Return,
    Flight,
    HoleDone,
    Scores,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    None,
    Water,
    OutOfBounds,
    Sunk,
}
#[derive(Clone, Copy, Debug)]
pub struct Club {
    pub speed: f32,
    pub loft: f32,
    pub meter_speed: f32,
    pub range: f32,
}
pub const CLUBS: [Club; 5] = [
    Club {
        speed: 102.0,
        loft: 0.27,
        meter_speed: 1.2,
        range: 265.0,
    },
    Club {
        speed: 79.0,
        loft: 0.51,
        meter_speed: 1.0,
        range: 210.0,
    },
    Club {
        speed: 58.0,
        loft: 0.78,
        meter_speed: 1.0,
        range: 150.0,
    },
    Club {
        speed: 32.0,
        loft: 1.2,
        meter_speed: 0.85,
        range: 78.0,
    },
    Club {
        speed: 23.0,
        loft: 0.0,
        meter_speed: 0.65,
        range: 66.0,
    },
];
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub courses: Arc<[Course; 9]>,
    pub hole: usize,
    pub club: usize,
    pub ball: [f32; 3],
    pub target: [f32; 2],
    pub origin: [f32; 2],
    pub wind: [f32; 2],
    pub mode: Mode,
    pub strokes: u32,
    pub card: [u32; 9],
    pub meter: f32,
    pub power: f32,
    pub mulligan_used: bool,
    pub notice: Notice,
    pub pause: Duration,
    velocity: [f32; 3],
    acceleration: [f32; 2],
    base_wind: [f32; 2],
    flight_time: f32,
    wind_ticks: u32,
    gust_ticks: u32,
    debt: Duration,
}
impl Game {
    pub fn new(courses: Arc<[Course; 9]>, rng: &mut impl Rng) -> Self {
        let mut game = Self {
            courses,
            hole: 0,
            club: 0,
            ball: [0.0; 3],
            target: [0.0; 2],
            origin: [0.0; 2],
            wind: [0.0; 2],
            mode: Mode::Aim,
            strokes: 0,
            card: [0; 9],
            meter: 0.0,
            power: 0.0,
            mulligan_used: false,
            notice: Notice::None,
            pause: Duration::ZERO,
            velocity: [0.0; 3],
            acceleration: [0.0; 2],
            base_wind: [0.0; 2],
            flight_time: 0.0,
            wind_ticks: 0,
            gust_ticks: 0,
            debt: Duration::ZERO,
        };
        game.start_hole(rng);
        game
    }
    pub fn course(&self) -> &Course {
        &self.courses[self.hole]
    }
    pub fn total(&self) -> u32 {
        self.card.iter().sum()
    }
    pub fn select_club(&mut self, club: usize) {
        if matches!(self.mode, Mode::Aim | Mode::Windup) {
            self.club = club.min(4);
            self.mode = Mode::Aim;
            self.meter = 0.0;
        }
    }
    pub fn aim(&mut self, target: [f32; 2]) {
        if self.mode == Mode::Aim && target.iter().all(|v| v.is_finite()) {
            self.target = [target[0].clamp(0.0, 139.0), target[1].clamp(0.0, 251.0)];
        }
    }
    pub fn primary(&mut self, rng: &mut impl Rng) {
        match self.mode {
            Mode::Aim => {
                self.mode = Mode::Windup;
                self.meter = 0.0;
                self.notice = Notice::None;
            }
            Mode::Windup => {
                self.power = self.meter;
                self.mode = Mode::Return;
            }
            Mode::Return if self.meter < 0.75 => self.launch(rng),
            Mode::HoleDone => {
                if self.hole == 8 {
                    self.mode = Mode::Scores;
                    self.pause = Duration::ZERO;
                } else {
                    self.hole += 1;
                    self.start_hole(rng);
                }
            }
            Mode::Scores if self.pause >= Duration::from_secs(3) => {
                let courses = self.courses.clone();
                *self = Self::new(courses, rng);
            }
            _ => {}
        }
    }
    pub fn mulligan(&mut self, hacked: bool, rng: &mut impl Rng) {
        if hacked && !self.mulligan_used && !matches!(self.mode, Mode::HoleDone | Mode::Scores) {
            self.start_hole(rng);
            self.mulligan_used = true;
        }
    }
    fn start_hole(&mut self, rng: &mut impl Rng) {
        let course = self.course();
        self.ball = [course.tee[0], course.tee[1], 0.0];
        self.origin = self.course().tee;
        self.target = self.course().hole;
        self.club = 0;
        self.strokes = 0;
        self.mode = Mode::Aim;
        self.meter = 0.0;
        self.power = 0.0;
        self.mulligan_used = false;
        self.notice = Notice::None;
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = rng.gen_range(10.0..25.0);
        self.wind = [angle.cos() * speed, angle.sin() * speed];
        self.base_wind = self.wind;
        self.wind_ticks = 0;
        self.gust_ticks = 0;
    }
    pub fn advance(&mut self, elapsed: Duration, rng: &mut impl Rng) {
        self.debt += elapsed;
        while self.debt >= STEP {
            self.debt -= STEP;
            self.tick(rng);
        }
    }
    fn tick(&mut self, rng: &mut impl Rng) {
        if self.mode == Mode::Scores {
            self.pause += STEP;
            return;
        }
        // Wind evolves on a fixed clock, independent of UI refresh frequency.
        self.wind_ticks += 1;
        if self.wind_ticks % 5 == 0 {
            if self.wind_ticks > 400 && self.gust_ticks == 0 && rng.gen_bool(1.0 / 128.0) {
                self.gust_ticks = 50;
                self.wind_ticks = 0;
            }
            let spread = if self.gust_ticks > 0 { 0.26 } else { 0.05 };
            for i in 0..2 {
                self.wind[i] *= rng.gen_range(1.0 - spread..1.0 + spread);
                if self.gust_ticks == 0 {
                    self.wind[i] += (self.base_wind[i] - self.wind[i]) * rng.gen_range(0.0..0.5);
                }
            }
        }
        self.gust_ticks = self.gust_ticks.saturating_sub(1);
        let delta = 0.01 * CLUBS[self.club].meter_speed / 0.8;
        match self.mode {
            Mode::Windup => {
                self.meter += delta;
                if self.meter >= MAX_METER {
                    self.meter = MAX_METER;
                    self.power = self.meter;
                    self.mode = Mode::Return;
                }
            }
            Mode::Return => {
                self.meter -= delta;
                if self.meter <= MIN_METER {
                    self.meter = MIN_METER;
                    self.launch(rng);
                }
            }
            Mode::Flight => self.physics(rng),
            _ => {}
        }
    }
    fn launch(&mut self, rng: &mut impl Rng) {
        let dx = self.target[0] - self.ball[0];
        let dy = self.target[1] - self.ball[1];
        let length = dx.hypot(dy);
        let direction = if length > 0.001 {
            [dx / length, dy / length]
        } else {
            [0.0, -1.0]
        };
        let terrain = self.course().terrain(self.ball[0], self.ball[1]);
        let (_, _, terrain_power) = properties(terrain);
        let mut power = self.power;
        let mut error = self.meter;
        if power > 1.0 {
            error +=
                error / 12.0 * rng.gen_range(-32.0..32.0) * if power < 1.08 { 0.5 } else { 1.0 };
            power = 1.0 + (power - 1.0) * 1.2;
        }
        power *= terrain_power;
        if terrain == 200 && self.club != 3 {
            power *= 0.4;
        }
        let club = CLUBS[self.club];
        let speed = club.speed * power;
        self.velocity = [
            direction[0] * speed,
            direction[1] * speed,
            club.loft * speed,
        ];
        let curve = error
            * if error.abs() > 0.09 { 18.0 } else { 12.0 }
            * if self.club == 4 { 0.35 } else { 1.0 };
        self.acceleration = [-direction[1] * curve, direction[0] * curve];
        self.origin = [self.ball[0], self.ball[1]];
        self.mode = Mode::Flight;
        self.flight_time = 0.0;
        self.strokes += 1;
    }
    fn physics(&mut self, rng: &mut impl Rng) {
        let dt = 0.01;
        self.flight_time += dt;
        let air = self.ball[2] > 0.0 || self.velocity[2] > 0.0;
        for i in 0..2 {
            self.velocity[i] += self.acceleration[i] * dt;
            self.ball[i] +=
                (self.velocity[i] + if air { self.wind[i] * 0.2 } else { 0.0 }) * dt / METERS;
        }
        if air {
            self.velocity[2] -= 9.8 * dt;
            self.ball[2] += self.velocity[2] * dt;
        }
        let terrain = self.course().terrain(self.ball[0], self.ball[1]);
        let (bounce, drag, _) = properties(terrain);
        if self.ball[2] < 0.0 {
            self.ball[2] = 0.0;
            self.velocity[2] *= -bounce;
            for i in 0..2 {
                self.velocity[i] *= 1.3;
            }
            if self.velocity[2] < 4.0 {
                self.velocity[2] = 0.0;
            }
        }
        let grounded = self.ball[2] == 0.0 && self.velocity[2] == 0.0;
        for i in 0..2 {
            self.velocity[i] *= 1.0 - if grounded { drag } else { 0.36 } * dt;
            self.acceleration[i] *= 1.0 - if grounded { 0.4 } else { 0.15 } * dt;
        }
        let speed = self.velocity[0].hypot(self.velocity[1]);
        if grounded && terrain == 183 {
            if speed < 8.0 {
                self.finish_shot();
                return;
            }
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            self.velocity[0] = angle.cos() * speed * 0.6;
            self.velocity[1] = angle.sin() * speed * 0.6;
        }
        if self.ball[0] < 0.0
            || self.ball[0] >= 140.0
            || self.ball[1] < 0.0
            || self.ball[1] >= 252.0
            || self.flight_time > 15.0
            || (grounded && self.velocity[0].abs() + self.velocity[1].abs() < 2.8)
        {
            self.finish_shot();
        }
    }
    fn finish_shot(&mut self) {
        self.ball[2] = 0.0;
        if self.course().at_hole(self.ball) {
            self.card[self.hole] = self.strokes;
            self.mode = Mode::HoleDone;
            self.notice = Notice::Sunk;
            return;
        }
        let terrain = self.course().terrain(self.ball[0], self.ball[1]);
        if penalty(terrain) {
            self.strokes += 1;
            self.notice = if terrain == 131 {
                Notice::Water
            } else {
                Notice::OutOfBounds
            };
            // Search the shot segment back toward its valid origin, with a bounded iteration count.
            let end = [self.ball[0], self.ball[1]];
            let length = (end[0] - self.origin[0]).hypot(end[1] - self.origin[1]);
            let steps = length.ceil().max(1.0) as usize;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                self.ball[0] = end[0] + (self.origin[0] - end[0]) * t;
                self.ball[1] = end[1] + (self.origin[1] - end[1]) * t;
                if !penalty(self.course().terrain(self.ball[0], self.ball[1])) {
                    break;
                }
            }
        }
        self.origin = [self.ball[0], self.ball[1]];
        self.target = self.course().hole;
        self.mode = Mode::Aim;
    }
}
fn properties(terrain: u8) -> (f32, f32, f32) {
    match terrain {
        32 | 102 => (0.1, 1.0, 1.0),
        100 => (0.15, 0.5, 1.0),
        183 => (0.0, 1.0, 1.0),
        200 => (0.05, 4.0, 1.0),
        131 => (0.0, 9.0, 1.0),
        86 => (0.15, 0.7, 1.0),
        107 => (0.1, 1.5, 0.8),
        111 => (0.0, 9.0, 0.7),
        214 => (0.6, 1.0, 1.0),
        _ => (0.0, 3.0, 1.0),
    }
}
fn penalty(terrain: u8) -> bool {
    !matches!(terrain, 32 | 102 | 100 | 183 | 200 | 86 | 107 | 214)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    fn courses() -> Arc<[Course; 9]> {
        let mut pixels = vec![100; WIDTH * HEIGHT];
        pixels[180 * WIDTH + 70] = 32;
        pixels[176 * WIDTH + 70] = 183;
        Arc::new(std::array::from_fn(|_| {
            Course::from_pixels(pixels.clone()).unwrap()
        }))
    }
    #[test]
    fn metadata_requires_a_complete_course_with_tee_and_hole() {
        assert!(Course::from_pixels(vec![100; WIDTH * HEIGHT - 1]).is_none());
        assert!(Course::from_pixels(vec![100; WIDTH * HEIGHT]).is_none());
        let c = courses();
        assert_eq!(c[0].tee, [70.0, 180.0]);
        assert_eq!(c[0].hole, [70.0, 176.0]);
        for (x, y) in [(-0.1, 0.0), (140.0, 0.0), (0.0, 252.0), (f32::NAN, 1.0)] {
            assert_eq!(c[0].terrain(x, y), 0);
        }
    }
    #[test]
    fn three_click_putts_complete_all_nine_holes_and_restart_after_delay() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut game = Game::new(courses(), &mut rng);
        for hole in 0..9 {
            assert_eq!(game.hole, hole);
            game.select_club(4);
            game.primary(&mut rng);
            game.advance(Duration::from_millis(400), &mut rng);
            game.primary(&mut rng);
            game.advance(Duration::from_millis(400), &mut rng);
            game.primary(&mut rng);
            assert_eq!(game.mode, Mode::Flight);
            assert_eq!(game.strokes, 1);
            game.advance(Duration::from_secs(15), &mut rng);
            assert_eq!(game.mode, Mode::HoleDone);
            assert_eq!(game.card[hole], 1);
            game.primary(&mut rng);
        }
        assert_eq!(game.mode, Mode::Scores);
        assert_eq!(game.total(), 9);
        game.primary(&mut rng);
        assert_eq!(game.mode, Mode::Scores);
        game.advance(Duration::from_secs(3), &mut rng);
        game.primary(&mut rng);
        assert_eq!(game.total(), 0);
        assert_eq!(game.hole, 0);
    }
    #[test]
    fn automatic_swing_timeout_launches_once_and_eventually_stops() {
        let mut rng = StdRng::seed_from_u64(2);
        let mut game = Game::new(courses(), &mut rng);
        game.aim([139.0, 100.0]);
        game.primary(&mut rng);
        game.advance(Duration::from_secs(2), &mut rng);
        assert_eq!(game.mode, Mode::Flight);
        assert_eq!(game.strokes, 1);
        game.advance(Duration::from_secs(16), &mut rng);
        assert!(matches!(game.mode, Mode::Aim | Mode::HoleDone));
        assert!(game.strokes <= 2);
        assert!(game.ball.iter().all(|v| v.is_finite()));
        assert!(!penalty(game.course().terrain(game.ball[0], game.ball[1])));
    }
    #[test]
    fn hazard_and_out_of_bounds_drop_back_once_without_nan() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut game = Game::new(courses(), &mut rng);
        game.strokes = 1;
        game.ball = [-5.0, 180.0, 0.0];
        game.finish_shot();
        assert_eq!(game.strokes, 2);
        assert_eq!(game.notice, Notice::OutOfBounds);
        assert!(game.ball[0] >= 0.0);
        game.primary(&mut rng);
        assert_eq!(game.strokes, 2);
    }
    #[test]
    fn hacked_mulligan_is_once_per_hole_and_cannot_rewrite_finished_round() {
        let mut rng = StdRng::seed_from_u64(4);
        let mut game = Game::new(courses(), &mut rng);
        game.strokes = 3;
        game.mulligan(false, &mut rng);
        assert_eq!(game.strokes, 3);
        game.mulligan(true, &mut rng);
        assert_eq!(game.strokes, 0);
        assert!(game.mulligan_used);
        game.strokes = 2;
        game.mulligan(true, &mut rng);
        assert_eq!(game.strokes, 2);
        game.mode = Mode::HoleDone;
        game.primary(&mut rng);
        assert!(!game.mulligan_used);
        game.mode = Mode::Scores;
        let card = game.card;
        game.mulligan(true, &mut rng);
        assert_eq!(game.mode, Mode::Scores);
        assert_eq!(game.card, card);
    }
    #[test]
    fn fixed_steps_match_split_time_including_gusts_and_swing() {
        let mut ra = StdRng::seed_from_u64(5);
        let mut a = Game::new(courses(), &mut ra);
        let mut b = a.clone();
        let mut rb = ra.clone();
        a.primary(&mut ra);
        b.primary(&mut rb);
        a.advance(Duration::from_secs(20), &mut ra);
        for _ in 0..2000 {
            b.advance(STEP, &mut rb);
        }
        assert_eq!(a, b);
    }
    #[test]
    fn sand_wedge_avoids_the_sand_power_penalty() {
        let mut rng = StdRng::seed_from_u64(6);
        let mut game = Game::new(courses(), &mut rng);
        let mut array = (*game.courses).clone();
        array[0].pixels[180 * WIDTH + 70] = 200;
        game.courses = Arc::new(array);
        game.power = 1.0;
        game.club = 0;
        game.launch(&mut rng);
        let driver = game.velocity[1].abs();
        game.club = 3;
        game.launch(&mut rng);
        assert!((driver - 102.0 * 0.4).abs() < 0.001);
        assert!((game.velocity[1].abs() - 32.0).abs() < 0.001);
    }
}
