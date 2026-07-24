use cgmath::{Vector2, Vector3, vec2};

use engine::assets::asset_cache::AssetCache;
use shipyard::{EntityId, Unique, UniqueView, World};

use crate::gui::{ButtonHoverBehavior, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::quest_info::QuestInfo;

use crate::gui;

use crate::scripts::Effect;

/// The five main-tram elevator stops, decoded from the gamesys `Elev` file-var
/// (`Eng1, medsci1, Hydro2, ops2, Rec1` - projects/flat-ui-panels.md §2.1),
/// index 0 = deck 1. Kept as a verified hardcode with this citation; the
/// gamesys param reader that would source it lands with the trainer PR (§7.1
/// PR C2). Deck 6 (the Rec<->Command tram) is a separate elevator, never on
/// this 5-floor panel.
const ELEVATOR_STOPS: [&str; 5] = [
    "eng1.mis",
    "medsci1.mis",
    "hydro2.mis",
    "ops2.mis",
    "rec1.mis",
];

/// English fallbacks for the MISC.STR `ElevLevel<n>` floor labels, used when a
/// data install lacks the string table (verbatim from retail MISC.STR).
const FALLBACK_FLOOR_LABELS: [&str; 5] = [
    "Engineering (1)",
    "Med / Sci (2)",
    "Hydroponics (3)",
    "Operations (4)",
    "Recreational (5)",
];

/// English fallback for MISC.STR `ElevBlocked`.
const FALLBACK_BLOCKED_LABEL: &str = "Error!  Shaft inaccessible!";

/// Preloaded elevator-panel data, stored as a world `Unique` because
/// `Gui::get_components` has no `AssetCache` to read MISC.STR from at draw
/// time. Populated once at mission load (`MissionCore::load`).
#[derive(Unique, Clone, Debug)]
pub struct ElevatorContext {
    /// Current mission basename (e.g. "medsci1.mis"), to light the current
    /// floor's button (force-lit + inert, matching the original).
    pub current_level: String,
    /// MISC.STR `ElevLevel1..5` floor labels (index 0 = deck 1 = Engineering).
    pub floor_labels: [String; 5],
    /// MISC.STR `ElevBlocked` ("Error!  Shaft inaccessible!").
    pub blocked_label: String,
}

impl ElevatorContext {
    /// Read the elevator floor strings from MISC.STR (falling back to the
    /// verified English defaults when absent) and record the current mission.
    pub fn load(asset_cache: &mut AssetCache, current_level: &str) -> ElevatorContext {
        let strings = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "misc.str");
        let lookup = |key: &str, fallback: &str| -> String {
            strings
                .as_ref()
                .and_then(|s| s.get(&key.to_ascii_lowercase()).cloned())
                .unwrap_or_else(|| fallback.to_owned())
        };
        let floor_labels = std::array::from_fn(|i| {
            lookup(&format!("ElevLevel{}", i + 1), FALLBACK_FLOOR_LABELS[i])
        });
        let blocked_label = lookup("ElevBlocked", FALLBACK_BLOCKED_LABEL);
        ElevatorContext {
            current_level: current_level.to_owned(),
            floor_labels,
            blocked_label,
        }
    }
}

/// Whether a floor (`deck` 1..=5) is reachable given the `ElevState` quest bit
/// (raw value). The original gates the panel on `ElevState`: 0 = no power,
/// 1 = partial ("worm goo") lockout, 2 = fully accessible
/// (projects/flat-ui-panels.md §2.1).
///
/// We deliberately treat the **unset/0 default as fully accessible** rather
/// than "no power": the shipped, hands-on-verified behavior is an ungated
/// elevator, and shipping ungated is "strictly more playable" (doc §7.3 #2).
/// Only an explicit partial-lockout value (`INCOMPLETE`, raw 1) gates - it
/// seals deck 1 (Engineering), leaving decks >= 2 reachable, matching the
/// original's "only buttons >= deck 2 work" rule.
fn is_floor_available(elev_state: u32, deck: usize) -> bool {
    match elev_state {
        1 => deck >= 2,
        _ => true,
    }
}

pub struct ElevatorGui;

#[derive(Clone, Debug, Default)]
pub struct ElevatorGuiState {}

#[derive(Clone)]
pub enum ElevatorGuiMsg {
    ButtonPressed(String),
}

