//! O/S upgrade machine MFD panel (projects/flat-ui-panels.md §4).
//!
//! The original game's O/S upgrade overlay: one pick from all 16 traits on a
//! 4x4 icon matrix, an owned-slots row (up to 4 - the whole game has exactly
//! four machines), a description of the hovered trait from `TRAITS.STR`, no
//! confirmation step, and **purchases are free**. On buy the machine becomes
//! single-use ("Used").
//!
//! Ours mirrors that: traits are stored on the character sheet
//! (`PlayerStats::os_traits`, persisted via `QuestInfo`), the per-machine used
//! flag is a quest bit keyed by the machine's stable mission object id (also
//! `QuestInfo`, so it survives save/load and deck re-entry), and
//! `Effect::AcquireOsTrait` applies the pick atomically. Live effects are
//! implemented for the subset with existing consumers (Tank, Naturally Able,
//! Pack-Rat, Pharmo-Friendly, Replicator Expert, Security Expert - see
//! [`live_effect_note`]); everything else stays visible
//! but cannot consume a one-shot machine or trait slot until its effect exists.

use cgmath::{Vector2, Vector3, vec2};
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::player_stats::OS_TRAIT_SLOTS;
use crate::quest_info::QuestInfo;
use crate::scripts::Effect;

use crate::gui;

/// The 16 O/S traits, by the original game's trait id (matching the shipped
/// `TRAITS.STR` `Trait1..16` order, verified against those strings).
pub const OS_TRAITS: [(u8, &str); 16] = [
    (1, "Strong Metabolism"),
    (2, "Pharmo-Friendly"),
    (3, "Pack-Rat"),
    (4, "Speedy"),
    (5, "Sharpshooter"),
    (6, "Naturally Able"),
    (7, "Cybernetically Enhanced"),
    (8, "Tank"),
    (9, "Lethal Weapon"),
    (10, "Security Expert"),
    (11, "Smasher"),
    (12, "Cyber-Assimilation"),
    (13, "Replicator Expert"),
    (14, "Power Psi"),
    (15, "Tinker"),
    (16, "Spatially Aware"),
];

/// Retail trait ids with live gameplay effects here (see the effect handler).
pub const TRAIT_NATURALLY_ABLE: u8 = 6;
pub const TRAIT_PACK_RAT: u8 = 3;
pub const TRAIT_PHARMO_FRIENDLY: u8 = 2;
pub const TRAIT_TANK: u8 = 8;
pub const TRAIT_REPLICATOR_EXPERT: u8 = 13;
pub const TRAIT_SECURITY_EXPERT: u8 = 10;

/// Tank: "+5 maximum hit points" (TRAITS.STR Trait8). The original raises the
/// ceiling AND current HP by the bonus on purchase (buying at 25/30 yields
/// 30/35); the live grant does the same, and both re-derive on every mission
/// load (the player entity is rebuilt each load).
pub const TANK_HP_BONUS: i32 = 5;

/// Naturally Able: "One-time bonus of 8 Cyber Enhancement Units" (Trait6).
pub const NATURALLY_ABLE_MODULES: i32 = 8;

/// Display name of a trait id, or "?" for an out-of-range id.
pub fn trait_name(trait_id: u8) -> &'static str {
    OS_TRAITS
        .iter()
        .find(|(id, _)| *id == trait_id)
        .map(|(_, name)| *name)
        .unwrap_or("?")
}

/// The quest bit marking a machine as used, keyed by the machine's stable
/// mission object id (for a concrete level object, `PropTemplateId` holds the
/// object's own positive mission id - NOT the shared archetype id - so each
/// machine keys its own bit). Lives in `QuestInfo`, so it survives save/load
/// and deck re-entry - the durable equivalent of the original machine script
/// remembering a "Used" message.
///
/// Note: the bit space is game-global while object ids are per-mission; the
/// four shipped machines have distinct ids (medsci2 133, rec3 153, rick3 511,
/// hydro2 879), so no cross-lock is possible with retail data.
pub fn used_bit_name(machine_template_id: i32) -> String {
    format!("trait_machine_used_{}", machine_template_id)
}

/// The machine's stable mission object id (`PropTemplateId`), if any. A
/// machine without one cannot record a used state, so it must not vend.
fn machine_template_id(world: &World, machine: EntityId) -> Option<i32> {
    world
        .borrow::<View<dark::properties::PropTemplateId>>()
        .ok()
        .and_then(|v| v.get(machine).ok().map(|t| t.template_id))
}

/// Whether the machine bound to this panel has already vended (its used bit
/// is set). Machines without a stable id read as unused here (the display
/// path); the pick path refuses them outright.
fn machine_used(world: &World, machine: EntityId) -> bool {
    let Some(template_id) = machine_template_id(world, machine) else {
        return false;
    };
    world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|q| q.read_quest_bit_value(&used_bit_name(template_id)).bits() != 0)
        .unwrap_or(false)
}

