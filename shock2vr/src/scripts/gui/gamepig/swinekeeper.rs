use cgmath::vec2;
use rand::seq::SliceRandom;
use shipyard::World;

use super::GamePigGuiMsg;
use crate::{
    gui::{self, GuiComponent},
    scripts::gui::PanelText,
    ui::Rect,
};

const WIDTH: usize = 6;
const HEIGHT: usize = 10;
const CORN: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tile {
    Hidden,
    Revealed,
    Flag,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Reveal,
    Flag,
    Won,
    Lost,
}

#[derive(Clone, Debug)]
pub(super) struct Swinekeeper {
    corn: [bool; WIDTH * HEIGHT],
    tiles: [Tile; WIDTH * HEIGHT],
    mode: Mode,
    flags: usize,
}

impl Swinekeeper {
    pub fn random() -> Self {
        let mut positions: Vec<_> = (0..WIDTH * HEIGHT).collect();
        positions.shuffle(&mut rand::thread_rng());
        let mut corn = [false; WIDTH * HEIGHT];
        for &index in &positions[..CORN] {
            corn[index] = true;
        }
        Self {
            corn,
            tiles: [Tile::Hidden; WIDTH * HEIGHT],
            mode: Mode::Reveal,
            flags: CORN,
        }
    }

    fn neighbors(index: usize) -> impl Iterator<Item = usize> {
        let x = (index % WIDTH) as i32;
        let y = (index / WIDTH) as i32;
        (-1..=1).flat_map(move |dy| {
            (-1..=1).filter_map(move |dx| {
                let (nx, ny) = (x + dx, y + dy);
                ((dx != 0 || dy != 0)
                    && (0..WIDTH as i32).contains(&nx)
                    && (0..HEIGHT as i32).contains(&ny))
                .then_some((ny * WIDTH as i32 + nx) as usize)
            })
        })
    }

    fn swine(&self, index: usize) -> usize {
        Self::neighbors(index).filter(|&i| self.corn[i]).count()
    }

    pub(super) fn finished(&self) -> bool {
        matches!(self.mode, Mode::Won | Mode::Lost)
    }

    fn reveal(&mut self, index: usize) {
        if self.tiles[index] != Tile::Hidden {
            return;
        }
        self.tiles[index] = Tile::Revealed;
        if self.corn[index] {
            self.mode = Mode::Lost;
        } else if self.swine(index) == 0 {
            for neighbor in Self::neighbors(index) {
                self.reveal(neighbor);
            }
        }
    }

    pub fn toggle_flag(&mut self) {
        self.mode = match self.mode {
            Mode::Reveal => Mode::Flag,
            Mode::Flag => Mode::Reveal,
            finished => finished,
        };
    }

