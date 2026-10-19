use cgmath::vec2;
use dark::properties::ObjectState;
use shipyard::{EntityId, World};

use super::{GamePigGuiMsg, PanelText};
use crate::{
    gui::{self, GuiComponent},
    scripts::{
        Effect,
        gui::keypad::{self, HackState},
    },
    ui::{Rect, canvas_viewport::CanvasViewport},
};

// Index and the unused Slots entry are not games.
pub(crate) const ALL_GAMES: u32 = 0x1ffc;

pub(crate) fn can_hack(world: &World, entity: EntityId) -> bool {
    !matches!(
        keypad::object_state(world, entity),
        ObjectState::Hacked | ObjectState::Broken | ObjectState::Destroyed
    ) && keypad::hack_diff(world, entity).is_some()
}

pub(crate) fn success(entity: EntityId, _world: &World) -> Effect {
    Effect::combine(vec![
        Effect::SetObjectState {
            entity_id: entity,
            state: ObjectState::Hacked,
        },
        Effect::UnlockMiniGames { games: ALL_GAMES },
    ])
}

pub(crate) fn critical_failure(entity: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id: entity,
        state: ObjectState::Broken,
    }
}

pub(super) fn button(
    message: GamePigGuiMsg,
    label: &str,
    text: &str,
    rect: Rect,
) -> Vec<GuiComponent<GamePigGuiMsg>> {
    vec![
        GuiComponent::Fill {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            color: [0, 45, 40],
            alpha: 1.0,
        },
        PanelText::centered(text, rect),
        gui::button(message)
            .with_rect(rect)
            .with_alpha(0.0)
            .with_label(label),
    ]
}

pub(super) fn components(
    world: &World,
    entity: EntityId,
    state: &HackState,
) -> Vec<GuiComponent<GamePigGuiMsg>> {
    let mut components =
        vec![gui::image("gameback.pcx").with_rect(Rect::new(0.0, 0.0, 188.0, 296.0))];
    if let Some(diff) = keypad::hack_diff(world, entity) {
        // Fit the complete board, including its right-hand START and cost,
        // into the same content window in flat and mounted presentations.
        let viewport = CanvasViewport::fit(
            Rect::new(0.0, 0.0, 188.0, 296.0),
            Rect::new(13.0, 11.0, 140.0, 240.0),
        );
        let scale = viewport.dst.w / viewport.src.w;
        components.extend(
            keypad::draw_hack_panel(world, entity, state, diff, false, GamePigGuiMsg::Hack)
                .into_iter()
                .map(|component| {
                    let rect = viewport.to_dst(component.rect());
                    let mut component = component.with_rect(rect);
                    if let GuiComponent::Text { font_size, .. } = &mut component {
                        *font_size = if *font_size <= 0.0 {
                            PanelText::line_height(world)
                        } else {
                            *font_size
                        } * scale;
                    }
                    component
                }),
        );
    }
    components.extend(button(
        GamePigGuiMsg::Back,
        "gamepig-back",
        "BACK",
        Rect::new(16.0, 262.0, 134.0, 20.0),
    ));
    components
}

