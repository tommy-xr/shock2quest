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
pub struct GamePigGuiState {}

#[derive(Clone)]
pub enum GamePigGuiMsg {}

impl Gui<GamePigGuiState, GamePigGuiMsg> for GamePigGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        _state: &GamePigGuiState,
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
        for index in 1..=12 {
            if games & (1 << index) == 0 {
                continue;
            }
            let name =
                PanelText::string(world, "minigame", &format!("GameName{index}"), "Test Game");
            components.push(PanelText::text(
                &name,
                Rect::new(16.0, 14.0 + (index - 1) as f32 * 15.0, 156.0, 15.0),
            ));
        }
        components
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
        _world: &World,
        state: &GamePigGuiState,
        _msg: &GamePigGuiMsg,
    ) -> (GamePigGuiState, Effect) {
        (state.clone(), Effect::NoEffect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_table::StringTable;
    use std::collections::HashMap;

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
