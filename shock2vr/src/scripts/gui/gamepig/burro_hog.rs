//! Native girder sprites with pointer dragging and held-controller movement.
use super::{GamePigGuiMsg, sprite};
use crate::{
    burro_hog_game::{BOTTOM, Game, LADDER, Mode, Motion, TOP},
    gui::{self, GuiComponent, GuiCursor},
    handheld_input::HandheldControl,
    scripts::gui::PanelText,
    time::Time,
    ui::Rect,
};
use cgmath::{Point2, vec2};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub(super) struct BurroHog {
    game: Game,
    last_frame: Option<Duration>,
    anchor: Option<Point2<f32>>,
    stick: Option<[f32; 2]>,
    moving: bool,
}
impl BurroHog {
    pub fn controller(&mut self, control: HandheldControl) {
        match control {
            HandheldControl::Stick { x, y } => self.stick = Some([x, -y]),
            HandheldControl::Primary => self.game.jump(),
            _ => {}
        }
    }
    pub fn jump(&mut self) {
        self.game.jump();
    }
    pub fn advance(
        &mut self,
        time: &Time,
        cursor: Option<&GuiCursor>,
        controller: bool,
        hacked: bool,
    ) {
        if self.last_frame == Some(time.total) {
            return;
        }
        self.last_frame = Some(time.total);
        let direction = if controller {
            self.anchor = None;
            let [x, y] = self.stick.take().unwrap_or([0.0, 0.0]);
            if y.abs() > x.abs() && y.abs() > 0.4 {
                [0, y.signum() as i32]
            } else if x.abs() > 0.4 {
                [x.signum() as i32, 0]
            } else {
                [0, 0]
            }
        } else if let Some(cursor) = cursor.filter(|c| {
            c.pressed
                && Rect::new(13.0, 11.0, 140.0, 276.0).contains(vec2(c.position.x, c.position.y))
        }) {
            let anchor = self.anchor.get_or_insert(cursor.position);
            let delta = cursor.position - *anchor;
            if delta.x.abs() < 10.0 && delta.y.abs() > 20.0 {
                [0, delta.y.signum() as i32]
            } else if delta.x.abs() > 10.0 {
                [delta.x.signum() as i32, 0]
            } else {
                [0, 0]
            }
        } else {
            self.anchor = None;
            [0, 0]
        };
        self.moving = direction != [0, 0];
        self.game.steer(direction[0], direction[1]);
        self.game
            .advance(time.elapsed, hacked, &mut rand::thread_rng());
    }
    pub fn components(&self, hacked: bool) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut result = vec![GuiComponent::Fill {
            position: vec2(13.0, 11.0),
            size: vec2(140.0, 276.0),
            color: [0, 0, 0],
            alpha: 1.0,
        }];
        for (row, tiles) in self.game.board().iter().enumerate() {
            for (col, tile) in tiles.iter().enumerate() {
                if *tile == 0 {
                    continue;
                }
                let asset = if tile & TOP != 0 {
                    "dhladtop.pcx"
                } else if tile & BOTTOM != 0 {
                    "dhladbot.pcx"
                } else if tile & LADDER != 0 {
                    "dhladder.pcx"
                } else {
                    "dhgird.pcx"
                };
                result.push(
                    sprite(asset, col as i32 * 8 + 4, row as i32 * 4 + 2, 8.0, 4.0).with_alpha(1.0),
                );
            }
        }
        result.push(
            sprite(
                &format!("dhkong{}.pcx", self.game.kong_frame),
                17,
                27,
                32.0,
                32.0,
            )
            .with_alpha(1.0),
        );
        for ham in &self.game.hams {
            let frame = if ham.direction > 0 {
                self.game.frame % 4
            } else {
                3 - self.game.frame % 4
            };
            result.push(
                sprite(&format!("dhham{frame}.pcx"), ham.x + 4, ham.y + 4, 8.0, 8.0)
                    .with_alpha(1.0),
            );
        }
        let asset = match self.game.motion {
            Motion::Climbing => format!(
                "dhtimcl{}.pcx",
                if self.moving {
                    self.game.frame / 2 % 2
                } else {
                    0
                }
            ),
            Motion::Jumping | Motion::Falling => format!(
                "dhtimjp{}.pcx",
                if self.game.facing > 0 { 'r' } else { 'l' }
            ),
            Motion::Walking => format!(
                "dhtimw{}{}.pcx",
                if self.game.facing > 0 { 'r' } else { 'l' },
                if self.moving {
                    self.game.frame / 2 % 4
                } else {
                    0
                }
            ),
        };
        result.push(sprite(&asset, self.game.x + 6, self.game.y + 8, 12.0, 16.0).with_alpha(1.0));
        result.push(PanelText::text(
            &format!(
                "L{}  {}  {}",
                self.game.level + 1,
                if hacked {
                    "HACKED".to_owned()
                } else {
                    format!("Lives:{}", self.game.lives)
                },
                self.game.score
            ),
            Rect::new(17.0, 13.0, 132.0, 14.0),
        ));
        let message = match self.game.mode {
            Mode::Playing => None,
            Mode::Died => Some("Ouch!"),
            Mode::Won => Some("Level clear!"),
            Mode::GameOver => Some("Game Over"),
        };
        if let Some(message) = message {
            result.push(PanelText::text(
                message,
                Rect::new(45.0, 140.0, 105.0, 14.0),
            ));
        }
        result.push(
            gui::button(GamePigGuiMsg::BurroField)
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0))
                .with_label("burro-field")
                .with_alpha(0.0),
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::point2;
    #[test]
    fn pointer_drag_moves_and_release_stops() {
        let mut game = BurroHog::default();
        let mut cursor = GuiCursor {
            position: point2(70.0, 150.0),
            held_entity_id: None,
            pressed: true,
        };
        let time = |n| Time {
            total: Duration::from_millis(n * 50),
            elapsed: Duration::from_millis(50),
        };
        for n in 1..5 {
            game.advance(&time(n), Some(&cursor), false, true);
        }
        let start = game.game.x;
        cursor.position.x += 25.0;
        game.advance(&time(5), Some(&cursor), false, true);
        assert!(game.game.x > start);
        cursor.pressed = false;
        game.advance(&time(6), Some(&cursor), false, true);
        let stopped = game.game.x;
        game.advance(&time(7), Some(&cursor), false, true);
        assert_eq!(game.game.x, stopped);
    }
    #[test]
    fn controller_capture_ignores_pointer_and_lost_input_stops() {
        let mut game = BurroHog::default();
        for n in 1..5 {
            game.advance(
                &Time {
                    total: Duration::from_millis(n * 50),
                    elapsed: Duration::from_millis(50),
                },
                None,
                true,
                true,
            );
        }
        game.controller(HandheldControl::Stick { x: 1.0, y: 0.0 });
        game.advance(
            &Time {
                total: Duration::from_secs(1),
                elapsed: Duration::from_millis(50),
            },
            None,
            true,
            true,
        );
        let moved = game.game.x;
        game.advance(
            &Time {
                total: Duration::from_secs(2),
                elapsed: Duration::from_millis(50),
            },
            None,
            true,
            true,
        );
        assert_eq!(game.game.x, moved);
    }
}