/// Use the shared button rectangles so stick focus follows the visible board.
pub(super) fn navigate(
    buttons: &[&GuiComponent<GamePigGuiMsg>],
    selected: usize,
    x: i32,
    y: i32,
) -> usize {
    let Some(current) = buttons.get(selected) else {
        return 0;
    };
    let direction = vec2(x as f32, y as f32);
    let origin = current.rect().center();
    buttons
        .iter()
        .enumerate()
        .filter_map(|(index, button)| {
            let delta = button.rect().center() - origin;
            let along = delta.x * direction.x + delta.y * direction.y;
            let across = (delta.x * direction.y - delta.y * direction.x).abs();
            (along > 0.1).then_some((index, along + across * 4.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(selected, |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::super::{GamePigGui, GamePigGuiState};
    use super::*;
    use crate::{
        gui::Gui,
        quest_info::QuestInfo,
        scripts::gui::keypad::{HackNode, HackPhase, KeyPadMsg},
    };
    use dark::properties::{PropHackDiff, PropObjState, PropScripts};

    fn fixture(nanites: i32) -> (World, EntityId) {
        let mut world = World::new();
        let entity = world.add_entity((
            PropHackDiff {
                success_chance: 85,
                critical_chance: 0,
                cost: 50.0,
            },
            PropScripts {
                scripts: vec!["MiniGameBoy".into()],
                inherits: false,
            },
        ));
        let mut quest = QuestInfo::new();
        quest.player_stats_mut().nanites = nanites;
        quest.mini_games = 1 << 3;
        world.add_unique(quest);
        (world, entity)
    }

    fn open(world: &World, entity: EntityId) -> GamePigGuiState {
        let (state, effect) = GamePigGui.handle_msg(
            entity,
            world,
            &GamePigGuiState::default(),
            &GamePigGuiMsg::OpenHack,
        );
        assert!(matches!(effect, Effect::NoEffect));
        assert_eq!(state.hack.as_ref().unwrap().phase, HackPhase::Unpaid);
        state
    }

    #[test]
    fn gamepig_hacking_requires_payment_and_only_a_winning_board_unlocks_games() {
        let (world, entity) = fixture(500);
        let unpaid = open(&world, entity);
        let (playing, paid) = GamePigGui.handle_msg(
            entity,
            &world,
            &unpaid,
            &GamePigGuiMsg::Hack(KeyPadMsg::StartHack),
        );
        let effects = Effect::flatten(vec![paid]);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SpendNanites { amount: 50 }))
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::UnlockMiniGames { .. }))
        );
        for seed in 1..1000 {
            let mut state = playing.clone();
            state.hack.as_mut().unwrap().rng_state = seed;
            for x in 2..=4 {
                let (next, effect) = GamePigGui.handle_msg(
                    entity,
                    &world,
                    &state,
                    &GamePigGuiMsg::Hack(KeyPadMsg::PlayNode { x, y: 0 }),
                );
                state = next;
                let effects = Effect::flatten(vec![effect]);
                let won = state.hack.as_ref().unwrap().phase == HackPhase::Won;
                assert_eq!(
                    effects
                        .iter()
                        .any(|e| matches!(e, Effect::UnlockMiniGames { games: ALL_GAMES })),
                    won
                );
                if won {
                    assert!(effects.iter().any(|e| matches!(e, Effect::SetObjectState { entity_id, state: ObjectState::Hacked } if *entity_id == entity)));
                    let (_, again) = GamePigGui.handle_msg(
                        entity,
                        &world,
                        &state,
                        &GamePigGuiMsg::Hack(KeyPadMsg::StartHack),
                    );
                    assert!(matches!(again, Effect::NoEffect));
                    assert_eq!(
                        world
                            .borrow::<shipyard::UniqueView<QuestInfo>>()
                            .unwrap()
                            .mini_games,
                        1 << 3,
                        "scripts return effects instead of mutating the campaign"
                    );
                    return;
                }
            }
        }
        panic!("no winning deterministic board found");
    }

    #[test]
    fn gamepig_hacking_cancellation_empty_wallet_and_critical_loss_never_unlock() {
        let (mut world, entity) = fixture(0);
        let state = open(&world, entity);
        let (state, effect) = GamePigGui.handle_msg(
            entity,
            &world,
            &state,
            &GamePigGuiMsg::Hack(KeyPadMsg::StartHack),
        );
        assert_eq!(
            state.hack.as_ref().unwrap().phase,
            HackPhase::InsufficientNanites
        );
        assert!(!Effect::flatten(vec![effect]).iter().any(|e| matches!(
            e,
            Effect::SpendNanites { .. } | Effect::UnlockMiniGames { .. }
        )));
        let (closed, effect) = GamePigGui.handle_msg(entity, &world, &state, &GamePigGuiMsg::Back);
        assert!(closed.hack.is_none() && matches!(effect, Effect::NoEffect));
        world.add_component(
            entity,
            PropHackDiff {
                success_chance: 0,
                critical_chance: 14,
                cost: 50.0,
            },
        );
        let mut state = open(&world, entity);
        let board = state.hack.as_mut().unwrap();
        board.phase = HackPhase::Playing;
        board.nodes[keypad::board_index(2, 0)] = HackNode::Mine;
        let (state, effect) = GamePigGui.handle_msg(
            entity,
            &world,
            &state,
            &GamePigGuiMsg::Hack(KeyPadMsg::PlayNode { x: 2, y: 0 }),
        );
        assert_eq!(state.hack.as_ref().unwrap().phase, HackPhase::Lost);
        let effects = Effect::flatten(vec![effect]);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::SetObjectState {
                state: ObjectState::Broken,
                ..
            }
        )));
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::UnlockMiniGames { .. }))
        );
        world.add_component(entity, PropObjState(ObjectState::Broken));
        let (closed, _) = GamePigGui.handle_msg(entity, &world, &state, &GamePigGuiMsg::Back);
        let drawn = GamePigGui.get_components(&None, entity, &world, &closed);
        assert!(!drawn.iter().any(|c| c.click_event().is_some()));
    }

    #[test]
    fn gamepig_hacking_layout_and_stick_targets_fit_the_physical_screen() {
        let (world, entity) = fixture(500);
        let state = open(&world, entity);
        let drawn = components(&world, entity, state.hack.as_ref().unwrap());
        for element in drawn.iter().skip(1) {
            let r = element.rect();
            assert!(
                r.x >= 13.0 && r.y >= 11.0 && r.x + r.w <= 153.01 && r.y + r.h <= 287.0,
                "{r:?}"
            );
        }
        let buttons: Vec<_> = drawn.iter().filter(|c| c.click_event().is_some()).collect();
        assert!(matches!(
            buttons[state.selection].click_event(),
            Some(GamePigGuiMsg::Hack(KeyPadMsg::StartHack))
        ));
        let above = navigate(&buttons, state.selection, 0, -1);
        assert!(matches!(
            buttons[above].click_event(),
            Some(GamePigGuiMsg::Hack(KeyPadMsg::PlayNode { .. }))
        ));
        assert_ne!(navigate(&buttons, above, -1, 0), above);
        let back = navigate(&buttons, state.selection, 0, 1);
        assert!(matches!(
            buttons[back].click_event(),
            Some(GamePigGuiMsg::Back)
        ));
    }

    fn plug_button(world: &World, entity: EntityId, state: &GamePigGuiState) -> Option<Rect> {
        GamePigGui
            .get_components(&None, entity, world, state)
            .iter()
            .find(|c| matches!(c.click_event(), Some(GamePigGuiMsg::OpenHack)))
            .map(GuiComponent::rect)
    }

    #[test]
    fn gamepig_index_raises_the_hrm_plug_instead_of_a_hack_row() {
        use crate::scripts::gui::hrm_plug::{CANVAS_W, PLUG_RECT, plug_sidecar};
        let (mut world, entity) = fixture(500);
        let index = GamePigGuiState::default();
        let drawn = GamePigGui.get_components(&None, entity, &world, &index);
        assert!(
            drawn
                .iter()
                .any(|c| matches!(c, GuiComponent::Image { .. }) && c.rect() == PLUG_RECT),
            "the plug's backdrop is drawn beside the body"
        );
        assert!(
            !drawn
                .iter()
                .any(|c| matches!(c, GuiComponent::Text { text, .. } if text == "HACK")),
            "the placeholder HACK row is gone"
        );
        let button = plug_button(&world, entity, &index).expect("plug button");
        assert!(PLUG_RECT.contains(button.center()), "{button:?}");
        assert_eq!(
            GamePigGui.sidecar(entity, &world, &index),
            Some(plug_sidecar(true))
        );
        assert_eq!(
            GamePigGui
                .get_config_for(entity, &world, &index)
                .screen_size_in_pixels,
            vec2(CANVAS_W, 296.0)
        );

        // The room stays reserved, but the plug is hidden beside a board or game.
        let hacking = open(&world, entity);
        let (playing, _) =
            GamePigGui.handle_msg(entity, &world, &index, &GamePigGuiMsg::SelectSwinekeeper);
        assert!(playing.game.is_some());
        let booting = GamePigGuiState {
            boot: Some(Default::default()),
            ..Default::default()
        };
        for state in [&hacking, &playing, &booting] {
            assert_eq!(plug_button(&world, entity, state), None);
            assert_eq!(
                GamePigGui.sidecar(entity, &world, state),
                Some(plug_sidecar(false))
            );
        }
        world.add_component(entity, PropObjState(ObjectState::Hacked));
        assert_eq!(plug_button(&world, entity, &index), None);
        assert_eq!(
            GamePigGui.sidecar(entity, &world, &index),
            Some(plug_sidecar(false))
        );
        let unhackable = world.add_entity(PropScripts {
            scripts: vec!["MiniGameBoy".into()],
            inherits: false,
        });
        assert_eq!(plug_button(&world, unhackable, &index), None);
        assert_eq!(
            GamePigGui.sidecar(unhackable, &world, &index),
            Some(plug_sidecar(false))
        );
    }

    /// The held screen windows only the body, so its controller never selects
    /// the plug; like other plug panels, the plug takes a pointer.
    #[test]
    fn gamepig_held_controls_cycle_the_games_without_selecting_the_plug() {
        use crate::handheld_input::HandheldControl;
        let (world, entity) = fixture(500);
        let mut state = GamePigGuiState::default();
        for y in [-1, 1, -1] {
            GamePigGui.handheld_input(
                entity,
                &world,
                &mut state,
                HandheldControl::Navigate { x: 0, y },
            );
            assert_eq!(state.selection, 0, "Swinekeeper is the only row");
        }
        GamePigGui.handheld_input(entity, &world, &mut state, HandheldControl::Primary);
        assert!(state.hack.is_none());
        assert!(matches!(
            state.game,
            Some(super::super::Game::Swinekeeper(_))
        ));
    }

    #[test]
    fn gamepig_hacking_psionic_route_shares_unlocks_and_rejects_terminal_devices() {
        let (mut world, entity) = fixture(500);
        let (_, outcomes) = crate::scripts::gui::psi_hack::target(&world, entity).unwrap();
        assert!(
            Effect::flatten(vec![(outcomes.success)(entity, &world)])
                .iter()
                .any(|e| matches!(e, Effect::UnlockMiniGames { games: ALL_GAMES }))
        );
        assert_eq!(ALL_GAMES.count_ones(), 11);
        for state in [
            ObjectState::Hacked,
            ObjectState::Broken,
            ObjectState::Destroyed,
        ] {
            world.add_component(entity, PropObjState(state));
            assert!(!can_hack(&world, entity));
            assert!(crate::scripts::gui::psi_hack::target(&world, entity).is_none());
        }
    }
}
