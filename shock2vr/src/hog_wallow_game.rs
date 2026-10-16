//! Deterministic lane-shooter simulation, independent of UI and world state.
use rand::Rng;
use std::{f32::consts::TAU, time::Duration};

const STEP: Duration = Duration::from_millis(10);
pub const SHOT_SPEED: f32 = 8.0;
pub const SHOT_LENGTH: f32 = 1.5;
const SPEEDS: [f32; 5] = [1.0, 1.3, 1.6, 1.8, 2.0];
const FLIP_SECONDS: [f32; 5] = [4.0, 3.2, 3.0, 2.5, 1.8];
pub const POINTS: [i32; 5] = [10, 20, 40, 50, 100];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Playing,
    Died,
    FinishedLevel,
    Scores,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Monster {
    pub lane: usize,
    pub kind: usize,
    pub distance: f32,
    last_flip: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub lane: usize,
    pub distance: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub mode: Mode,
    pub level: u32,
    pub score: i32,
    pub lives: u8,
    pub remaining: Duration,
    pub pause: Duration,
    pub lane: usize,
    /// Ordered points around the tunnel mouth, shared by aiming and projection.
    pub rim: Vec<[f32; 2]>,
    pub depth: f32,
    pub monsters: Vec<Monster>,
    pub shots: Vec<Shot>,
    pub bomb_used: bool,
    next_life: i32,
    debt: Duration,
    last_spawn_lane: Option<usize>,
}

impl Game {
    pub fn new(rng: &mut impl Rng) -> Self {
        let mut game = Self {
            mode: Mode::Playing,
            level: 0,
            score: 0,
            lives: 3,
            remaining: Duration::ZERO,
            pause: Duration::ZERO,
            lane: 0,
            rim: Vec::new(),
            depth: 8.0,
            monsters: Vec::new(),
            shots: Vec::new(),
            bomb_used: false,
            next_life: 5000,
            debt: Duration::ZERO,
            last_spawn_lane: None,
        };
        game.make_tunnel(rng);
        game.reset_level();
        game
    }

    fn make_tunnel(&mut self, rng: &mut impl Rng) {
        let count = (8 + self.level % 7 + self.level / 7).min(14);
        let variation = (0.1 + (self.level % 7) as f32 * 0.1).min(0.5);
        self.depth = 8.0 - (self.level % 7) as f32 * 0.25 - rng.gen_range(0.0..0.3);
        self.rim = (0..count)
            .map(|i| {
                let angle = TAU * (i as f32 + rng.gen_range(0.0..0.2)) / count as f32;
                let radius = 0.92 * (1.0 - variation * rng.gen_range(0.0..1.0));
                [angle.cos() * radius, angle.sin() * radius * 1.7]
            })
            .collect();
    }

    fn reset_level(&mut self) {
        self.mode = Mode::Playing;
        self.pause = Duration::ZERO;
        self.remaining = Duration::from_secs((12 + self.level).min(30) as u64);
        self.monsters.clear();
        self.shots.clear();
        self.lane = 0;
        self.last_spawn_lane = None;
    }

    pub fn move_lane(&mut self, step: i32) {
        if self.mode == Mode::Playing {
            self.lane = (self.lane as i32 + step).rem_euclid(self.rim.len() as i32) as usize;
        }
    }

    pub fn select_lane(&mut self, lane: usize) {
        if self.mode == Mode::Playing {
            self.lane = lane % self.rim.len();
        }
    }

    pub fn fire(&mut self) {
        if self.mode == Mode::Playing && self.shots.len() < 16 {
            self.shots.push(Shot {
                lane: self.lane,
                distance: 0.0,
            });
        }
    }

    pub fn restart(&mut self, rng: &mut impl Rng) {
        if self.mode == Mode::Scores && self.pause >= Duration::from_secs(5) {
            *self = Self::new(rng);
        }
    }

    fn add_score(&mut self, points: i32) {
        self.score += points;
        if self.score >= self.next_life {
            self.lives = (self.lives + 1).min(7);
            self.next_life = self.next_life.saturating_mul(2);
        }
    }

    pub fn bomb(&mut self, hacked: bool) {
        if !hacked || self.bomb_used || self.mode != Mode::Playing {
            return;
        }
        self.bomb_used = true;
        let points = self.monsters.iter().map(|m| POINTS[m.kind]).sum();
        self.monsters.clear();
        self.add_score(points);
    }

    pub fn advance(&mut self, elapsed: Duration, rng: &mut impl Rng) {
        self.debt += elapsed;
        while self.debt >= STEP {
            self.debt -= STEP;
            self.tick(rng);
        }
    }

