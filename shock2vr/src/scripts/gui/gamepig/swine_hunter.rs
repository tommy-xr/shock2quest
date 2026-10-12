use std::time::Duration;

use cgmath::vec2;
use rand::Rng;
use shipyard::World;

use super::{GamePigGuiMsg, PanelText, sprite};
use crate::{
    gui::{self, GuiComponent, GuiCursor},
    mini_game_scores::MiniGameScore,
    scripts::Effect,
    time::Time,
    ui::Rect,
};

const FRAME: Duration = Duration::from_nanos(16_666_667);
const STEP: Duration = Duration::from_millis(50);
const RECOVERY: Duration = Duration::from_millis(2000);
type Point = [i32; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Pedestrian,
    Pig,
    Razor,
    Shooter,
}

impl Kind {
    fn points(self) -> i32 {
        match self {
            Self::Pedestrian => -1000,
            Self::Pig => 500,
            Self::Razor => 750,
            Self::Shooter => 600,
        }
    }

    fn images(self) -> [&'static str; 2] {
        match self {
            Self::Pedestrian => ["raceped1.pcx", "raceped2.pcx"],
            Self::Pig => ["racepig1.pcx", "racepig2.pcx"],
            Self::Razor => ["raceraz1.pcx", "raceraz2.pcx"],
            Self::Shooter => ["racesht1.pcx", "racesht2.pcx"],
        }
    }
}

#[derive(Clone, Debug)]
struct Traffic {
    kind: Kind,
    position: Point,
    precise: [f32; 2],
    velocity: [f32; 2],
    frame: usize,
    movement: Duration,
    animation: Duration,
    speed: Duration,
    crashed: bool,
    shot: Option<Point>,
}

impl Traffic {
    fn crash(&mut self) {
        self.crashed = true;
        self.shot = None;
        self.animation = Duration::ZERO;
    }
}

#[derive(Clone, Debug)]
pub(super) struct SwineHunter {
    road: [[i32; 2]; 90],
    curve: i32,
    car: Point,
    traffic: Vec<Traffic>,
    shot: Option<Point>,
    rear_shot: Option<Point>,
    delay: Duration,
    finished: bool,
    lives: i32,
    score: i32,
    movement: Duration,
    scoring: Duration,
    rank: Option<usize>,
    last_frame: Option<Duration>,
    simulation_debt: Duration,
    pub stick: Option<[f32; 2]>,
}

impl Default for SwineHunter {
    fn default() -> Self {
        let mut rng = rand::thread_rng();
        let mut game = Self {
            road: [[0; 2]; 90],
            curve: 0,
            car: [67, 54],
            traffic: Vec::new(),
            shot: None,
            rear_shot: None,
            delay: Duration::ZERO,
            finished: false,
            lives: 3,
            score: 0,
            movement: Duration::ZERO,
            scoring: Duration::ZERO,
            rank: None,
            last_frame: None,
            simulation_debt: Duration::ZERO,
            stick: None,
        };
        game.road[0] = [rng.gen_range(0..33), 135 - rng.gen_range(0..33)];
        for row in 1..90 {
            game.build_road(row, &mut rng);
        }
        game
    }
}

impl SwineHunter {
    fn build_road(&mut self, row: usize, rng: &mut impl Rng) {
        let [left, right] = self.road[row - 1];
        let center = (left + right) / 2;
        self.curve += match rng.gen_range(0..100) {
            0..=7 => -2,
            8..=24 => -1,
            93..=99 => 2,
            76..=92 => 1,
            _ => 0,
        };
        self.curve += i32::from(center < 45) - i32::from(center > 90);
        self.curve = self.curve.clamp(-2, 2);
        let roll = rng.gen_range(0..100);
        let width = right - left;
        let change = if width == 110 {
            i32::from(roll > 75) - i32::from(roll < 25)
        } else {
            (110 - width).signum() * (i32::from(roll > 50) + i32::from(roll > 75))
        };
        let half = (width + change) / 2;
        self.road[row] = [
            (center + self.curve - half).max(4),
            (center + self.curve + half).min(131),
        ];
    }

