mod boot;
mod kabacon;
mod ping;
mod swinekeeper;
mod tictactriop;

use super::PanelText;
use cgmath::{vec2, vec3};
use shipyard::{EntityId, UniqueView, World};

use crate::{
    gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor},
    quest_info::QuestInfo,
    scripts::Effect,
    ui::Rect,
};

pub struct GamePigGui;

#[derive(Clone, Debug, Default)]
pub struct GamePigGuiState {
    game: Option<Game>,
    boot: Option<boot::Boot>,
    selection: usize,
    controller_active: bool,
}

#[derive(Clone, Debug)]
enum Game {
    Ping(ping::Ping),
    KaBacon(kabacon::KaBacon),
    TicTacTriop(tictactriop::TicTacTriop),
    Swinekeeper(swinekeeper::Swinekeeper),
}

#[derive(Clone)]
pub enum GamePigGuiMsg {
    SelectPing,
    SelectKaBacon,
    BaconActivate,
    SelectTicTacTriop,
    SelectSwinekeeper,
    PlayCell(usize),
    SwineCell(usize),
    SwineToggleFlag,
    Restart,
}

impl Gui<GamePigGuiState, GamePigGuiMsg> for GamePigGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        state: &GamePigGuiState,
    ) -> Vec<GuiComponent<GamePigGuiMsg>> {
        // The index leaves locked rows empty and keeps
        // each game's authored index, rather than packing unlocked titles.
        let games = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|quest| quest.mini_games)
            .unwrap_or(0);
        let mut components = vec![
            gui::image("gameback.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];
        if let Some(boot) = &state.boot {
            components.extend(boot.components());
            return components;
        }
        if let Some(game) = &state.game {
            components.extend(match game {
                Game::Ping(game) => game.components(world),
                Game::KaBacon(game) => game.components(world),
                Game::TicTacTriop(game) => game.components(world),
                Game::Swinekeeper(game) => game.components(world),
            });
            let board = match game {
                Game::Ping(_) | Game::KaBacon(_) => None,
                Game::TicTacTriop(_) => Some((22.0, 91.0, 40.0, 3)),
                Game::Swinekeeper(_) => Some((14.0, 52.0, 23.0, 6)),
            };
            if let Some((x, y, size, width)) = board.filter(|_| state.controller_active) {
                highlight(
                    &mut components,
                    Rect::new(
                        x + (state.selection % width) as f32 * size,
                        y + (state.selection / width) as f32 * size,
                        size,
                        size,
                    ),
                );
            }
            return components;
        }
        let installed = installed_games(world);
        for index in 1..=12 {
            if games & (1 << index) == 0 {
                continue;
            }
            if state.controller_active && installed.get(state.selection) == Some(&index) {
                highlight(
                    &mut components,
                    Rect::new(14.0, 14.0 + (index - 1) as f32 * 15.0, 137.0, 15.0),
                );
            }
            let name =
                PanelText::string(world, "minigame", &format!("GameName{index}"), "Test Game");
            let selection = match index {
                2 => Some(GamePigGuiMsg::SelectPing),
                3 => Some(GamePigGuiMsg::SelectSwinekeeper),
                5 => Some(GamePigGuiMsg::SelectKaBacon),
                8 => Some(GamePigGuiMsg::SelectTicTacTriop),
                _ => None,
            };
            if let Some(selection) = selection {
                components.push(
                    gui::button(selection)
                        .with_position(vec2(16.0, 14.0 + (index - 1) as f32 * 15.0))
                        .with_size(vec2(135.0, 15.0))
                        .with_label(&format!("game-{index}"))
                        .with_alpha(0.0),
                );
            }
            components.push(PanelText::text(
                &name,
                Rect::new(16.0, 14.0 + (index - 1) as f32 * 15.0, 135.0, 15.0),
            ));
        }
        components
    }

    fn handheld_input(
        &self,
        entity: EntityId,
        world: &World,
        state: &mut GamePigGuiState,
        control: crate::handheld_input::HandheldControl,
    ) -> Effect {
        use crate::handheld_input::HandheldControl;
        state.controller_active = true;
        if state.boot.is_some() {
            return Effect::NoEffect;
        }
        let message = match control {
            HandheldControl::Focus => return Effect::NoEffect,
            HandheldControl::Stick { x, .. } => {
                if let Some(Game::Ping(game)) = &mut state.game {
                    game.stick = Some(x);
                }
                if let Some(Game::KaBacon(game)) = &mut state.game {
                    game.stick = Some(x);
                }
                return Effect::NoEffect;
            }
            HandheldControl::Back => {
                state.game = None;
                state.selection = 0;
                return Effect::NoEffect;
            }
            HandheldControl::Navigate { x, y } => {
                let (width, height) = match &state.game {
                    Some(Game::Ping(_) | Game::KaBacon(_)) => return Effect::NoEffect,
                    Some(Game::TicTacTriop(_)) => (3, 3),
                    Some(Game::Swinekeeper(_)) => (6, 10),
                    None => (1, installed_games(world).len().max(1)),
                };
                let column = (state.selection % width) as i32;
                let row = (state.selection / width) as i32;
                state.selection = (row + y).rem_euclid(height as i32) as usize * width
                    + (column + x).rem_euclid(width as i32) as usize;
                return Effect::NoEffect;
            }
            HandheldControl::Primary => match &state.game {
                Some(Game::Ping(_)) => None,
                Some(Game::KaBacon(_)) => Some(GamePigGuiMsg::BaconActivate),
                Some(Game::TicTacTriop(game)) => Some(if game.finished() {
                    GamePigGuiMsg::Restart
                } else {
                    GamePigGuiMsg::PlayCell(state.selection)
                }),
                Some(Game::Swinekeeper(game)) => Some(if game.finished() {
                    GamePigGuiMsg::Restart
                } else {
                    GamePigGuiMsg::SwineCell(state.selection)
                }),
                None => match installed_games(world).get(state.selection) {
                    Some(2) => Some(GamePigGuiMsg::SelectPing),
                    Some(3) => Some(GamePigGuiMsg::SelectSwinekeeper),
                    Some(5) => Some(GamePigGuiMsg::SelectKaBacon),
                    Some(8) => Some(GamePigGuiMsg::SelectTicTacTriop),
                    _ => None,
                },
            },
            HandheldControl::Secondary => matches!(state.game, Some(Game::Swinekeeper(_)))
                .then_some(GamePigGuiMsg::SwineToggleFlag),
        };
        if let Some(message) = message {
            let (next, effect) = self.handle_msg(entity, world, state, &message);
            *state = next;
            effect
        } else {
            Effect::NoEffect
        }
    }

    fn on_panel_opened(&self, _world: &World, state: &mut GamePigGuiState) {
        *state = GamePigGuiState {
            boot: crate::dev_params::get_bool(crate::dev_params::GAMEPIG_INTRO)
                .then(boot::Boot::default),
            ..Default::default()
        };
    }

    fn on_frame(
        &self,
        entity: EntityId,
        world: &World,
        state: &mut GamePigGuiState,
        time: &crate::time::Time,
        cursor: Option<&GuiCursor>,
    ) -> Effect {
        if !crate::dev_params::get_bool(crate::dev_params::GAMEPIG_INTRO) {
            state.boot = None;
        }
        let Some(boot) = &mut state.boot else {
            if let Some(Game::Ping(game)) = &mut state.game {
                let finished = game.advance(time, cursor, state.controller_active);
                if finished {
                    state.game = None;
                    state.selection = 0;
                }
            }
            if let Some(Game::KaBacon(game)) = &mut state.game {
                return game.advance(time, cursor, state.controller_active, world);
            }
            return Effect::NoEffect;
        };
        let effect = boot.advance(entity, time);
        if boot.ready() {
            state.boot = None;
        }
        effect
    }

    fn resets_state_on_frob(&self) -> bool {
        true
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: vec3(0.0, 0.0, -0.05),
            screen_size_in_pixels: vec2(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &GamePigGuiState,
        msg: &GamePigGuiMsg,
    ) -> (GamePigGuiState, Effect) {
        let mut state = state.clone();
        if state.boot.is_some() {
            return (state, Effect::NoEffect);
        }
        match msg {
            GamePigGuiMsg::SelectKaBacon => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|quest| quest.mini_games & (1 << 5) != 0)
                {
                    state.game = Some(Game::KaBacon(kabacon::KaBacon::default()));
                }
            }
            GamePigGuiMsg::BaconActivate => {
                if let Some(Game::KaBacon(game)) = &mut state.game {
                    game.activate(
                        super::keypad::object_state(world, entity_id)
                            == dark::properties::ObjectState::Hacked,
                    );
                }
            }
            GamePigGuiMsg::SelectPing => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|quest| quest.mini_games & (1 << 2) != 0)
                {
                    state.game = Some(Game::Ping(ping::Ping::default()));
                }
            }
            GamePigGuiMsg::SelectTicTacTriop => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|quest| quest.mini_games & (1 << 8) != 0)
                {
                    state.selection = 0;
                    state.game = Some(Game::TicTacTriop(tictactriop::TicTacTriop::random()));
                }
            }
            GamePigGuiMsg::SelectSwinekeeper => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|quest| quest.mini_games & (1 << 3) != 0)
                {
                    state.selection = 0;
                    state.game = Some(Game::Swinekeeper(swinekeeper::Swinekeeper::random()));
                }
            }
            GamePigGuiMsg::PlayCell(index) => {
                if let Some(Game::TicTacTriop(game)) = &mut state.game {
                    game.play(*index);
                }
            }
            GamePigGuiMsg::SwineCell(index) => {
                if let Some(Game::Swinekeeper(game)) = &mut state.game {
                    game.play(*index);
                }
            }
            GamePigGuiMsg::SwineToggleFlag => {
                if let Some(Game::Swinekeeper(game)) = &mut state.game {
                    game.toggle_flag();
                }
            }
            GamePigGuiMsg::Restart => {
                state.game = state.game.map(|game| match game {
                    Game::Ping(game) => Game::Ping(game),
                    Game::KaBacon(game) => Game::KaBacon(game),
                    Game::TicTacTriop(_) => Game::TicTacTriop(tictactriop::TicTacTriop::random()),
                    Game::Swinekeeper(_) => Game::Swinekeeper(swinekeeper::Swinekeeper::random()),
                });
            }
        }
        (state, Effect::NoEffect)
    }
}