    fn tick(&mut self, rng: &mut impl Rng) {
        if self.mode != Mode::Playing {
            self.pause += STEP;
            if self.pause >= Duration::from_secs(5) {
                match self.mode {
                    Mode::Died => self.reset_level(),
                    Mode::FinishedLevel => {
                        self.level += 1;
                        self.bomb_used = false;
                        self.make_tunnel(rng);
                        self.reset_level();
                    }
                    _ => {}
                }
            }
            return;
        }
        let dt = STEP.as_secs_f32();
        let speed = (1.1 + self.level as f32 * 0.02).min(2.5);
        for monster in &mut self.monsters {
            let can_flip = monster.distance >= 2.0 && monster.last_flip - monster.distance >= 2.0;
            if can_flip && rng.gen_range(0.0..1.0) < 1.0 - (-dt / FLIP_SECONDS[monster.kind]).exp()
            {
                let delta = if rng.gen_bool(0.5) { 1 } else { -1 };
                monster.lane =
                    (monster.lane as i32 + delta).rem_euclid(self.rim.len() as i32) as usize;
                monster.last_flip = monster.distance;
            } else {
                monster.distance -= SPEEDS[monster.kind] * speed * dt;
            }
        }
        for shot in &mut self.shots {
            shot.distance += SHOT_SPEED * dt;
        }
        // Resolve nearest targets first. A single projectile cannot kill a whole lane.
        self.monsters
            .sort_by(|a, b| a.distance.total_cmp(&b.distance));
        let mut points = 0;
        self.monsters.retain(|monster| {
            let height = if monster.kind == 2 { 1.7 } else { 1.0 };
            if let Some(index) = self.shots.iter().position(|shot| {
                shot.lane == monster.lane
                    && shot.distance >= monster.distance
                    && shot.distance - SHOT_LENGTH - SHOT_SPEED * dt <= monster.distance + height
            }) {
                self.shots.remove(index);
                points += POINTS[monster.kind];
                false
            } else {
                true
            }
        });
        self.add_score(points);
        self.shots
            .retain(|shot| shot.distance - SHOT_LENGTH <= self.depth);
        // Multiple simultaneous breaches consume one attempt, not every remaining life.
        if self.monsters.iter().any(|m| m.distance < 0.0) {
            self.mode = if self.lives == 0 {
                Mode::Scores
            } else {
                self.lives -= 1;
                Mode::Died
            };
            self.pause = Duration::ZERO;
            return;
        }
        self.remaining = self.remaining.saturating_sub(STEP);
        if self.remaining.is_zero() {
            self.mode = Mode::FinishedLevel;
            self.pause = Duration::ZERO;
            return;
        }
        let interval = (0.95 * 0.97_f32.powf(self.level as f32)).max(0.2);
        if self.remaining > Duration::from_secs(2)
            && self.monsters.len() < 6
            && rng.gen_range(0.0..1.0) < 1.0 - (-dt / interval).exp()
        {
            let lane = rng.gen_range(0..self.rim.len());
            if Some(lane) != self.last_spawn_lane {
                // Box-Muller provides a bounded archetype selection around the level's mean.
                let normal = (-2.0 * rng.gen_range(f32::MIN_POSITIVE..1.0).ln()).sqrt()
                    * (TAU * rng.gen_range(0.0..1.0)).cos();
                let kind =
                    (normal * 3.0 + (self.level as f32 / 8.0).min(5.0) + rng.gen_range(0.0..0.5))
                        .floor()
                        .clamp(0.0, 4.0) as usize;
                self.monsters.push(Monster {
                    lane,
                    kind,
                    distance: self.depth,
                    last_flip: self.depth + 2.0,
                });
                self.last_spawn_lane = Some(lane);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    fn monster(lane: usize, distance: f32) -> Monster {
        Monster {
            lane,
            kind: 0,
            distance,
            last_flip: distance,
        }
    }

    #[test]
    fn fixed_steps_match_batched_time_including_random_spawns() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut game = Game::new(&mut rng);
        let mut split = game.clone();
        let mut split_rng = rng.clone();
        game.advance(Duration::from_secs(8), &mut rng);
        for _ in 0..800 {
            split.advance(STEP, &mut split_rng);
        }
        assert_eq!(game, split);
        assert!(!game.monsters.is_empty());
    }

    #[test]
    fn shot_hits_nearest_target_once_and_misses_other_lanes() {
        let mut rng = StdRng::seed_from_u64(2);
        let mut game = Game::new(&mut rng);
        game.remaining = Duration::from_secs(2);
        game.monsters = vec![monster(0, 1.0), monster(0, 3.0), monster(1, 1.0)];
        game.fire();
        game.advance(Duration::from_millis(200), &mut rng);
        assert_eq!(game.score, 10);
        assert_eq!(game.monsters.len(), 2);
        assert!(game.monsters.iter().any(|m| m.lane == 1));
        assert!(game.shots.is_empty());
    }

    #[test]
    fn simultaneous_breaches_cost_one_life_and_retry_preserves_spent_bomb() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut game = Game::new(&mut rng);
        game.bomb(true);
        let rim = game.rim.clone();
        game.monsters = vec![monster(0, 0.001), monster(1, 0.001)];
        game.advance(STEP, &mut rng);
        assert_eq!((game.mode, game.lives), (Mode::Died, 2));
        game.fire();
        assert!(game.shots.is_empty());
        game.advance(Duration::from_secs(5), &mut rng);
        assert_eq!(game.mode, Mode::Playing);
        assert_eq!(game.rim, rim);
        assert!(game.bomb_used);
        assert!(game.monsters.is_empty());
        assert_eq!(game.remaining, Duration::from_secs(12));
    }

    #[test]
    fn timer_advances_level_after_five_seconds_and_renews_bomb() {
        let mut rng = StdRng::seed_from_u64(4);
        let mut game = Game::new(&mut rng);
        game.bomb(true);
        game.remaining = STEP;
        game.advance(STEP, &mut rng);
        assert_eq!(game.mode, Mode::FinishedLevel);
        game.advance(Duration::from_millis(4990), &mut rng);
        assert_eq!(game.level, 0);
        game.advance(STEP, &mut rng);
        assert_eq!(game.level, 1);
        assert_eq!(game.remaining, Duration::from_secs(13));
        assert!(!game.bomb_used);
    }

    #[test]
    fn bomb_requires_hack_and_scores_only_once_with_extra_life() {
        let mut rng = StdRng::seed_from_u64(5);
        let mut game = Game::new(&mut rng);
        game.score = 4990;
        game.monsters = vec![monster(0, 5.0), monster(1, 5.0)];
        game.bomb(false);
        assert_eq!(game.monsters.len(), 2);
        game.bomb(true);
        assert_eq!((game.score, game.lives), (5010, 4));
        assert!(game.monsters.is_empty());
        game.monsters.push(monster(0, 5.0));
        game.bomb(true);
        assert_eq!(game.monsters.len(), 1);
        assert_eq!(game.score, 5010);
    }

    #[test]
    fn game_over_and_restart_are_delayed_and_reset_campaign_run() {
        let mut rng = StdRng::seed_from_u64(6);
        let mut game = Game::new(&mut rng);
        game.lives = 0;
        game.score = 100;
        game.monsters.push(monster(0, 0.001));
        game.advance(STEP, &mut rng);
        assert_eq!(game.mode, Mode::Scores);
        game.restart(&mut rng);
        assert_eq!(game.score, 100);
        game.advance(Duration::from_secs(5), &mut rng);
        game.restart(&mut rng);
        assert_eq!((game.mode, game.lives, game.score), (Mode::Playing, 3, 0));
    }

    #[test]
    fn lane_selection_wraps_and_shot_pool_is_bounded() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut game = Game::new(&mut rng);
        game.move_lane(-1);
        assert_eq!(game.lane, game.rim.len() - 1);
        game.move_lane(1);
        assert_eq!(game.lane, 0);
        for _ in 0..20 {
            game.fire();
        }
        assert_eq!(game.shots.len(), 16);
        game.remaining = Duration::from_secs(2);
        game.advance(Duration::from_secs(2), &mut rng);
        assert!(game.shots.is_empty());
    }

    #[test]
    fn generated_tunnels_have_ordered_lanes_and_bounded_difficulty() {
        let mut rng = StdRng::seed_from_u64(8);
        let mut game = Game::new(&mut rng);
        for level in 0..100 {
            game.level = level;
            game.make_tunnel(&mut rng);
            assert!((8..=14).contains(&game.rim.len()));
            let angles: Vec<_> = game
                .rim
                .iter()
                .map(|p| (p[1] / 1.7).atan2(p[0]).rem_euclid(TAU))
                .collect();
            assert!(angles.windows(2).all(|p| p[0] < p[1]));
            assert!(game.depth > 6.0 && game.depth <= 8.0);
        }
    }
}