    pub fn primary(&mut self) {
        if self.finished {
            let last_frame = self.last_frame;
            *self = Self::default();
            self.last_frame = last_frame;
        } else if self.delay.is_zero() && self.shot.is_none() {
            self.shot = Some([self.car[0], self.car[1] + 5]);
        }
    }

    pub fn secondary(&mut self, hacked: bool) {
        if hacked && !self.finished && self.delay.is_zero() && self.rear_shot.is_none() {
            self.rear_shot = Some([self.car[0], self.car[1] - 5]);
        }
    }

    fn crash(&mut self) {
        // Simultaneous wall and traffic impacts consume one life, not several.
        if self.finished || !self.delay.is_zero() {
            return;
        }
        self.delay = RECOVERY;
        self.lives -= 1;
        if self.lives < 0 {
            self.lives = 0;
            self.finished = true;
        }
    }

    fn spawn(&mut self, kind: Kind, rng: &mut impl Rng) {
        let top = match kind {
            Kind::Pedestrian => true,
            Kind::Shooter => false,
            _ => rng.gen_range(0..100) > 50,
        };
        let y = if top { 266 } else { 5 };
        let [left, right] = self.road[y / 3];
        let position = [rng.gen_range(left + 5..right - 5), y as i32];
        self.traffic.push(Traffic {
            kind,
            position,
            precise: position.map(|value| value as f32),
            velocity: [0.0, if top { -1.0 } else { 1.0 }],
            frame: if matches!(kind, Kind::Pedestrian | Kind::Shooter) {
                rng.gen_range(0..2)
            } else {
                0
            },
            movement: Duration::ZERO,
            animation: Duration::ZERO,
            speed: Duration::from_millis(rng.gen_range(19..35)),
            crashed: false,
            shot: None,
        });
    }