fn installed_games(world: &World) -> Vec<usize> {
    let bits = world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|q| q.mini_games)
        .unwrap_or(0);
    (1..=12).filter(|index| bits & (1 << index) != 0).collect()
}

fn highlight(components: &mut Vec<GuiComponent<GamePigGuiMsg>>, rect: Rect) {
    for (x, y, w, h) in [
        (rect.x, rect.y, rect.w, 1.0),
        (rect.x, rect.y + rect.h - 1.0, rect.w, 1.0),
        (rect.x, rect.y, 1.0, rect.h),
        (rect.x + rect.w - 1.0, rect.y, 1.0, rect.h),
    ] {
        components.push(GuiComponent::Fill {
            position: vec2(x, y),
            size: vec2(w, h),
            color: [0, 255, 192],
            alpha: 1.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_table::StringTable;
    use std::collections::HashMap;

    #[test]
    fn edge_sprite_crops_source_pixels_in_shared_layout() {
        let component = sprite("kbbomber.pcx", 12, 32, 32.0, 32.0);
        assert_eq!(component.rect(), Rect::new(13.0, 27.0, 28.0, 32.0));
        assert!(matches!(
            component,
            GuiComponent::Image {
                kind: crate::ui::ImageKind::Crop {
                    u0: 0.125,
                    v0: 0.0,
                    u1: 1.0,
                    v1: 1.0
                },
                alpha: 0.5,
                ..
            }
        ));
    }

    #[test]
    fn tictactriop_requires_unlock_and_reopening_returns_to_index() {
        let mut world = World::new();
        let entity = world.add_entity(());
        world.add_unique(QuestInfo::new());
        let (state, _) = GamePigGui.handle_msg(
            entity,
            &world,
            &GamePigGuiState::default(),
            &GamePigGuiMsg::SelectTicTacTriop,
        );
        assert!(state.game.is_none());
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .mini_games = 1 << 8;
        let (mut state, _) =
            GamePigGui.handle_msg(entity, &world, &state, &GamePigGuiMsg::SelectTicTacTriop);
        assert!(state.game.is_some());
        GamePigGui.prepare_state_on_frob(&mut state);
        assert!(state.game.is_none());
    }

    #[test]
    fn index_uses_only_unlocked_authored_names_at_retail_rows() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let mut quest = QuestInfo::new();
        quest.mini_games = (1 << 3) | (1 << 8);
        world.add_unique(quest);
        world.add_unique(StringTable::from_table(
            "minigame.str",
            HashMap::from([
                ("gamename3".into(), "Swinekeeper".into()),
                ("gamename8".into(), "Tic-Tac-Triop".into()),
                ("gamename12".into(), "Golf".into()),
            ]),
        ));
        let components =
            GamePigGui.get_components(&None, entity, &world, &GamePigGuiState::default());
        let text: Vec<_> = components
            .iter()
            .filter_map(|component| match component {
                GuiComponent::Text { text, position, .. } => Some((text.as_str(), *position)),
                _ => None,
            })
            .collect();
        assert_eq!(
            text,
            vec![
                ("Swinekeeper", vec2(16.0, 44.0)),
                ("Tic-Tac-Triop", vec2(16.0, 119.0))
            ]
        );
        assert_eq!(
            GamePigGui.get_config().screen_size_in_pixels,
            vec2(188.0, 296.0)
        );
    }
}

fn sprite(name: &str, x: i32, y: i32, w: f32, h: f32) -> GuiComponent<GamePigGuiMsg> {
    // Games draw into the authored field before either presentation maps it.
    // Edge sprites must not spill over the flat casing or differ from VR's crop.
    let left = x as f32 - w / 2.0;
    let top = y as f32 - h / 2.0;
    let x0 = left.max(0.0);
    let y0 = top.max(0.0);
    let x1 = (left + w).min(140.0).max(x0);
    let y1 = (top + h).min(276.0).max(y0);
    let mut canvas = crate::ui::UiCanvas::with_events(vec2(188.0, 296.0));
    canvas.cropped_image(
        Rect::new(13.0 + x0, 11.0 + y0, x1 - x0, y1 - y0),
        name,
        Rect::new(x0 - left, y0 - top, x1 - x0, y1 - y0),
        vec2(w, h),
    );
    canvas.into_elements().pop().unwrap().with_alpha(0.5)
}
