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
    game: Option<tictactriop::TicTacTriop>,
}

#[derive(Clone)]
pub enum GamePigGuiMsg {
    SelectTicTacTriop,
    PlayCell(usize),
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
        if let Some(game) = &state.game {
            components.extend(game.components(world));
            return components;
        }
        for index in 1..=12 {
            if games & (1 << index) == 0 {
                continue;
            }
            let name =
                PanelText::string(world, "minigame", &format!("GameName{index}"), "Test Game");
            if index == 8 {
                components.push(
                    gui::button(GamePigGuiMsg::SelectTicTacTriop)
                        .with_position(vec2(16.0, 14.0 + (index - 1) as f32 * 15.0))
                        .with_size(vec2(156.0, 15.0))
                        .with_label("game-8")
                        .with_alpha(0.0),
                );
            }
            components.push(PanelText::text(
                &name,
                Rect::new(16.0, 14.0 + (index - 1) as f32 * 15.0, 156.0, 15.0),
            ));
        }
        components
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
        _entity_id: EntityId,
        world: &World,
        state: &GamePigGuiState,
        msg: &GamePigGuiMsg,
    ) -> (GamePigGuiState, Effect) {
        let mut state = state.clone();
        match msg {
            GamePigGuiMsg::SelectTicTacTriop => {
                if world
                    .borrow::<UniqueView<QuestInfo>>()
                    .is_ok_and(|quest| quest.mini_games & (1 << 8) != 0)
                {
                    state.game = Some(tictactriop::TicTacTriop::random());
                }
            }
            GamePigGuiMsg::PlayCell(index) => {
                if let Some(game) = &mut state.game {
                    game.play(*index);
                }
            }
            GamePigGuiMsg::Restart => {
                if state.game.is_some() {
                    state.game = Some(tictactriop::TicTacTriop::random());
                }
            }
        }
        (state, Effect::NoEffect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_table::StringTable;
    use std::collections::HashMap;

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