/// English fallbacks for the MISC.STR trait-panel strings (verbatim from the
/// shipped table), used when a data install lacks it.
const FALLBACK_HEADER_LABEL: &str = "Choose one upgrade.";
const FALLBACK_USED_LABEL: &str = "Your OS has already been upgraded at this unit.";
const UNAVAILABLE_LABEL: &str = "Upgrade unavailable in this build.";

/// Preloaded trait-panel strings (`TRAITS.STR` descriptions plus the MISC.STR
/// header / used-machine lines), added as a world unique at mission load so
/// the (`AssetCache`-less) `TraitGui` can show them - the `ElevatorContext`
/// pattern.
#[derive(shipyard::Unique)]
pub struct TraitsContext {
    /// `Trait1..16` description strings; index 0 = trait id 1. Falls back to
    /// the bare trait name when TRAITS.STR is absent.
    pub descriptions: [String; 16],
    /// MISC.STR `TraitHeader` ("Choose one upgrade."), drawn atop the panel.
    pub header_label: String,
    /// MISC.STR `TraitMachineUsed`, the used-machine refusal.
    pub used_label: String,
}

impl TraitsContext {
    pub fn load(asset_cache: &mut AssetCache) -> TraitsContext {
        let strings = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "traits.str");
        let descriptions = std::array::from_fn(|i| {
            let key = format!("trait{}", i + 1);
            strings
                .as_ref()
                .and_then(|s| s.get(&key).cloned())
                .unwrap_or_else(|| trait_name((i + 1) as u8).to_owned())
        });
        let misc = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "misc.str");
        let misc_lookup = |key: &str, fallback: &str| -> String {
            misc.as_ref()
                .and_then(|s| s.get(key).cloned())
                .unwrap_or_else(|| fallback.to_owned())
        };
        TraitsContext {
            descriptions,
            header_label: misc_lookup("traitheader", FALLBACK_HEADER_LABEL),
            used_label: misc_lookup("traitmachineused", FALLBACK_USED_LABEL),
        }
    }
}

/// The used-machine refusal line (MISC.STR `TraitMachineUsed`), from the
/// preloaded context (fallback for scenes that never load it).
fn used_label(world: &World) -> String {
    world
        .borrow::<UniqueView<TraitsContext>>()
        .map(|ctx| ctx.used_label.clone())
        .unwrap_or_else(|_| FALLBACK_USED_LABEL.to_owned())
}

// Panel layout on the 188x296 backdrop: header text at (17,14); owned-slots
// row at (15,35) in 35x34 cells; 4x4 selection matrix in (15,76)-(152,209),
// icons TRAIT01..16 at 34x32, `which = col + row*4 + 1`; description text
// below from (15,214).
const HEADER_X: f32 = 17.0;
const HEADER_Y: f32 = 14.0;
const OWNED_X: f32 = 15.0;
const OWNED_Y: f32 = 35.0;
const OWNED_PITCH: f32 = 35.0;
const CELL_W: f32 = 34.0;
const CELL_H: f32 = 32.0;
const MATRIX_X: f32 = 15.0;
const MATRIX_Y: f32 = 76.0;
const MATRIX_PITCH_X: f32 = 34.5;
const MATRIX_PITCH_Y: f32 = 33.5;
const DESC_X: f32 = 15.0;
const DESC_Y: f32 = 214.0;
/// Bottom of the description area ((15,214)-(174,264) in the original).
const DESC_Y_MAX: f32 = 264.0;
/// Crude wrap width for the ~160px description column with the engine font.
const DESC_CHARS_PER_LINE: usize = 34;