impl Gui<ElevatorGuiState, ElevatorGuiMsg> for ElevatorGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        _state: &ElevatorGuiState,
    ) -> Vec<GuiComponent<ElevatorGuiMsg>> {
        let button_height = 54.0;
        let initial_padding_y = 6.0;
        let initial_padding_x = 14.0;
        let button_width = 142.0;
        let button_padding = 4.0;

        // Floor labels + the current mission come from the preloaded context
        // (MISC.STR is unreachable here - no AssetCache). Missing context (e.g.
        // a debug scene that never loads it) falls back to the English labels.
        let (floor_labels, blocked_label, current_level) = world
            .borrow::<UniqueView<ElevatorContext>>()
            .map(|ctx| {
                (
                    ctx.floor_labels.clone(),
                    ctx.blocked_label.clone(),
                    ctx.current_level.clone(),
                )
            })
            .unwrap_or_else(|_| {
                (
                    FALLBACK_FLOOR_LABELS.map(|s| s.to_owned()),
                    FALLBACK_BLOCKED_LABEL.to_owned(),
                    String::new(),
                )
            });

        // ElevState quest bit (raw): 1 = partial lockout, else accessible.
        let elev_state = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|q| q.read_quest_bit_value("ElevState").bits())
            .unwrap_or(0);

        let mut components: Vec<GuiComponent<ElevatorGuiMsg>> = vec![
            gui::image("elev.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];

        // Top button = deck 5, bottom = deck 1 (the original's index-inverted
        // layout). Row 0 is the topmost button.
        for (row, deck) in (1..=5usize).rev().enumerate() {
            let floor_index = deck - 1;
            let level = ELEVATOR_STOPS[floor_index];
            let label = &floor_labels[floor_index];
            let up_texture = format!("elev{deck}0.pcx");
            let on_texture = format!("elev{deck}1.pcx");

            let button_y = initial_padding_y + (button_height + button_padding) * row as f32;
            let button_x = initial_padding_x;
            let position = vec2(button_x, button_y);
            let size = vec2(button_width, button_height);

            let is_current = level.eq_ignore_ascii_case(&current_level);
            let available = is_floor_available(elev_state, deck);

            if is_current {
                // Current floor: force the lit bitmap and make it inert (no
                // transition) - you can't ride to the deck you're on.
                components.push(
                    gui::image(&on_texture)
                        .with_position(position)
                        .with_size(size),
                );
                components.push(floor_text(label, position));
            } else if available {
                components.push(
                    gui::button(ElevatorGuiMsg::ButtonPressed(level.to_owned()))
                        .with_position(position)
                        .with_size(size)
                        .with_image(&up_texture)
                        .with_hover(ButtonHoverBehavior::Texture(on_texture.clone()))
                        .with_label(label),
                );
                components.push(floor_text(label, position));
            } else {
                // Blocked by the partial-power lockout: inert, showing the
                // ElevBlocked message in place of the floor name.
                components.push(
                    gui::image(&up_texture)
                        .with_position(position)
                        .with_size(size),
                );
                components.push(floor_text(&blocked_label, position));
            }
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.2),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        _world: &World,
        state: &ElevatorGuiState,
        msg: &ElevatorGuiMsg,
    ) -> (ElevatorGuiState, Effect) {
        match msg {
            ElevatorGuiMsg::ButtonPressed(level) => (
                state.clone(),
                Effect::GlobalEffect(crate::scripts::GlobalEffect::TransitionLevel {
                    level_file: level.clone(),
                    loc: Some(22),
                    entities_to_trigger: vec![],
                    vitals_transition: crate::scripts::PlayerVitalsTransition::Preserve,
                }),
            ),
        }
    }
}

/// The floor-name text drawn inside a button (`TEXT_X 60` from the original,
/// projects/flat-ui-panels.md §2.1).
fn floor_text(label: &str, button_position: Vector2<f32>) -> GuiComponent<ElevatorGuiMsg> {
    gui::text(label)
        .with_position(vec2(button_position.x + 60.0, button_position.y + 30.0))
        .with_size(vec2(100.0, 20.0))
        .with_alpha(0.7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_full_power_unlock_every_floor() {
        // Unset (0) and fully-powered (COMPLETE, 2) leave every deck reachable -
        // the shipped playable behavior.
        for state in [0u32, 2] {
            for deck in 1..=5 {
                assert!(is_floor_available(state, deck), "state {state} deck {deck}");
            }
        }
    }

    #[test]
    fn partial_lockout_seals_only_engineering() {
        // INCOMPLETE (1) is the "worm goo" partial lockout: deck 1 sealed,
        // decks >= 2 reachable.
        assert!(!is_floor_available(1, 1));
        for deck in 2..=5 {
            assert!(is_floor_available(1, deck), "deck {deck} should stay open");
        }
    }
}
