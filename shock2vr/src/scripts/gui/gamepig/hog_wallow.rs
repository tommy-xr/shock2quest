//! Project the lane shooter into the shared handheld canvas.
use super::{GamePigGuiMsg, score_rank, score_table};
use crate::{
    gui::{self, GuiComponent, GuiCursor},
    handheld_input::HandheldControl,
    hog_wallow_game::{Game, Mode, SHOT_LENGTH},
    mini_game_scores::MiniGameScore,
    scripts::{Effect, gui::PanelText},
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use shipyard::World;
use std::{f32::consts::TAU, time::Duration};

#[derive(Clone, Debug)]
pub(super) struct HogWallow {
    game: Game,
    last_frame: Option<Duration>,
    recorded: bool,
    rank: Option<usize>,
    pointer_primary: bool,
}

impl Default for HogWallow {
    fn default() -> Self {
        Self {
            game: Game::new(&mut rand::thread_rng()),
            last_frame: None,
            recorded: false,
            rank: None,
            pointer_primary: false,
        }
    }
}

impl HogWallow {
    fn aim(&mut self, x: f32, y: f32) {
        if x * x + y * y < 0.01 {
            return;
        }
        let angle = y.atan2(x).rem_euclid(TAU);
        let lane = self
            .game
            .rim
            .iter()
            .rposition(|p| p[1].atan2(p[0]).rem_euclid(TAU) <= angle)
            .unwrap_or(self.game.rim.len() - 1);
        self.game.select_lane(lane);
    }

    pub fn controller(&mut self, control: HandheldControl, hacked: bool) {
        match control {
            HandheldControl::Stick { x, y } => self.aim(x, -y),
            HandheldControl::Primary => self.primary(),
            HandheldControl::Secondary => self.game.bomb(hacked),
            _ => {}
        }
    }

    pub fn primary(&mut self) {
        if self.game.mode == Mode::Scores {
            self.game.restart(&mut rand::thread_rng());
            if self.game.mode == Mode::Playing {
                self.recorded = false;
                self.rank = None;
            }
        } else {
            self.game.fire();
        }
    }

    pub fn bomb(&mut self, hacked: bool) {
        self.game.bomb(hacked);
    }

    pub fn pointer_primary(&mut self) {
        self.pointer_primary = true;
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
        if !controller {
            if let Some(cursor) = cursor.filter(|c| {
                (13.0..153.0).contains(&c.position.x) && (11.0..287.0).contains(&c.position.y)
            }) {
                self.aim(
                    (cursor.position.x - 83.0) / 68.0,
                    (cursor.position.y - 149.0) / 68.0,
                );
            }
        }
        // Hover messages arrive before this frame's cursor; aim before firing.
        if std::mem::take(&mut self.pointer_primary) {
            self.primary();
        }
        self.game.advance(time.elapsed, &mut rand::thread_rng());
        if self.game.mode == Mode::Scores && !self.recorded {
            self.recorded = true;
            self.rank = score_rank(world, 6, self.game.score);
            let format =
                PanelText::string(world, "minigame", "ab_score_form", "%d points, level %d");
            return Effect::RecordMiniGameScore {
                game: 6,
                score: MiniGameScore {
                    points: self.game.score,
                    text: PanelText::format(
                        &format,
                        &[self.game.score, self.game.level as i32 + 1],
                    ),
                },
            };
        }
        Effect::NoEffect
    }

    pub fn components(&self, world: &World, hacked: bool) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut result = vec![GuiComponent::Fill {
            position: vec2(13.0, 11.0),
            size: vec2(140.0, 276.0),
            color: [0, 0, 0],
            alpha: 1.0,
        }];
        let mut raster = Raster::new();
        let pause = self.game.pause.as_secs_f32();
        let camera = match self.game.mode {
            Mode::Died => -1.0 - pause * 8.0,
            Mode::FinishedLevel => -1.0 + pause * 5.0,
            _ => -1.0,
        };
        let web_color = if matches!(self.game.mode, Mode::Died | Mode::Scores) {
            [255, 0, 0]
        } else {
            [255, 255, 255]
        };
        if self.game.mode != Mode::Scores {
            for i in 0..self.game.rim.len() {
                let a = self.game.rim[i];
                let b = self.game.rim[(i + 1) % self.game.rim.len()];
                raster.segment(
                    [a[0], a[1], 0.0],
                    [a[0], a[1], self.game.depth],
                    camera,
                    web_color,
                );
                for depth in [0.0, self.game.depth] {
                    raster.segment([a[0], a[1], depth], [b[0], b[1], depth], camera, web_color);
                }
            }
            self.draw_actor(&mut raster, self.game.lane, 5, -0.21, camera);
            for monster in &self.game.monsters {
                self.draw_actor(
                    &mut raster,
                    monster.lane,
                    monster.kind,
                    monster.distance,
                    camera,
                );
            }
            for shot in &self.game.shots {
                let a = self.game.rim[shot.lane];
                let b = self.game.rim[(shot.lane + 1) % self.game.rim.len()];
                let [x, y] = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                raster.segment(
                    [x, y, (shot.distance - SHOT_LENGTH).max(0.0)],
                    [x, y, shot.distance.min(self.game.depth)],
                    camera,
                    [32, 96, 255],
                );
            }
            result.extend(raster.components());
        } else {
            result.push(PanelText::text(
                &PanelText::string(world, "minigame", "ab_gameover", "Game Over"),
                Rect::new(48.0, 30.0, 104.0, 20.0),
            ));
            result.extend(score_table(
                world,
                6,
                self.rank,
                Rect::new(21.0, 77.0, 130.0, 165.0),
            ));
        }
        result.push(PanelText::text(
            &format!(
                "L{}  {:.1}s  {}",
                self.game.level + 1,
                self.game.remaining.as_secs_f32(),
                self.game.score
            ),
            Rect::new(17.0, 13.0, 132.0, 14.0),
        ));
        result.push(PanelText::text(
            &format!(
                "Lives:{}  Bomb:{}",
                self.game.lives,
                if self.game.bomb_used || !hacked {
                    "--"
                } else {
                    "1"
                }
            ),
            Rect::new(17.0, 252.0, 132.0, 14.0),
        ));
        let label = if self.game.mode == Mode::Scores && self.game.pause >= Duration::from_secs(5) {
            "wallow-restart"
        } else {
            "wallow-fire"
        };
        result.push(
            gui::button(GamePigGuiMsg::WallowFire)
                .with_rect(Rect::new(13.0, 11.0, 140.0, 276.0))
                .with_label(label)
                .with_alpha(0.0),
        );
        result
    }

    fn draw_actor(
        &self,
        raster: &mut Raster,
        lane: usize,
        kind: usize,
        distance: f32,
        camera: f32,
    ) {
        let (vertices, edges) = SHAPES[kind];
        let a = self.game.rim[lane];
        let b = self.game.rim[(lane + 1) % self.game.rim.len()];
        let height = if kind == 5 {
            0.2
        } else if kind == 2 {
            1.7
        } else {
            1.0
        };
        let point = |index: usize| {
            let v = vertices[index];
            [
                a[0] + (b[0] - a[0]) * v[0],
                a[1] + (b[1] - a[1]) * v[0],
                distance + height * (1.0 - v[1]),
            ]
        };
        for &[a, b, color] in edges {
            raster.segment(point(a), point(b), camera, COLORS[color]);
        }
    }
}

