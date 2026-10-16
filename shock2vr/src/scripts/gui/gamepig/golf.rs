//! Native courses with shared swing, club, and score controls.
use super::{GamePigGuiMsg, score_rank, score_table, sprite};
use crate::{
    golf_game::{CLUBS, Game, Mode, Notice, PAR},
    gui::{self, GuiComponent, GuiCursor},
    handheld_input::HandheldControl,
    mini_game_scores::MiniGameScore,
    scripts::{Effect, gui::PanelText},
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use shipyard::{UniqueView, World};
use std::time::Duration;

#[derive(Clone, Debug)]
pub(super) struct Golf {
    game: Game,
    last_frame: Option<Duration>,
    primary: bool,
    aim: bool,
    stick: Option<[f32; 2]>,
    recorded: bool,
    rank: Option<usize>,
}
impl Golf {
    pub fn new(world: &World) -> Option<Self> {
        let courses = world
            .borrow::<UniqueView<crate::golf_courses::GolfCourses>>()
            .ok()?
            .0
            .clone()?;
        Some(Self {
            game: Game::new(courses, &mut rand::thread_rng()),
            last_frame: None,
            primary: false,
            aim: false,
            stick: None,
            recorded: false,
            rank: None,
        })
    }
    pub fn primary(&mut self) {
        self.primary = true;
    }
    pub fn aim(&mut self) {
        self.aim = true;
    }
    pub fn club(&mut self, club: usize) {
        self.game.select_club(club);
    }
    pub fn mulligan(&mut self, hacked: bool) {
        self.game.mulligan(hacked, &mut rand::thread_rng());
    }
    pub fn controller(&mut self, control: HandheldControl) {
        match control {
            HandheldControl::Stick { x, y } => self.stick = Some([x, -y]),
            HandheldControl::Primary => self.primary(),
            HandheldControl::Secondary => self.club((self.game.club + 1) % 5),
            _ => {}
        }
    }
    pub fn advance(&mut self, time: &Time, cursor: Option<&GuiCursor>, world: &World) -> Effect {
        if self.last_frame == Some(time.total) {
            return Effect::NoEffect;
        }
        self.last_frame = Some(time.total);
        if let Some([x, y]) = self.stick.take().filter(|[x, y]| x * x + y * y > 0.16) {
            self.game
                .aim([self.game.ball[0] + x * 120.0, self.game.ball[1] + y * 120.0]);
        }
        if std::mem::take(&mut self.aim) {
            if let Some(cursor) = cursor {
                self.game
                    .aim([cursor.position.x - 13.0, cursor.position.y - 11.0]);
            }
        }
        if std::mem::take(&mut self.primary) {
            let previous = self.game.mode;
            self.game.primary(&mut rand::thread_rng());
            if previous == Mode::Scores && self.game.mode == Mode::Aim {
                self.recorded = false;
                self.rank = None;
            }
        }
        self.game.advance(time.elapsed, &mut rand::thread_rng());
        if self.game.mode == Mode::Scores && !self.recorded {
            self.recorded = true;
            let total = self.game.total() as i32;
            let points = 10000 - total;
            self.rank = score_rank(world, 12, points);
            let relative = total - PAR.iter().map(|p| *p as i32).sum::<i32>();
            let format =
                PanelText::string(world, "minigame", "gf_score_long", "Ended %+d (%d shots)")
                    .replacen("%+d", &format!("{relative:+}"), 1);
            return Effect::RecordMiniGameScore {
                game: 12,
                score: MiniGameScore {
                    points,
                    text: PanelText::format(&format, &[total]),
                },
            };
        }
        Effect::NoEffect
    }
    pub fn components(&self, world: &World, hacked: bool) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut result = Vec::new();
        if self.game.mode == Mode::Scores {
            result.push(GuiComponent::Fill {
                position: vec2(13.0, 11.0),
                size: vec2(140.0, 276.0),
                color: [0, 0, 0],
                alpha: 1.0,
            });
            result.push(PanelText::text(
                "Golf scores",
                Rect::new(30.0, 27.0, 118.0, 16.0),
            ));
            result.extend(score_table(
                world,
                12,
                self.rank,
                Rect::new(21.0, 61.0, 130.0, 165.0),
            ));
            result.push(PanelText::text(
                &format!("{} shots", self.game.total()),
                Rect::new(30.0, 230.0, 110.0, 16.0),
            ));
            if self.game.pause >= Duration::from_secs(3) {
                result.push(
                    gui::button(GamePigGuiMsg::GolfPrimary)
                        .with_position(vec2(13.0, 11.0))
                        .with_size(vec2(140.0, 276.0))
                        .with_label("golf-restart")
                        .with_alpha(0.0),
                );
            }
            return result;
        }
        result.push(
            gui::image(&format!("gfhol00{}.pcx", self.game.hole + 1))
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0))
                .with_alpha(1.0),
        );
        let course = self.game.course();
        result.push(
            sprite(
                "gfflag.pcx",
                course.hole[0] as i32 + 2,
                course.hole[1] as i32 - 4,
                4.0,
                8.0,
            )
            .with_alpha(1.0),
        );
        result.push(
            sprite(
                "gfgolf2.pcx",
                self.game.origin[0] as i32 - 4,
                self.game.origin[1] as i32 - 8,
                7.0,
                16.0,
            )
            .with_alpha(1.0),
        );
        if self.game.mode == Mode::Aim {
            let dx = self.game.target[0] - self.game.ball[0];
            let dy = self.game.target[1] - self.game.ball[1];
            let length = dx.hypot(dy).max(0.001);
            let distance = CLUBS[self.game.club].range / 2.2;
            for i in 1..=24 {
                let t = i as f32 / 24.0;
                dot(
                    &mut result,
                    self.game.ball[0] + dx / length * distance * t,
                    self.game.ball[1] + dy / length * distance * t,
                    [255, 255, 255],
                    1.0,
                );
            }
        }
        dot(
            &mut result,
            self.game.ball[0],
            self.game.ball[1],
            [0, 0, 0],
            3.0,
        );
        dot(
            &mut result,
            self.game.ball[0],
            self.game.ball[1] - self.game.ball[2] * 0.4,
            [255, 255, 255],
            2.0,
        );
        let [wx, wy] = course.wind_marker;
        for i in 0..=8 {
            dot(
                &mut result,
                wx + self.game.wind[0] * i as f32 / 16.0,
                wy + self.game.wind[1] * i as f32 / 16.0,
                [255, 255, 0],
                1.0,
            );
        }
        let distance = ((self.game.ball[0] - course.hole[0])
            .hypot(self.game.ball[1] - course.hole[1])
            * 2.2) as i32;
        result.push(PanelText::text(
            &format!("{distance}m"),
            Rect::new(
                13.0 + (wx - 10.0).clamp(0.0, 100.0),
                11.0 + (wy + 10.0).min(238.0),
                40.0,
                14.0,
            ),
        ));
        let strokes = self.game.strokes
            + u32::from(matches!(
                self.game.mode,
                Mode::Aim | Mode::Windup | Mode::Return
            ));
        let relative =
            self.game.total() as i32 - PAR[..self.game.hole].iter().map(|p| *p as i32).sum::<i32>();
        let [sx, sy] = course.score_marker;
        let status = if course.three_lines {
            format!("Hole {}\nShot {strokes}\n{relative:+}", self.game.hole + 1)
        } else {
            format!("H{} S{strokes} {relative:+}", self.game.hole + 1)
        };
        result.push(PanelText::text(
            &status,
            Rect::new(
                13.0 + sx,
                11.0 + sy,
                140.0 - sx,
                if course.three_lines { 42.0 } else { 14.0 },
            ),
        ));
        let msg = match self.game.notice {
            Notice::Water => Some(PanelText::string(
                world,
                "minigame",
                "gf_water",
                "Water: penalty stroke",
            )),
            Notice::OutOfBounds => Some(PanelText::string(
                world,
                "minigame",
                "gf_oob",
                "Out of bounds",
            )),
            Notice::Sunk => Some(PanelText::format(
                &PanelText::string(world, "minigame", "gf_sink_shot", "Sinks it in %d"),
                &[self.game.strokes as i32],
            )),
            Notice::None => None,
        };
        if let Some(msg) = msg {
            result.push(GuiComponent::Fill {
                position: vec2(14.0, 239.0),
                size: vec2(138.0, 23.0),
                color: [0, 0, 0],
                alpha: 1.0,
            });
            result.push(PanelText::text(&msg, Rect::new(16.0, 241.0, 134.0, 20.0)));
        }
        if matches!(self.game.mode, Mode::Windup | Mode::Return | Mode::Flight) {
            result.push(GuiComponent::Fill {
                position: vec2(14.0, 264.0),
                size: vec2(138.0, 10.0),
                color: [255, 255, 255],
                alpha: 1.0,
            });
            for (x, color) in [
                (18.0, [0, 0, 0]),
                (122.0, [255, 0, 0]),
                (18.0 + self.game.power * 104.0, [0, 0, 128]),
                (18.0 + self.game.meter * 104.0, [0, 160, 255]),
            ] {
                result.push(GuiComponent::Fill {
                    position: vec2(13.0 + x.clamp(1.0, 138.0), 264.0),
                    size: vec2(1.0, 10.0),
                    color,
                    alpha: 1.0,
                });
            }
        }
        result.push(
            gui::button(if self.game.mode == Mode::Aim {
                GamePigGuiMsg::GolfAim
            } else {
                GamePigGuiMsg::GolfPrimary
            })
            .with_position(vec2(13.0, 11.0))
            .with_size(vec2(140.0, 252.0))
            .with_label("golf-course")
            .with_alpha(0.0),
        );
        result.push(
            gui::button(GamePigGuiMsg::GolfPrimary)
                .with_position(vec2(13.0, 263.0))
                .with_size(vec2(140.0, 12.0))
                .with_label("golf-swing")
                .with_alpha(0.0),
        );
        for club in 0..5 {
            if club == self.game.club {
                super::highlight(
                    &mut result,
                    Rect::new(13.0 + club as f32 * 28.0, 275.0, 28.0, 12.0),
                );
            }
            result.push(
                gui::button(GamePigGuiMsg::GolfClub(club))
                    .with_position(vec2(13.0 + club as f32 * 28.0, 275.0))
                    .with_size(vec2(28.0, 12.0))
                    .with_label(&format!("golf-club-{club}"))
                    .with_alpha(0.0),
            );
        }
        if hacked
            && !self.game.mulligan_used
            && !matches!(self.game.mode, Mode::HoleDone | Mode::Scores)
        {
            result.push(GuiComponent::Fill {
                position: vec2(114.0, 242.0),
                size: vec2(38.0, 19.0),
                color: [0, 0, 0],
                alpha: 1.0,
            });
            result.push(PanelText::text(
                "Retry",
                Rect::new(115.0, 244.0, 36.0, 14.0),
            ));
            result.push(
                gui::button(GamePigGuiMsg::GolfRetry)
                    .with_position(vec2(114.0, 242.0))
                    .with_size(vec2(38.0, 19.0))
                    .with_label("golf-retry")
                    .with_alpha(0.0),
            );
        }
        result
    }
}
fn dot(result: &mut Vec<GuiComponent<GamePigGuiMsg>>, x: f32, y: f32, color: [u8; 3], size: f32) {
    if x >= 0.0 && y >= 0.0 && x + size <= 140.0 && y + size <= 252.0 {
        result.push(GuiComponent::Fill {
            position: vec2(13.0 + x, 11.0 + y),
            size: vec2(size, size),
            color,
            alpha: 1.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{golf_courses::GolfCourses, golf_game::Course, quest_info::QuestInfo};
    use std::sync::Arc;
    fn world() -> World {
        let mut pixels = vec![100; 140 * 252];
        pixels[180 * 140 + 70] = 32;
        pixels[176 * 140 + 70] = 183;
        let world = World::new();
        world.add_unique(QuestInfo::new());
        world.add_unique(GolfCourses(Some(Arc::new(std::array::from_fn(|_| {
            Course::from_pixels(pixels.clone()).unwrap()
        })))));
        world
    }
    #[test]
    fn queued_aim_precedes_swing_and_controller_cycles_clubs() {
        let world = world();
        let mut golf = Golf::new(&world).unwrap();
        golf.aim();
        golf.primary();
        let cursor = GuiCursor {
            position: cgmath::point2(20.0, 40.0),
            held_entity_id: None,
            pressed: false,
        };
        golf.advance(
            &Time {
                total: Duration::from_secs(1),
                elapsed: Duration::ZERO,
            },
            Some(&cursor),
            &world,
        );
        assert_eq!(golf.game.target, [7.0, 29.0]);
        assert_eq!(golf.game.mode, Mode::Windup);
        golf.controller(HandheldControl::Secondary);
        assert_eq!(golf.game.club, 1);
        assert_eq!(golf.game.mode, Mode::Aim);
    }
    #[test]
    fn completed_round_records_once_and_restart_resets_latch() {
        let world = world();
        let mut golf = Golf::new(&world).unwrap();
        golf.game.mode = Mode::Scores;
        golf.game.card = [1; 9];
        let time = Time {
            total: Duration::from_secs(1),
            elapsed: Duration::ZERO,
        };
        match golf.advance(&time, None, &world) {
            Effect::RecordMiniGameScore { game: 12, score } => {
                assert_eq!(score.points, 9991);
                assert!(score.text.contains("-27"));
            }
            _ => panic!("missing round score"),
        }
        assert!(matches!(
            golf.advance(
                &Time {
                    total: Duration::from_secs(2),
                    ..time
                },
                None,
                &world
            ),
            Effect::NoEffect
        ));
        golf.game.pause = Duration::from_secs(3);
        golf.primary();
        golf.advance(
            &Time {
                total: Duration::from_secs(3),
                ..time
            },
            None,
            &world,
        );
        assert_eq!(golf.game.mode, Mode::Aim);
        assert!(!golf.recorded);
        assert_eq!(golf.game.total(), 0);
    }
    #[test]
    fn hacked_retry_is_visible_and_spent_once() {
        let world = world();
        let mut golf = Golf::new(&world).unwrap();
        let retry = |g: &Golf, hacked| {
            g.components(&world, hacked).iter().any(|c| {
                matches!(
                    c,
                    GuiComponent::Button {
                        on_click: Some(GamePigGuiMsg::GolfRetry),
                        ..
                    }
                )
            })
        };
        assert!(!retry(&golf, false));
        assert!(retry(&golf, true));
        golf.mulligan(true);
        assert!(!retry(&golf, true));
    }
}