    fn steer_traffic(&mut self, index: usize, rng: &mut impl Rng) {
        let position = self.traffic[index].position;
        let seeking = self.traffic[index].kind == Kind::Razor;
        let nearest = self
            .traffic
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, other)| other.position)
            .chain((!seeking).then_some(self.car))
            .map(|other| {
                let dx = (position[0] - other[0]) as f32;
                let dy = (position[1] - other[1]) as f32;
                ((dx * dx + dy * dy).sqrt(), other[0])
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let car = &mut self.traffic[index];
        if seeking && rng.gen_range(0..100) < 10 {
            car.velocity[1] += if self.car[1] > position[1] { 0.2 } else { -0.2 };
        }
        let [left, right] = self.road[position[1] as usize / 3];
        let left_distance = position[0] - left;
        let right_distance = right - position[0];
        if left_distance < 20 {
            car.velocity[0] += 2.0 - left_distance as f32 / 10.0;
        }
        if right_distance < 20 {
            car.velocity[0] -= 2.0 - right_distance as f32 / 10.0;
        }
        if rng.gen_range(0..100) < 10 {
            car.velocity[0] -= 0.1;
        }
        if car.kind == Kind::Shooter {
            car.velocity[0] += if car.frame == 0 { -0.1 } else { 0.1 };
            if car.shot.is_none() && (position[1] - self.car[1]).abs() < 3 {
                car.shot = Some(position);
            }
        }
        if seeking && rng.gen_range(0..100) < 10 {
            car.velocity[0] += if position[0] > self.car[0] { -0.2 } else { 0.2 };
        }
        if let Some((distance, x)) = nearest.filter(|(distance, _)| *distance < 15.0) {
            car.velocity[0] += (1.5 - distance / 10.0) * if position[0] > x { 1.0 } else { -1.0 };
        }
        for axis in 0..2 {
            car.velocity[axis] = car.velocity[axis].clamp(-1.0, 1.0);
            car.precise[axis] += car.velocity[axis];
            car.position[axis] = car.precise[axis] as i32;
        }
    }

    fn move_traffic(&mut self, elapsed: Duration, rng: &mut impl Rng) {
        let mut index = 0;
        while index < self.traffic.len() {
            let car = &mut self.traffic[index];
            if let Some(shot) = &mut car.shot {
                shot[0] += if car.frame == 0 { 4 } else { -4 };
            }
            if car.crashed {
                car.animation += elapsed;
                if car.animation > RECOVERY {
                    self.traffic.remove(index);
                    continue;
                }
            } else {
                if matches!(car.kind, Kind::Pig | Kind::Razor) {
                    car.animation += elapsed;
                    while car.animation > Duration::from_millis(200) {
                        car.animation -= Duration::from_millis(200);
                        car.frame ^= 1;
                    }
                }
                car.movement += elapsed;
                let mut removed = false;
                while self.traffic[index].movement > self.traffic[index].speed {
                    let speed = self.traffic[index].speed;
                    self.traffic[index].movement -= speed;
                    self.steer_traffic(index, rng);
                    if !(5..=266).contains(&self.traffic[index].position[1]) {
                        self.traffic.remove(index);
                        removed = true;
                        break;
                    }
                }
                if removed {
                    continue;
                }
            }
            index += 1;
        }
    }

    fn road_step(&mut self, movement: Point, rng: &mut impl Rng) {
        self.road.copy_within(1..90, 0);
        self.build_road(89, rng);
        self.car[0] = (self.car[0] + movement[0]).clamp(3, 132);
        self.car[1] = (self.car[1] + movement[1]).clamp(67, 203);
        if let Some(shot) = &mut self.shot {
            shot[1] += 8;
        }
        if let Some(shot) = &mut self.rear_shot {
            shot[1] -= 8;
        }
        self.traffic.retain_mut(|car| {
            if car.crashed {
                car.position[1] -= 3;
            }
            car.position[1] >= 5
        });
        let chance = (self.score / 5000 + 2 + i32::from(self.traffic.is_empty())
            - self.traffic.len() as i32)
            .clamp(0, 10);
        if rng.gen_range(0..100) < chance {
            let kind = match rng.gen_range(0..100) {
                0..=4 => Kind::Shooter,
                5..=14 => Kind::Razor,
                15..=64 => Kind::Pig,
                _ => Kind::Pedestrian,
            };
            self.spawn(kind, rng);
        }
        self.scoring += STEP;
        while self.scoring > Duration::from_secs(1) {
            self.scoring -= Duration::from_secs(1);
            self.score += 5;
        }
    }

    fn hits_wall(&self, position: Point, margin: i32) -> bool {
        let row = (position[1] / 3).clamp(0, 89) as usize;
        position[0] - margin <= self.road[row][0] || position[0] + margin >= self.road[row][1]
    }

    fn collisions(&mut self) {
        for index in 0..self.traffic.len() {
            if let Some(shot) = self.traffic[index].shot {
                if self.hits_wall(shot, 3) {
                    self.traffic[index].shot = None;
                }
                if near(shot, self.car, 5) {
                    self.crash();
                }
            }
            if self.traffic[index].crashed {
                continue;
            }
            let position = self.traffic[index].position;
            if self.hits_wall(position, 2) {
                self.traffic[index].crash();
                self.score += self.traffic[index].kind.points().max(0);
                continue;
            }
            if self.traffic[index].kind == Kind::Razor {
                for other in 0..self.traffic.len() {
                    if other != index
                        && !self.traffic[other].crashed
                        && self.traffic[other].kind != Kind::Pig
                        && near(position, self.traffic[other].position, 12)
                    {
                        self.traffic[other].crash();
                        self.score += Kind::Razor.points();
                    }
                }
            }
            for shot in [&mut self.shot, &mut self.rear_shot] {
                if shot.is_some_and(|shot| near(position, shot, 6)) {
                    if !self.traffic[index].crashed {
                        self.traffic[index].crash();
                        self.score += self.traffic[index].kind.points();
                    }
                    *shot = None;
                }
            }
        }
        for index in 0..self.traffic.len() {
            let car = &mut self.traffic[index];
            if car.crashed || !near(self.car, car.position, 11) {
                continue;
            }
            if matches!(car.kind, Kind::Pedestrian | Kind::Shooter) {
                for axis in 0..2 {
                    let direction = if self.car[axis] > car.position[axis] {
                        1
                    } else {
                        -1
                    };
                    self.car[axis] += direction;
                    car.velocity[axis] -= direction as f32;
                }
            } else {
                self.crash();
                break;
            }
        }
        if self
            .shot
            .is_some_and(|shot| shot[1] >= 266 || self.hits_wall(shot, 3))
        {
            self.shot = None;
        }
        if self
            .rear_shot
            .is_some_and(|shot| shot[1] <= 5 || self.hits_wall(shot, 3))
        {
            self.rear_shot = None;
        }
        if self.hits_wall(self.car, 3) {
            self.crash();
        }
        self.score = self.score.max(0);
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
        let stick = self.stick.take().unwrap_or([0.0; 2]);
        if self.finished {
            return Effect::NoEffect;
        }
        // The authored per-frame shot and bounce steps run at the same rate on
        // every display, with collision checks between movement steps.
        self.simulation_debt += time.elapsed;
        let mut rng = rand::thread_rng();
        while self.simulation_debt >= FRAME && !self.finished {
            self.simulation_debt -= FRAME;
            self.tick(cursor, controller, stick, &mut rng);
        }
        if self.finished {
            self.rank = super::score_rank(world, 9, self.score);
            let format = PanelText::string(world, "minigame", "rc_score_form", "%d Points");
            return Effect::RecordMiniGameScore {
                game: 9,
                score: MiniGameScore {
                    points: self.score,
                    text: PanelText::format(&format, &[self.score]),
                },
            };
        }
        Effect::NoEffect
    }

    fn tick(
        &mut self,
        cursor: Option<&GuiCursor>,
        controller: bool,
        stick: [f32; 2],
        rng: &mut impl Rng,
    ) {
        if !self.delay.is_zero() {
            self.delay = self.delay.saturating_sub(FRAME);
            if self.delay.is_zero() {
                self.shot = None;
                self.rear_shot = None;
                self.traffic.clear();
                let [left, right] = self.road[(self.car[1] / 3).clamp(0, 89) as usize];
                self.car[0] = (left + right) / 2;
            }
            return;
        }
        let movement = if controller {
            stick.map(|axis| {
                if axis.abs() < 0.15 {
                    0
                } else {
                    axis.signum() as i32 * if axis.abs() > 0.6 { 2 } else { 1 }
                }
            })
        } else {
            cursor
                .filter(|cursor| cursor.position.x >= 0.0)
                .map_or([0; 2], |cursor| {
                    let target = [
                        cursor.position.x as i32 - 13,
                        271 - (cursor.position.y as i32 - 11),
                    ];
                    [0, 1].map(|axis| {
                        let distance = target[axis] - self.car[axis];
                        if distance.abs() <= 4 {
                            0
                        } else {
                            distance.signum() * if distance.abs() > 20 { 2 } else { 1 }
                        }
                    })
                })
        };
        self.move_traffic(FRAME, rng);
        self.movement += FRAME;
        while self.movement > STEP {
            self.movement -= STEP;
            self.road_step(movement, rng);
        }
        self.collisions();
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut components = vec![
            gui::image("raceback.pcx")
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0)),
        ];
        if self.finished {
            components.push(PanelText::text(
                &PanelText::string(world, "minigame", "sh_gameover", "Game Over"),
                Rect::new(53.0, 51.0, 100.0, 15.0),
            ));
            components.extend(super::score_table(
                world,
                9,
                self.rank,
                Rect::new(30.0, 61.0, 122.0, 198.0),
            ));
        } else {
            for (row, walls) in self.road.iter().enumerate() {
                for &x in walls {
                    components.push(sprite("racewall.pcx", x, 271 - row as i32 * 3, 4.0, 4.0));
                }
            }
            let (name, size) = if self.delay.is_zero() {
                ("racecar.pcx", 12.0)
            } else {
                ("racecrsh.pcx", 8.0)
            };
            components.push(sprite(name, self.car[0], 271 - self.car[1], size, size));
            for shot in [self.shot, self.rear_shot].into_iter().flatten() {
                components.push(sprite("raceshot.pcx", shot[0], 271 - shot[1], 2.0, 4.0));
            }
            for car in &self.traffic {
                let (name, size) = if car.crashed {
                    ("racecrsh.pcx", 8.0)
                } else {
                    (car.kind.images()[car.frame], 16.0)
                };
                components.push(sprite(
                    name,
                    car.position[0],
                    271 - car.position[1],
                    size,
                    size,
                ));
                if !car.crashed {
                    if let Some(shot) = car.shot {
                        components.push(sprite("racessht.pcx", shot[0], 271 - shot[1], 4.0, 2.0));
                    }
                }
            }
            let score = PanelText::string(world, "minigame", "sh_score", "Score:");
            let lives = PanelText::string(world, "minigame", "sh_lives", "Lives:");
            components.push(PanelText::text(
                &format!("{score} {}", self.score),
                Rect::new(17.0, 13.0, 86.0, 15.0),
            ));
            components.push(PanelText::text(
                &format!("{lives} {}", self.lives),
                Rect::new(103.0, 13.0, 50.0, 15.0),
            ));
        }
        components.push(
            gui::button(GamePigGuiMsg::HunterPrimary)
                .with_rect(Rect::new(13.0, 11.0, 140.0, 276.0))
                .with_alpha(0.0)
                .with_label(if self.finished {
                    "hunter-restart"
                } else {
                    "hunter-play"
                }),
        );
        components
    }
}