const COLORS: [[u8; 3]; 7] = [
    [0, 255, 0],
    [128, 128, 255],
    [255, 255, 0],
    [255, 0, 255],
    [255, 0, 0],
    [255, 128, 128],
    [255, 255, 255],
];
type Shape = (&'static [[f32; 2]], &'static [[usize; 3]]);
const SHAPES: [Shape; 6] = [
    (
        &[[0., 0.], [1., 1.], [1., 0.], [0., 1.]],
        &[[0, 1, 0], [1, 2, 0], [2, 3, 0], [3, 0, 0]],
    ),
    (
        &[
            [0., 0.],
            [0.25, 0.25],
            [0., 1.],
            [0.25, 0.75],
            [1., 1.],
            [0.75, 0.75],
            [1., 0.],
            [0.75, 0.25],
            [0.5, 0.1],
            [0.1, 0.5],
            [0.5, 0.9],
            [0.9, 0.5],
        ],
        &[
            [0, 1, 1],
            [2, 3, 1],
            [4, 5, 1],
            [6, 7, 1],
            [8, 9, 0],
            [9, 10, 0],
            [10, 11, 0],
            [11, 8, 0],
        ],
    ),
    (
        &[
            [0.6, 0.1],
            [0.3, 0.2],
            [0.7, 0.5],
            [0.2, 0.6],
            [0.5, 0.7],
            [1., 0.7],
            [0., 0.7],
            [0.5, 1.],
        ],
        &[
            [0, 1, 2],
            [1, 2, 3],
            [2, 3, 3],
            [3, 4, 3],
            [5, 6, 4],
            [6, 7, 4],
            [7, 5, 4],
        ],
    ),
    (
        &[
            [0.2, 0.7],
            [0.1, 0.8],
            [0.2, 0.9],
            [0.3, 0.8],
            [0.8, 0.7],
            [0.7, 0.8],
            [0.8, 0.9],
            [0.9, 0.8],
            [0., 0.],
            [0.5, 0.4],
            [1., 0.],
        ],
        &[
            [0, 1, 2],
            [1, 2, 2],
            [2, 3, 2],
            [3, 0, 2],
            [4, 5, 2],
            [5, 6, 2],
            [6, 7, 2],
            [7, 4, 2],
            [0, 9, 4],
            [4, 9, 4],
            [8, 9, 4],
            [9, 10, 4],
            [10, 8, 4],
        ],
    ),
    (
        &[
            [0.5, 0.],
            [0., 0.3],
            [0., 0.8],
            [0.5, 0.5],
            [1., 0.8],
            [1., 0.3],
            [0., 1.],
            [1., 1.],
            [0.5, 0.9],
        ],
        &[
            [0, 1, 5],
            [2, 3, 5],
            [3, 4, 5],
            [5, 0, 5],
            [1, 6, 5],
            [0, 8, 2],
            [5, 4, 5],
        ],
    ),
    (
        &[
            [0., 0.],
            [0.4, 1.],
            [0.6, 1.],
            [1., 0.],
            [0.55, 0.7],
            [0.5, 0.1],
            [0.45, 0.7],
        ],
        &[
            [0, 1, 2],
            [1, 2, 2],
            [2, 3, 2],
            [3, 4, 2],
            [4, 5, 2],
            [5, 6, 2],
            [6, 0, 2],
        ],
    ),
];

