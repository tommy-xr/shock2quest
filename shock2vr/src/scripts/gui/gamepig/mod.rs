mod boot;
mod burro_hog;
mod hog_wallow;
mod kabacon;
mod overworld;
mod pig_stacker;
mod ping;
mod street_hog;
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
    BurroHog(burro_hog::BurroHog),
    HogWallow(hog_wallow::HogWallow),
    StreetHog(street_hog::StreetHog),
    OverWorld(overworld::Adventure),
    Ping(ping::Ping),
    PigStacker(pig_stacker::PigStacker),
    KaBacon(kabacon::KaBacon),
    TicTacTriop(tictactriop::TicTacTriop),
    Swinekeeper(swinekeeper::Swinekeeper),
}

#[derive(Clone)]
pub enum GamePigGuiMsg {
    SelectBurroHog,
    BurroField,
    SelectHogWallow,
    WallowFire,
    SelectStreetHog,
    HogCell { column: i32, row: i32 },
    HogRestart,
    SelectOverWorld,
    OverWorld(overworld::Message),
    SelectPing,
    SelectPigStacker,
    StackerPrimary,
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
        entity_id: EntityId,
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
                Game::BurroHog(game) => game.components(
                    super::keypad::object_state(world, entity_id)
                        == dark::properties::ObjectState::Hacked,
                ),
                Game::HogWallow(game) => game.components(
                    world,
                    super::keypad::object_state(world, entity_id)
                        == dark::properties::ObjectState::Hacked,
                ),
                Game::StreetHog(game) => game.components(world),
                Game::OverWorld(game) => game.components(world, entity_id),
                Game::Ping(game) => game.components(world),
                Game::PigStacker(game) => game.components(world),
                Game::KaBacon(game) => game.components(world),
                Game::TicTacTriop(game) => game.components(world),
                Game::Swinekeeper(game) => game.components(world),
            });
            let board = match game {
                Game::BurroHog(_)
                | Game::HogWallow(_)
                | Game::StreetHog(_)
                | Game::OverWorld(_)
                | Game::Ping(_)
                | Game::KaBacon(_)
                | Game::PigStacker(_) => None,
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
                4 => Some(GamePigGuiMsg::SelectOverWorld),
                5 => Some(GamePigGuiMsg::SelectKaBacon),
                6 => Some(GamePigGuiMsg::SelectHogWallow),
                7 => Some(GamePigGuiMsg::SelectStreetHog),
                8 => Some(GamePigGuiMsg::SelectTicTacTriop),
                10 => Some(GamePigGuiMsg::SelectPigStacker),
                11 => Some(GamePigGuiMsg::SelectBurroHog),
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

    fn pointer_secondary(
        &self,
        entity: EntityId,
        world: &World,
        state: &mut GamePigGuiState,
        position: cgmath::Point2<f32>,
    ) -> Effect {
        if state.boot.is_some() {
            return Effect::NoEffect;
        }
        if let Some(Game::HogWallow(game)) = &mut state.game {
            game.bomb(
                super::keypad::object_state(world, entity) == dark::properties::ObjectState::Hacked,
            );
            return Effect::NoEffect;
        }
        if let Some(Game::BurroHog(game)) = &mut state.game {
            game.jump();
            return Effect::NoEffect;
        }
        if let Some(Game::OverWorld(game)) = &state.game {
            return game.inspect(entity, world, position);
        }
        if let Some(Game::PigStacker(game)) = &mut state.game {
            game.hard_drop();
        }
        if let Some(Game::Swinekeeper(game)) = &mut state.game {
            if game.finished() {
                state.game = None;
                state.selection = 0;
            } else {
                let canvas = crate::ui::UiCanvas::from_elements(
                    self.get_config().screen_size_in_pixels,
                    game.components(world),
                );
                if let Some(GamePigGuiMsg::SwineCell(index)) =
                    canvas.click_at(vec2(position.x, position.y))
                {
                    game.flag(index);
                }
            }
        }
        Effect::NoEffect
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
        if let Some(Game::OverWorld(game)) = &mut state.game {
            if matches!(control, HandheldControl::Primary) && game.wants_back() {
                state.game = None;
                state.selection = 0;
                return Effect::NoEffect;
            }
            if !matches!(control, HandheldControl::Back) {
                return game.controller(entity, world, control);
            }
        }
        if let Some(Game::HogWallow(game)) = &mut state.game {
            if !matches!(control, HandheldControl::Back) {
                game.controller(
                    control,
                    super::keypad::object_state(world, entity)
                        == dark::properties::ObjectState::Hacked,
                );
                return Effect::NoEffect;
            }
        }
        if let Some(Game::BurroHog(game)) = &mut state.game {
            if !matches!(control, HandheldControl::Back) {
                game.controller(control);
                return Effect::NoEffect;
            }
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
                if let Some(Game::PigStacker(game)) = &mut state.game {
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
                if let Some(Game::StreetHog(game)) = &mut state.game {
                    game.navigate(x, y);
                    return Effect::NoEffect;
                }
                let (width, height) = match &state.game {
                    Some(
                        Game::HogWallow(_)
                        | Game::BurroHog(_)
                        | Game::StreetHog(_)
                        | Game::OverWorld(_)
                        | Game::Ping(_)
                        | Game::KaBacon(_)
                        | Game::PigStacker(_),
                    ) => {
                        return Effect::NoEffect;
                    }
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
                Some(Game::HogWallow(_) | Game::BurroHog(_)) => None,
                Some(Game::StreetHog(_)) => Some(GamePigGuiMsg::HogRestart),
                Some(Game::OverWorld(_) | Game::Ping(_)) => None,
                Some(Game::PigStacker(_)) => Some(GamePigGuiMsg::StackerPrimary),
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
                    Some(4) => Some(GamePigGuiMsg::SelectOverWorld),
                    Some(5) => Some(GamePigGuiMsg::SelectKaBacon),
                    Some(6) => Some(GamePigGuiMsg::SelectHogWallow),
                    Some(7) => Some(GamePigGuiMsg::SelectStreetHog),
                    Some(8) => Some(GamePigGuiMsg::SelectTicTacTriop),
                    Some(10) => Some(GamePigGuiMsg::SelectPigStacker),
                    Some(11) => Some(GamePigGuiMsg::SelectBurroHog),
                    _ => None,
                },
            },
            HandheldControl::Secondary => {
                if let Some(Game::PigStacker(game)) = &mut state.game {
                    game.hard_drop();
                }
                matches!(state.game, Some(Game::Swinekeeper(_)))
                    .then_some(GamePigGuiMsg::SwineToggleFlag)
            }
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
            if let Some(Game::BurroHog(game)) = &mut state.game {
                game.advance(
                    time,
                    cursor,
                    state.controller_active,
                    super::keypad::object_state(world, entity)
                        == dark::properties::ObjectState::Hacked,
                );
                return Effect::NoEffect;
            }
            if let Some(Game::HogWallow(game)) = &mut state.game {
                return game.advance(time, cursor, state.controller_active, world);
            }
            if let Some(Game::StreetHog(game)) = &mut state.game {
                return game.advance(time, world);
            }
            if let Some(Game::OverWorld(game)) = &mut state.game {
                return game.advance(entity, world, time);
            }
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
            if let Some(Game::PigStacker(game)) = &mut state.game {
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
            GamePigGuiMsg::SelectBurroHog => {
                if installed_games(world).contains(&11) {
                    state.game = Some(Game::BurroHog(burro_hog::BurroHog::default()));
                }
            }
            GamePigGuiMsg::BurroField => {}
            GamePigGuiMsg::SelectHogWallow => {
                if installed_games(world).contains(&6) {
                    state.game = Some(Game::HogWallow(hog_wallow::HogWallow::default()));
                }
            }
            GamePigGuiMsg::WallowFire => {
                if let Some(Game::HogWallow(game)) = &mut state.game {
                    game.pointer_primary();
                }
            }
            GamePigGuiMsg::SelectStreetHog => {
                if installed_games(world).contains(&7) {
                    state.game = Some(Game::StreetHog(street_hog::StreetHog::new(
                        super::keypad::object_state(world, entity_id)
                            == dark::properties::ObjectState::Hacked,
                    )));
                }
            }
            GamePigGuiMsg::HogCell { column, row } => {
                if let Some(Game::StreetHog(game)) = &mut state.game {
                    game.click(*column, *row);
                }
            }
            GamePigGuiMsg::HogRestart => {
                if let Some(Game::StreetHog(game)) = &mut state.game {
                    game.restart();
                }
            }
            GamePigGuiMsg::SelectOverWorld => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|q| q.mini_games & (1 << 4) != 0)
                {
                    state.game = Some(Game::OverWorld(overworld::Adventure::default()));
                    return (
                        state,
                        Effect::UpdateOverWorld {
                            entity_id,
                            command: crate::overworld_game::Command::Start,
                        },
                    );
                }
            }
            GamePigGuiMsg::OverWorld(overworld::Message::Back) => {
                state.game = None;
                state.selection = 0;
            }
            GamePigGuiMsg::OverWorld(message) => {
                if let Some(Game::OverWorld(game)) = &mut state.game {
                    let effect = game.input(entity_id, world, *message);
                    return (state, effect);
                }
            }
            GamePigGuiMsg::SelectPigStacker => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|quest| quest.mini_games & (1 << 10) != 0)
                {
                    state.game = Some(Game::PigStacker(pig_stacker::PigStacker::default()));
                }
            }
            GamePigGuiMsg::StackerPrimary => {
                if let Some(Game::PigStacker(game)) = &mut state.game {
                    game.primary();
                }
            }
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
                    Game::HogWallow(game) => Game::HogWallow(game),
                    Game::StreetHog(game) => Game::StreetHog(game),
                    Game::OverWorld(game) => Game::OverWorld(game),
                    Game::Ping(game) => Game::Ping(game),
                    Game::PigStacker(game) => Game::PigStacker(game),
                    Game::KaBacon(game) => Game::KaBacon(game),
                    Game::BurroHog(game) => Game::BurroHog(game),
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