fn near(a: Point, b: Point, distance: i32) -> bool {
    (a[0] - b[0]).abs() <= distance && (a[1] - b[1]).abs() <= distance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;
    use rand::{SeedableRng, rngs::StdRng};

    fn game() -> SwineHunter {
        SwineHunter {
            road: [[4, 131]; 90],
            car: [67, 100],
            ..SwineHunter::default()
        }
    }

    fn world() -> World {
        let world = World::new();
        world.add_unique(QuestInfo::new());
        world
    }

    fn traffic(kind: Kind, position: Point) -> Traffic {
        Traffic {
            kind,
            position,
            precise: position.map(|x| x as f32),
            velocity: [0.0; 2],
            frame: 0,
            movement: Duration::ZERO,
            animation: Duration::ZERO,
            speed: Duration::from_millis(30),
            crashed: false,
            shot: None,
        }
    }

    fn time(total: u64, elapsed: u64) -> Time {
        Time {
            total: Duration::from_millis(total),
            elapsed: Duration::from_millis(elapsed),
        }
    }

    #[test]
    fn shots_require_hacking_for_rear_fire_and_rearm_after_the_boundary() {
        let mut game = game();
        game.secondary(false);
        assert!(game.rear_shot.is_none());
        game.primary();
        game.secondary(true);
        assert_eq!(game.shot, Some([67, 105]));
        assert_eq!(game.rear_shot, Some([67, 95]));
        game.car[0] = 70;
        game.primary();
        game.secondary(true);
        assert_eq!(game.shot, Some([67, 105]));
        assert_eq!(game.rear_shot, Some([67, 95]));
        game.shot = Some([67, 266]);
        game.rear_shot = Some([67, 5]);
        game.collisions();
        assert!(game.shot.is_none() && game.rear_shot.is_none());
        game.primary();
        assert_eq!(game.shot, Some([70, 105]));
        game.crash();
        game.rear_shot = None;
        game.secondary(true);
        assert!(game.rear_shot.is_none());
    }

    #[test]
    fn one_crash_consumes_one_life_and_recovery_clears_traffic_and_shots() {
        let mut game = game();
        let world = world();
        game.primary();
        game.traffic.push(traffic(Kind::Pig, [50, 150]));
        game.crash();
        game.crash();
        assert_eq!(game.lives, 2);
        game.stick = Some([1.0, 1.0]);
        game.advance(&time(1000, 1000), None, true, &world);
        assert_eq!(game.car, [67, 100]);
        assert!(game.delay >= Duration::from_secs(1) && game.delay < Duration::from_millis(1020));
        game.advance(&time(2001, 1001), None, true, &world);
        assert!(game.traffic.is_empty() && game.shot.is_none());
        assert_eq!(game.car, [67, 100]);
        game.advance(&time(2052, 51), None, true, &world);
        assert_eq!(
            game.car,
            [67, 100],
            "old stick input cannot survive recovery"
        );
        for lives in [1, 0] {
            game.crash();
            assert_eq!(game.lives, lives);
            assert!(!game.finished);
            game.delay = Duration::ZERO;
        }
        game.crash();
        assert!(game.finished);
        assert_eq!(game.lives, 0);
    }

    #[test]
    fn traffic_collision_rules_distinguish_bounces_razors_pigs_and_pedestrians() {
        let mut game = game();
        game.traffic.push(traffic(Kind::Pedestrian, [66, 99]));
        game.collisions();
        assert_eq!(game.car, [68, 101]);
        assert_eq!(game.traffic[0].velocity, [-1.0; 2]);
        assert!(game.delay.is_zero());
        game.traffic = vec![
            traffic(Kind::Razor, [50, 160]),
            traffic(Kind::Shooter, [55, 165]),
            traffic(Kind::Pig, [48, 158]),
        ];
        game.collisions();
        assert!(game.traffic[1].crashed);
        assert!(!game.traffic[2].crashed);
        assert_eq!(game.score, 750);
        game.traffic = vec![traffic(Kind::Pedestrian, [67, 150])];
        game.shot = Some([67, 150]);
        game.collisions();
        assert_eq!(
            game.score, 0,
            "pedestrian penalty cannot make score negative"
        );
        game.traffic = vec![traffic(Kind::Pig, game.car)];
        game.collisions();
        assert_eq!(game.lives, 2);
    }

    #[test]
    fn simultaneous_wall_and_shot_impacts_award_a_traffic_crash_once() {
        let mut game = game();
        game.traffic.push(traffic(Kind::Pig, [6, 160]));
        game.shot = Some([6, 160]);
        game.rear_shot = Some([6, 160]);
        game.collisions();
        assert!(game.traffic[0].crashed);
        assert_eq!(game.score, 500);
    }

    #[test]
    fn lateral_projectile_distance_is_independent_of_render_frame_partition() {
        let mut whole = game();
        let world = world();
        let mut shooter = traffic(Kind::Shooter, [30, 200]);
        shooter.speed = Duration::from_secs(100);
        shooter.shot = Some([40, 200]);
        whole.traffic.push(shooter);
        let mut split = whole.clone();
        whole.advance(&time(100, 100), None, true, &world);
        for frame in 1..=10 {
            split.advance(&time(frame * 10, 10), None, true, &world);
        }
        assert_eq!(whole.traffic[0].shot, split.traffic[0].shot);
        assert!(whole.traffic[0].shot.unwrap()[0] >= 60);
    }

    #[test]
    fn long_frame_cannot_skip_a_target_between_forward_shot_steps() {
        let mut game = game();
        let world = world();
        game.car = [67, 67];
        let mut pig = traffic(Kind::Pig, [67, 116]);
        pig.speed = Duration::from_secs(100);
        game.traffic.push(pig);
        game.shot = Some([67, 100]);
        game.advance(&time(200, 200), None, true, &world);
        assert!(game.traffic[0].crashed);
        assert_eq!(game.score, 500);
        assert!(game.shot.is_none());
    }

    #[test]
    fn crashed_shooter_cannot_leave_an_invisible_damaging_projectile() {
        let mut game = game();
        let mut shooter = traffic(Kind::Shooter, [30, 160]);
        shooter.shot = Some(game.car);
        shooter.crash();
        game.traffic.push(shooter);
        game.collisions();
        assert!(game.traffic[0].shot.is_none());
        assert!(game.delay.is_zero());
    }

    #[test]
    fn steering_consumes_both_axes_and_deduplicates_visible_frames() {
        let mut game = game();
        let world = world();
        game.stick = Some([-1.0, 1.0]);
        game.advance(&time(67, 67), None, true, &world);
        assert_eq!(game.car, [65, 102]);
        let road = game.road;
        game.advance(&time(67, 67), None, true, &world);
        assert_eq!(game.road, road);
        game.advance(&time(134, 67), None, true, &world);
        assert_eq!(game.car, [65, 102]);
        let cursor = GuiCursor {
            position: cgmath::point2(133.0, 211.0),
            held_entity_id: None,
        };
        game.advance(&time(201, 67), Some(&cursor), false, &world);
        assert!(game.car[0] > 65 && game.car[1] < 102);
    }

    #[test]
    fn road_bounds_tick_debt_score_clock_and_traffic_ai_are_preserved() {
        let mut game = game();
        let mut rng = StdRng::seed_from_u64(29);
        for _ in 0..20 {
            game.road_step([0, 0], &mut rng);
        }
        assert_eq!(game.score, 0);
        game.road_step([0, 0], &mut rng);
        assert_eq!(game.score, 5);
        for _ in 0..5000 {
            game.road.copy_within(1..90, 0);
            game.build_road(89, &mut rng);
        }
        assert!(
            game.road
                .iter()
                .all(|[left, right]| *left >= 4 && *right <= 131 && right - left > 10)
        );
        game.road = [[4, 131]; 90];
        game.traffic = vec![traffic(Kind::Shooter, [45, 100])];
        game.steer_traffic(0, &mut rng);
        assert_eq!(game.traffic[0].shot, Some([45, 100]));
        game.move_traffic(Duration::from_millis(1), &mut rng);
        assert_eq!(game.traffic[0].shot, Some([49, 100]));
        game.traffic = vec![traffic(Kind::Pig, [70, 160])];
        game.move_traffic(Duration::from_millis(201), &mut rng);
        assert_eq!(game.traffic[0].frame, 1);
        assert!(
            game.traffic[0]
                .velocity
                .iter()
                .all(|speed| speed.abs() <= 1.0)
        );
    }

    #[test]
    fn final_crash_records_once_and_restart_preserves_the_frame_guard() {
        let mut game = game();
        let world = world();
        game.lives = 0;
        game.score = 700;
        game.car[0] = 5;
        assert!(matches!(
            game.advance(&time(5000, 5000), None, true, &world),
            Effect::RecordMiniGameScore {
                game: 9,
                score: MiniGameScore { points: 700, .. }
            }
        ));
        assert!(matches!(
            game.advance(&time(5017, 17), None, true, &world),
            Effect::NoEffect
        ));
        assert!(game.components(&world).iter().all(
            |component| !matches!(component, GuiComponent::Text { text, .. } if text.is_empty())
        ));
        game.primary();
        assert_eq!(game.last_frame, Some(Duration::from_millis(5017)));
        assert_eq!(game.score, 0);
        assert_eq!(game.simulation_debt, Duration::ZERO);
        assert_eq!(game.lives, 3);
        assert!(!game.finished);
    }
}