// Resolve projected lines into disjoint pixel runs. Both presenters see one flat
// image, so crossing wires cannot accumulate depth above the handheld's glass.
struct Raster {
    pixels: Vec<[u8; 3]>,
}
impl Raster {
    fn new() -> Self {
        Self {
            pixels: vec![[0, 0, 0]; 140 * 276],
        }
    }
    fn segment(&mut self, mut a: [f32; 3], mut b: [f32; 3], camera: f32, color: [u8; 3]) {
        a[2] -= camera;
        b[2] -= camera;
        if a[2] < 0.05 && b[2] < 0.05 {
            return;
        }
        if a[2] < 0.05 {
            let t = (0.05 - a[2]) / (b[2] - a[2]);
            for i in 0..3 {
                a[i] += (b[i] - a[i]) * t;
            }
        }
        if b[2] < 0.05 {
            let t = (0.05 - b[2]) / (a[2] - b[2]);
            for i in 0..3 {
                b[i] += (a[i] - b[i]) * t;
            }
        }
        let a = [70.0 + 68.0 * a[0] / a[2], 138.0 + 68.0 * a[1] / a[2]];
        let b = [70.0 + 68.0 * b[0] / b[2], 138.0 + 68.0 * b[1] / b[2]];
        let d = [b[0] - a[0], b[1] - a[1]];
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        for (p, q) in [
            (-d[0], a[0]),
            (d[0], 139.0 - a[0]),
            (-d[1], a[1] - 16.0),
            (d[1], 238.0 - a[1]),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return;
                }
            } else if p < 0.0 {
                low = low.max(q / p);
            } else {
                high = high.min(q / p);
            }
        }
        if low > high {
            return;
        }
        let start = [a[0] + d[0] * low, a[1] + d[1] * low];
        let end = [a[0] + d[0] * high, a[1] + d[1] * high];
        let steps = (end[0] - start[0])
            .abs()
            .max((end[1] - start[1]).abs())
            .ceil() as usize;
        for i in 0..=steps {
            let t = i as f32 / steps.max(1) as f32;
            let x = (start[0] + (end[0] - start[0]) * t)
                .round()
                .clamp(0.0, 139.0) as usize;
            let y = (start[1] + (end[1] - start[1]) * t)
                .round()
                .clamp(16.0, 238.0) as usize;
            self.pixels[y * 140 + x] = color;
        }
    }
    fn components(self) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut result = Vec::new();
        for (y, row) in self.pixels.chunks_exact(140).enumerate() {
            let mut x = 0;
            while x < 140 {
                let color = row[x];
                let start = x;
                while x < 140 && row[x] == color {
                    x += 1;
                }
                if color != [0, 0, 0] {
                    result.push(GuiComponent::Fill {
                        position: vec2(13.0 + start as f32, 11.0 + y as f32),
                        size: vec2((x - start) as f32, 1.0),
                        color,
                        alpha: 1.0,
                    });
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;
    #[test]
    fn projected_lines_clip_to_playfield_even_during_camera_flythrough() {
        for camera in [-40.0, -1.0, 0.0, 3.0, 7.0, 20.0] {
            let mut raster = Raster::new();
            raster.segment([-1.0, -1.7, 0.0], [1.0, 1.7, 8.0], camera, [255, 255, 255]);
            for component in raster.components() {
                let r = component.rect();
                assert!(r.x >= 13.0 && r.x + r.w <= 153.0 && r.y >= 27.0 && r.y + r.h <= 250.0);
            }
        }
    }
    #[test]
    fn controller_aim_and_pointer_angle_choose_same_lane() {
        let mut game = HogWallow::default();
        for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
            game.aim(x, y);
            let lane = game.game.lane;
            game.controller(HandheldControl::Stick { x, y: -y }, false);
            assert_eq!(game.game.lane, lane);
            game.primary();
            assert_eq!(game.game.shots.last().unwrap().lane, lane);
        }
    }
    #[test]
    fn same_frame_pointer_move_and_press_fires_into_new_lane() {
        let world = World::new();
        let mut game = HogWallow::default();
        game.aim(-1.0, 0.0);
        let old_lane = game.game.lane;
        game.pointer_primary();
        let cursor = GuiCursor {
            position: cgmath::point2(145.0, 149.0),
            held_entity_id: None,
        };
        game.advance(
            &Time {
                total: Duration::from_secs(1),
                elapsed: Duration::ZERO,
            },
            Some(&cursor),
            false,
            &world,
        );
        assert_ne!(game.game.lane, old_lane);
        assert_eq!(game.game.shots.len(), 1);
        assert_eq!(game.game.shots[0].lane, game.game.lane);
    }
    #[test]
    fn score_is_recorded_once_and_only_delayed_restart_clears_latch() {
        let world = World::new();
        world.add_unique(QuestInfo::new());
        let mut game = HogWallow::default();
        game.game.mode = Mode::Scores;
        game.game.score = 120;
        let time = Time {
            total: Duration::from_secs(1),
            elapsed: Duration::from_secs(1),
        };
        assert!(matches!(
            game.advance(&time, None, true, &world),
            Effect::RecordMiniGameScore { game: 6, .. }
        ));
        game.primary();
        assert!(game.recorded);
        assert!(matches!(
            game.advance(&time, None, true, &world),
            Effect::NoEffect
        ));
        game.game.pause = Duration::from_secs(5);
        game.primary();
        assert!(!game.recorded);
        assert_eq!(game.game.mode, Mode::Playing);
    }
}