fn score_rank(world: &World, game: u8, points: i32) -> Option<usize> {
    world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .and_then(|quest| {
            quest
                .mini_game_scores
                .get(&game)
                .map(|scores| scores.rank(points))
        })
        .unwrap_or_else(|| crate::mini_game_scores::MiniGameScores::default().rank(points))
}

/// Campaign score rows share wrapping and empty-line handling across games.
fn score_table(
    world: &World,
    game: u8,
    rank: Option<usize>,
    rect: Rect,
) -> Vec<GuiComponent<GamePigGuiMsg>> {
    let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
    let scores = quest.mini_game_scores.get(&game);
    let rows: Vec<_> = (0..10)
        .map(|index| {
            let text = scores
                .and_then(|scores| scores.entries().get(index))
                .map_or("", |entry| entry.text.as_str());
            PanelText::wrap(world, text, rect.w - 18.0)
        })
        .collect();
    let lines = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let mut components = Vec::new();
    for (index, row) in rows
        .iter()
        .take(((rect.h / 11.0) as usize / lines).min(10))
        .enumerate()
    {
        let y = rect.y + index as f32 * lines as f32 * 11.0;
        components.push(PanelText::text(
            &format!("{}:", index + 1),
            Rect::new(rect.x, y, 18.0, 11.0),
        ));
        for (line, text) in row
            .iter()
            .enumerate()
            .filter(|(_, text)| !text.trim().is_empty())
        {
            components.push(PanelText::text(
                text,
                Rect::new(rect.x + 18.0, y + line as f32 * 11.0, rect.w - 18.0, 11.0),
            ));
        }
    }
    let (key, fallback) = match rank {
        Some(0) => ("minig_topscore", "A New High Score!"),
        Some(_) => ("minig_newscore", "Score Number %d"),
        None => ("minig_youlose", "Score not in top 10."),
    };
    let text = PanelText::string(world, "minigame", key, fallback);
    let text = PanelText::format(&text, &[rank.unwrap_or(0) as i32 + 1]);
    components.push(PanelText::text(&text, Rect::new(34.0, 265.0, 118.0, 15.0)));
    components
}
