//! Three-by-three play against a full-depth minimax opponent.
use cgmath::vec2;
use shipyard::World;

use super::GamePigGuiMsg;
use crate::{
    gui::{self, GuiComponent},
    scripts::gui::PanelText,
    ui::{Rect, UiCanvas},
};

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];
const CORNERS: [usize; 4] = [0, 2, 6, 8];
const BOARD_X: f32 = 22.0;
const BOARD_Y: f32 = 91.0;
const CELL: f32 = 40.0;

#[derive(Clone, Debug)]
pub(super) struct TicTacTriop {
    board: [u8; 9],
    player: u8,
}

impl TicTacTriop {
    pub fn new(player_first: bool, corner: usize) -> Self {
        let mut game = Self {
            board: [0; 9],
            player: if player_first { 1 } else { 2 },
        };
        // Retail randomly gives the player X or O; Xerxes opens at a corner
        // when it owns X. This is a mark, not a free extra player turn.
        if !player_first {
            game.board[CORNERS[corner % 4]] = 1;
        }
        game
    }

    pub fn random() -> Self {
        Self::new(rand::random(), rand::random::<usize>())
    }

    fn winning_line(board: &[u8; 9]) -> Option<[usize; 3]> {
        LINES
            .into_iter()
            .find(|line| board[line[0]] != 0 && line.iter().all(|&i| board[i] == board[line[0]]))
    }

    fn finished(&self) -> bool {
        Self::winning_line(&self.board).is_some() || !self.board.contains(&0)
    }

    // X maximizes, O minimizes. A winning child ends the
    // search, equal scores replace the choice only for X, and draws score 0.
    fn minimax(board: [u8; 9], mover: u8) -> (i8, Option<usize>) {
        if let Some(line) = Self::winning_line(&board) {
            return (if board[line[0]] == 1 { 1 } else { -1 }, None);
        }
        let mut best: Option<(i8, usize)> = None;
        for index in 0..9 {
            if board[index] != 0 {
                continue;
            }
            let mut next = board;
            next[index] = mover;
            let score = Self::minimax(next, 3 - mover).0;
            if best.is_none_or(|(value, _)| {
                if mover == 1 {
                    score >= value
                } else {
                    score < value
                }
            }) {
                best = Some((score, index));
            }
            if score == if mover == 1 { 1 } else { -1 } {
                break;
            }
        }
        best.map_or((0, None), |(score, index)| (score, Some(index)))
    }

    pub fn play(&mut self, index: usize) {
        if self.finished() || self.board.get(index) != Some(&0) {
            return;
        }
        self.board[index] = self.player;
        if !self.finished() {
            let opponent = 3 - self.player;
            if let Some(reply) = Self::minimax(self.board, opponent).1 {
                self.board[reply] = opponent;
            }
        }
    }

    pub fn components(&self, world: &World) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let mut canvas = UiCanvas::with_events(vec2(188.0, 296.0));
        canvas.push(
            gui::image("pongback.pcx")
                .with_position(vec2(13.0, 11.0))
                .with_size(vec2(140.0, 276.0)),
        );
        let winner = Self::winning_line(&self.board);
        let message = if let Some(line) = winner {
            let who = if self.board[line[0]] == self.player {
                "YouHave"
            } else {
                "ComputerHas"
            };
            format!(
                "{} {}",
                PanelText::string(
                    world,
                    "minigame",
                    who,
                    if who == "YouHave" {
                        "You Have"
                    } else {
                        "Xerxes Has"
                    }
                ),
                PanelText::string(world, "minigame", "Won", "Won")
            )
        } else if self.finished() {
            PanelText::string(world, "minigame", "Draw", "Draw")
        } else {
            PanelText::string(world, "minigame", "YourMove", "Your move")
        };
        canvas.push(PanelText::centered(
            &message,
            Rect::new(13.0, 12.0, 139.0, 18.0),
        ));
        for offset in [CELL, CELL * 2.0] {
            canvas.fill(
                Rect::new(BOARD_X + offset, BOARD_Y, 1.0, CELL * 3.0),
                [0, 255, 192],
            );
            canvas.fill(
                Rect::new(BOARD_X, BOARD_Y + offset, CELL * 3.0, 1.0),
                [0, 255, 192],
            );
        }
        for index in 0..9 {
            let x = BOARD_X + (index % 3) as f32 * CELL;
            let y = BOARD_Y + (index / 3) as f32 * CELL;
            let owner = self.board[index];
            if owner != 0 {
                canvas.push(
                    gui::image(if owner == self.player {
                        "tttplayr.pcx"
                    } else {
                        "tttshodn.pcx"
                    })
                    .with_position(vec2(x + 1.0, y + 1.0))
                    .with_size(vec2(CELL, CELL)),
                );
            } else if !self.finished() {
                canvas.push(
                    gui::button(GamePigGuiMsg::PlayCell(index))
                        .with_position(vec2(x, y))
                        .with_size(vec2(CELL, CELL))
                        .with_label(&format!("ttt-cell-{index}"))
                        .with_alpha(0.0),
                );
            }
        }
        if let Some(line) = winner {
            // The shared canvas has axis-aligned fills; rasterize the thin
            // retail winning stroke in canvas pixels so both presenters agree.
            let a = line[0];
            let b = line[2];
            let start = vec2(
                BOARD_X + (a % 3) as f32 * CELL + CELL / 2.0,
                BOARD_Y + (a / 3) as f32 * CELL + CELL / 2.0,
            );
            let end = vec2(
                BOARD_X + (b % 3) as f32 * CELL + CELL / 2.0,
                BOARD_Y + (b / 3) as f32 * CELL + CELL / 2.0,
            );
            for step in 0..=80 {
                let point = start + (end - start) * (step as f32 / 80.0);
                canvas.fill(Rect::new(point.x, point.y, 1.0, 1.0), [0, 255, 192]);
            }
        }
        if self.finished() {
            canvas.push(
                gui::button(GamePigGuiMsg::Restart)
                    .with_position(vec2(13.0, 11.0))
                    .with_size(vec2(139.0, 275.0))
                    .with_label("ttt-restart")
                    .with_alpha(0.0),
            );
        }
        canvas.into_elements()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tictactriop_minimax_cannot_lose_to_any_legal_player_sequence() {
        fn explore(game: TicTacTriop) {
            if let Some(line) = TicTacTriop::winning_line(&game.board) {
                assert_ne!(game.board[line[0]], game.player);
                return;
            }
            for i in 0..9 {
                if game.board[i] == 0 {
                    let mut next = game.clone();
                    next.play(i);
                    explore(next);
                }
            }
        }
        explore(TicTacTriop::new(true, 0));
        for corner in 0..4 {
            explore(TicTacTriop::new(false, corner));
        }
    }

    #[test]
    fn tictactriop_rejects_outside_occupied_and_finished_moves() {
        let mut game = TicTacTriop::new(false, 0);
        let initial = game.board;
        game.play(0);
        game.play(9);
        game.play(usize::MAX);
        assert_eq!(game.board, initial);
        game.board = [1, 1, 1, 2, 2, 0, 0, 0, 0];
        let finished = game.board;
        game.play(5);
        assert_eq!(game.board, finished);
    }
}