    pub fn play(&mut self, index: usize) {
        if index >= self.tiles.len() || self.finished() {
            return;
        }
        if self.mode == Mode::Reveal {
            self.reveal(index);
        } else {
            match self.tiles[index] {
                Tile::Hidden if self.flags > 0 => {
                    self.tiles[index] = Tile::Flag;
                    self.flags -= 1;
                    self.mode = Mode::Reveal;
                    // Finding the corn means flagging it all; uncovering every
                    // safe square alone does not complete the game.
                    if self.flags == 0
                        && self
                            .corn
                            .iter()
                            .zip(self.tiles)
                            .all(|(&corn, tile)| corn == (tile == Tile::Flag))
                    {
                        self.mode = Mode::Won;
                    }
                }
                Tile::Flag => {
                    self.tiles[index] = Tile::Hidden;
                    self.flags += 1;
                }
                _ => {}
            }
        }
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut components = vec![
            gui::image("swinebck.pcx")
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0)),
        ];
        for index in 0..self.tiles.len() {
            let tile = self.tiles[index];
            let texture = if self.finished() && self.corn[index] {
                "swinecrn.pcx".to_owned()
            } else if tile == Tile::Flag {
                "swineflg.pcx".to_owned()
            } else if self.finished() || tile == Tile::Revealed {
                format!("swine{}.pcx", self.swine(index))
            } else {
                "swineune.pcx".to_owned()
            };
            let position = vec2(
                14.0 + (index % WIDTH) as f32 * 23.0,
                52.0 + (index / WIDTH) as f32 * 23.0,
            );
            components.push(
                gui::image(&texture)
                    .with_position(position)
                    .with_size(vec2(22.0, 22.0)),
            );
            if !self.finished() {
                components.push(
                    gui::button(GamePigGuiMsg::SwineCell(index))
                        .with_position(position)
                        .with_size(vec2(23.0, 23.0))
                        .with_label(&format!("swine-cell-{index}"))
                        .with_alpha(0.0),
                );
            }
        }
        let (key, fallback) = match self.mode {
            Mode::Won => ("SwineWin", "You found all the corn! You win!"),
            Mode::Lost => ("SwineLose", "You stepped on the corn!\nGame over."),
            _ => (
                "SwineStart",
                "Corn attracts swine. Don't step on corn! Flag the corn.",
            ),
        };
        components.extend(PanelText::paragraph(
            world,
            &PanelText::string(world, "minigame", key, fallback).replace("\\n", "\n"),
            Rect::new(16.0, 15.0, 110.0, 35.0),
        ));
        if self.mode == Mode::Flag {
            components.push(
                gui::image("swineflg.pcx")
                    .with_position(vec2(126.0, 12.0))
                    .with_size(vec2(22.0, 22.0)),
            );
        }
        components.push(PanelText::text(
            &self.flags.to_string(),
            Rect::new(142.0, 25.0, 14.0, 13.0),
        ));
        components.push(if self.finished() {
            gui::button(GamePigGuiMsg::Restart)
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(139.0, 275.0))
                .with_label("swine-restart")
                .with_alpha(0.0)
        } else {
            gui::button(GamePigGuiMsg::SwineToggleFlag)
                .with_position(vec2(125.0, 11.0))
                .with_size(vec2(27.0, 25.0))
                .with_label("swine-flag-mode")
                .with_alpha(0.0)
        });
        components
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> Swinekeeper {
        let mut game = Swinekeeper::random();
        game.corn = std::array::from_fn(|i| i < CORN);
        game
    }

    #[test]
    fn swinekeeper_random_field_has_twelve_unique_corn_tiles() {
        for _ in 0..20 {
            let game = Swinekeeper::random();
            assert_eq!(game.corn.iter().filter(|&&corn| corn).count(), CORN);
            assert_eq!(game.flags, CORN);
        }
    }

    #[test]
    fn swinekeeper_counts_edges_and_flood_reveals_without_crossing_flags() {
        let mut game = field();
        assert_eq!(game.swine(12), 2);
        assert_eq!(game.swine(13), 3);
        assert_eq!(Swinekeeper::neighbors(0).count(), 3);
        game.toggle_flag();
        game.play(59);
        game.play(58);
        assert_eq!(game.tiles[59], Tile::Flag);
        assert!(game.tiles[12..59].iter().all(|&t| t == Tile::Revealed));
        assert_eq!(game.mode, Mode::Reveal);
        game.play(0);
        assert_eq!(game.mode, Mode::Lost);
        let tiles = game.tiles;
        game.play(1);
        game.play(usize::MAX);
        game.toggle_flag();
        assert_eq!(game.tiles, tiles);
        assert_eq!(game.mode, Mode::Lost);
    }

    #[test]
    fn swinekeeper_requires_all_correct_flags_and_enforces_flag_limit() {
        let mut game = field();
        game.play(59);
        assert!(
            !game.finished(),
            "revealing all safe cells is not a victory"
        );
        for index in 0..CORN {
            game.toggle_flag();
            game.play(index);
        }
        assert_eq!(game.mode, Mode::Won);
        let mut game = field();
        for index in 1..=CORN {
            game.toggle_flag();
            game.play(index);
        }
        assert_eq!(game.flags, 0);
        assert_eq!(game.mode, Mode::Reveal, "a false flag prevents victory");
        game.toggle_flag();
        game.play(0);
        assert_eq!(game.tiles[0], Tile::Hidden);
        game.play(CORN);
        assert_eq!(game.flags, 1);
        assert_eq!(game.mode, Mode::Flag, "removing a flag keeps flag mode");
        game.play(0);
        assert_eq!(game.mode, Mode::Won);
    }
}