/// Greedy word-wrap: split `text` into lines of at most `width` characters
/// (long single words get their own over-long line rather than splitting).
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The trait icon art (`TRAIT01.PCX`..`TRAIT16.PCX`; `TRAIT00.PCX` is the
/// empty-slot art).
fn trait_icon(trait_id: u8) -> String {
    format!("trait{:02}.pcx", trait_id)
}

pub struct TraitGui;

#[derive(Clone, Debug, Default)]
pub struct TraitGuiState {
    /// Feedback line (acquired / refusal), shown in the description area.
    message: Option<String>,
}

#[derive(Clone)]
pub enum TraitGuiMsg {
    Pick(u8),
}

impl Gui<TraitGuiState, TraitGuiMsg> for TraitGui {
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &TraitGuiState,
    ) -> Vec<GuiComponent<TraitGuiMsg>> {
        let header = world
            .borrow::<UniqueView<TraitsContext>>()
            .map(|ctx| ctx.header_label.clone())
            .unwrap_or_else(|_| FALLBACK_HEADER_LABEL.to_owned());
        let mut components: Vec<GuiComponent<TraitGuiMsg>> = vec![
            gui::image("traits.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
            // Panel header (MISC.STR TraitHeader), as in the original.
            gui::text(&header)
                .with_position(vec2(HEADER_X, HEADER_Y))
                .with_size(vec2(160.0, 12.0)),
        ];

        let owned: Vec<u8> = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|q| q.player_stats().os_traits.clone())
            .unwrap_or_default();

        // Owned-slots row: acquired trait icons, empty-slot art for the rest.
        for slot in 0..OS_TRAIT_SLOTS {
            let icon = owned
                .get(slot)
                .map(|id| trait_icon(*id))
                .unwrap_or_else(|| "trait00.pcx".to_owned());
            components.push(
                gui::image(&icon)
                    .with_position(vec2(OWNED_X + OWNED_PITCH * slot as f32, OWNED_Y))
                    .with_size(vec2(CELL_W, CELL_H)),
            );
        }

        // 4x4 selection matrix: which = col + row*4 + 1.
        for (idx, (trait_id, name)) in OS_TRAITS.iter().enumerate() {
            let col = idx % 4;
            let row = idx / 4;
            components.push(
                gui::button(TraitGuiMsg::Pick(*trait_id))
                    .with_position(vec2(
                        MATRIX_X + MATRIX_PITCH_X * col as f32,
                        MATRIX_Y + MATRIX_PITCH_Y * row as f32,
                    ))
                    .with_size(vec2(CELL_W, CELL_H))
                    .with_image(&trait_icon(*trait_id))
                    .with_label(name),
            );
        }

        // Description area: the machine's used state, then purchase feedback,
        // then the hovered trait's TRAITS.STR text (the original shows the
        // hovered description there).
        let mut lines: Vec<String> = Vec::new();
        if machine_used(world, entity_id) {
            lines.push(used_label(world));
        }
        if let Some(message) = &state.message {
            // A used-machine refusal repeats the standing used line - skip
            // the duplicate.
            if !lines.contains(message) {
                lines.push(message.clone());
            }
        }
        if let Some(hovered) = cursor.as_ref().and_then(|c| hovered_trait(c.position)) {
            let description = world
                .borrow::<UniqueView<TraitsContext>>()
                .map(|ctx| ctx.descriptions[(hovered - 1) as usize].clone())
                .unwrap_or_else(|_| trait_name(hovered).to_owned());
            lines.push(description);
            if live_effect_note(hovered).is_none() {
                lines.push(UNAVAILABLE_LABEL.to_string());
            }
        }
        // Word-wrap into the description box (the engine text path draws a
        // single unwrapped line; sentence-length TRAITS.STR text would run
        // off the 188px panel).
        let mut y = DESC_Y;
        'lines: for line in &lines {
            for wrapped in wrap_text(line, DESC_CHARS_PER_LINE) {
                if y > DESC_Y_MAX {
                    break 'lines;
                }
                components.push(
                    gui::text(&wrapped)
                        .with_position(vec2(DESC_X, y))
                        .with_size(vec2(160.0, 12.0)),
                );
                y += 12.0;
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
        entity_id: EntityId,
        world: &World,
        _state: &TraitGuiState,
        msg: &TraitGuiMsg,
    ) -> (TraitGuiState, Effect) {
        let TraitGuiMsg::Pick(trait_id) = msg;
        // Pre-validate for immediate feedback; the effect handler re-validates
        // and applies atomically (handle_msg must not mutate).
        let refusal = {
            let quests = world.borrow::<UniqueView<QuestInfo>>().unwrap();
            let stats = quests.player_stats();
            if machine_template_id(world, entity_id).is_none() {
                Some("This upgrade unit is not responding.".to_string())
            } else if machine_used(world, entity_id) {
                Some(used_label(world))
            } else if live_effect_note(*trait_id).is_none() {
                Some(UNAVAILABLE_LABEL.to_string())
            } else if stats.has_os_trait(*trait_id) {
                Some(format!("{} is already installed.", trait_name(*trait_id)))
            } else if stats.os_traits.len() >= OS_TRAIT_SLOTS {
                Some("All O/S upgrade slots are full.".to_string())
            } else {
                None
            }
        };
        match refusal {
            Some(message) => (
                TraitGuiState {
                    message: Some(message),
                },
                Effect::NoEffect,
            ),
            None => (
                TraitGuiState {
                    message: Some(format!("{} installed.", trait_name(*trait_id))),
                },
                Effect::AcquireOsTrait {
                    trait_id: *trait_id,
                    machine: entity_id,
                },
            ),
        }
    }
}

/// The trait id under a panel-pixel cursor position, if it is over a matrix
/// cell.
fn hovered_trait(cursor: cgmath::Point2<f32>) -> Option<u8> {
    for (idx, (trait_id, _)) in OS_TRAITS.iter().enumerate() {
        let col = idx % 4;
        let row = idx / 4;
        let x = MATRIX_X + MATRIX_PITCH_X * col as f32;
        let y = MATRIX_Y + MATRIX_PITCH_Y * row as f32;
        if cursor.x >= x && cursor.x <= x + CELL_W && cursor.y >= y && cursor.y <= y + CELL_H {
            return Some(*trait_id);
        }
    }
    None
}

/// One-line note of a trait's live effect, if implemented (for logs).
pub fn live_effect_note(trait_id: u8) -> Option<&'static str> {
    match trait_id {
        TRAIT_TANK => Some("+5 max hit points"),
        TRAIT_NATURALLY_ABLE => Some("+8 cyber modules"),
        TRAIT_PACK_RAT => Some("+3 backpack slots"),
        TRAIT_PHARMO_FRIENDLY => Some("20% healing-item bonus"),
        TRAIT_REPLICATOR_EXPERT => Some("20% replicator discount"),
        TRAIT_SECURITY_EXPERT => Some("+2 Hack at security consoles"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::point2;

    #[test]
    fn sixteen_traits_in_retail_order() {
        assert_eq!(OS_TRAITS.len(), 16);
        // Spot checks against TRAITS.STR (Trait4 Speedy, Trait8 Tank, Trait16
        // Spatially Aware).
        assert_eq!(trait_name(4), "Speedy");
        assert_eq!(trait_name(8), "Tank");
        assert_eq!(trait_name(16), "Spatially Aware");
        // Ids are exactly 1..=16 in order (which = col + row*4 + 1).
        for (idx, (id, _)) in OS_TRAITS.iter().enumerate() {
            assert_eq!(*id as usize, idx + 1);
        }
    }

    #[test]
    fn matrix_hit_test_maps_cells_to_ids() {
        // Center of cell (col 0, row 0) = trait 1; (col 3, row 3) = trait 16.
        assert_eq!(
            hovered_trait(point2(MATRIX_X + 1.0, MATRIX_Y + 1.0)),
            Some(1)
        );
        assert_eq!(
            hovered_trait(point2(
                MATRIX_X + MATRIX_PITCH_X * 3.0 + 5.0,
                MATRIX_Y + MATRIX_PITCH_Y * 3.0 + 5.0
            )),
            Some(16)
        );
        // Off the matrix: no hover.
        assert_eq!(hovered_trait(point2(5.0, 5.0)), None);
        assert_eq!(hovered_trait(point2(100.0, 280.0)), None);
    }

    #[test]
    fn used_bit_is_keyed_by_stable_mission_id() {
        assert_eq!(used_bit_name(133), "trait_machine_used_133");
    }

    #[test]
    fn descriptions_word_wrap_to_the_panel_column() {
        let lines = wrap_text(
            "Tank: Increases maximum hit points by 5.",
            DESC_CHARS_PER_LINE,
        );
        assert!(lines.len() >= 2, "sentence text wraps to multiple lines");
        assert!(lines.iter().all(|l| l.len() <= DESC_CHARS_PER_LINE));
        // The full text survives the wrap.
        assert_eq!(lines.join(" "), "Tank: Increases maximum hit points by 5.");
        // Degenerate inputs.
        assert!(wrap_text("", 10).is_empty());
        assert_eq!(wrap_text("word", 10), vec!["word".to_string()]);
    }

    #[test]
    fn storage_only_trait_does_not_consume_the_machine() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let machine = world.add_entity((dark::properties::PropTemplateId { template_id: 133 },));

        let (state, effect) = TraitGui.handle_msg(
            machine,
            &world,
            &TraitGuiState::default(),
            &TraitGuiMsg::Pick(4), // Speedy has no locomotion consumer yet.
        );

        assert!(matches!(effect, Effect::NoEffect));
        assert_eq!(state.message.as_deref(), Some(UNAVAILABLE_LABEL));
        assert_eq!(
            world
                .borrow::<UniqueView<QuestInfo>>()
                .unwrap()
                .read_quest_bit_value(&used_bit_name(133)),
            dark::properties::QuestBitValue::UNKNOWN,
            "a refused trait must leave the one-shot machine unused"
        );
    }

    #[test]
    fn pack_rat_is_selectable_now_that_backpack_width_consumes_it() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let machine = world.add_entity((dark::properties::PropTemplateId { template_id: 133 },));

        let (state, effect) = TraitGui.handle_msg(
            machine,
            &world,
            &TraitGuiState::default(),
            &TraitGuiMsg::Pick(TRAIT_PACK_RAT),
        );

        assert!(matches!(
            effect,
            Effect::AcquireOsTrait {
                trait_id: TRAIT_PACK_RAT,
                machine: selected_machine,
            } if selected_machine == machine
        ));
        assert_eq!(state.message.as_deref(), Some("Pack-Rat installed."));
    }
}
