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
//! available for all sixteen classic traits; see [`live_effect_note`]. Invalid
//! trait IDs cannot consume a one-shot machine or trait slot.

use cgmath::{Vector2, Vector3, vec2};
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::player_stats::OS_TRAIT_SLOTS;
use crate::quest_info::QuestInfo;
use crate::scripts::Effect;

use crate::gui;
use crate::ui::Rect;

use super::panel_text::PanelText;

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
pub const TRAIT_STRONG_METABOLISM: u8 = 1;
pub const TRAIT_NATURALLY_ABLE: u8 = 6;
pub const TRAIT_PACK_RAT: u8 = 3;
pub const TRAIT_PHARMO_FRIENDLY: u8 = 2;
pub const TRAIT_TINKER: u8 = 15;
pub const TRAIT_TANK: u8 = 8;
pub const TRAIT_SPEEDY: u8 = 4;
pub const TRAIT_SHARPSHOOTER: u8 = 5;
pub const TRAIT_LETHAL_WEAPON: u8 = 9;
pub const TRAIT_SMASHER: u8 = 11;
pub const TRAIT_CYBERNETICALLY_ENHANCED: u8 = 7;
pub const TRAIT_SPATIALLY_AWARE: u8 = 16;
pub const TRAIT_SECURITY_EXPERT: u8 = 10;
pub const TRAIT_POWER_PSI: u8 = 14;
pub const TRAIT_REPLICATOR_EXPERT: u8 = 13;
/// Cyber-Assimilation: unlocks a creature's `P$GuarLoot` drop on top of its
/// ordinary loot table.
pub const TRAIT_CYBER_ASSIMILATION: u8 = 12;

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

/// Offline machines remain inspectable but cannot vend an upgrade.
pub fn trait_machine_locked(world: &World, machine: EntityId) -> bool {
    world
        .borrow::<View<dark::properties::PropLocked>>()
        .is_ok_and(|locked| locked.get(machine).is_ok_and(|lock| lock.0))
}
const OFFLINE_LABEL: &str = "Station offline. Check its wave requirement.";

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

/// Classic-rules descriptions plus the MISC.STR header / used-machine lines,
/// added as a world unique at mission load so
/// the (`AssetCache`-less) `TraitGui` can show them - the `ElevatorContext`
/// pattern.
#[derive(shipyard::Unique)]
pub struct TraitsContext {
    /// Descriptions of the supported classic rules; index 0 = trait id 1.
    /// Community Patch additions must not leak into these promises.
    pub descriptions: [String; 16],
    /// MISC.STR `TraitHeader` ("Choose one upgrade."), drawn atop the panel.
    pub header_label: String,
    /// MISC.STR `TraitMachineUsed`, the used-machine refusal.
    pub used_label: String,
}

impl TraitsContext {
    pub fn load(asset_cache: &mut AssetCache) -> TraitsContext {
        // The mounted Community Patch replaces TRAITS.STR with promises of
        // additional mechanics. This port targets classic retail: keep the
        // player-facing descriptions tied to its implemented rules instead.
        let descriptions = [
            "Strong Metabolism: Radiation damage reduced by 25%; toxin damage reduced by 50%.",
            "Pharmo-Friendly: 20% more benefit from healing, psi and hazard-treatment items.",
            "Pack-Rat: Adds three extra inventory slots.",
            "Speedy: Movement speed increased by 15%.",
            "Sharpshooter: Ranged, non-psionic weapons deal 35% more damage.",
            "Naturally Able: One-time bonus of 8 cyber modules.",
            "Cybernetically Enhanced: Allows two implants of different types at once.",
            "Tank: Adds 5 maximum and current hit points.",
            "Lethal Weapon: Melee attacks deal 35% more damage.",
            "Security Expert: +2 Hack at security computers. Requires at least Hack 1.",
            "Smasher: Hold trigger for 380 ms, then release: +6 melee base damage. In VR, strike within 0.8 s.",
            "Cyber-Assimilation: Destroyed robots drop repair modules that heal 15 hit points.",
            "Replicator Expert: Replicator purchases cost 20% less.",
            "Power Psi: Psionic burnout no longer damages you. Failed casts still spend psi points.",
            "Tinker: Weapon modification nanite costs reduced by 50%.",
            "Spatially Aware: The entire map of each sublevel is revealed.",
        ].map(str::to_owned);
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
/// Retail help well. Build-only availability feedback uses the spare space
/// beneath it so it cannot displace or truncate the authored description.
const DESC_RECT: Rect = Rect::new(DESC_X, DESC_Y, 159.0, DESC_Y_MAX - DESC_Y);
const STATUS_RECT: Rect = Rect::new(DESC_X, 266.0, 159.0, 24.0);

/// The trait icon art (`TRAIT01.PCX`..`TRAIT16.PCX`; `TRAIT00.PCX` is the
/// empty-slot art).
pub(crate) fn trait_icon(trait_id: u8) -> String {
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
            PanelText::text(&header, Rect::new(HEADER_X, HEADER_Y, 143.0, 12.0)),
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
            if owned.contains(trait_id) {
                continue;
            }
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

        // Installed icons have the same hover help as the purchase matrix.
        // Fresh hover help wins over stale purchase/refusal feedback.
        let hovered = cursor.as_ref().and_then(|c| {
            owned
                .iter()
                .enumerate()
                .find_map(|(slot, id)| {
                    Rect::new(OWNED_X + OWNED_PITCH * slot as f32, OWNED_Y, CELL_W, CELL_H)
                        .contains(vec2(c.position.x, c.position.y))
                        .then_some(*id)
                })
                .or_else(|| hovered_trait(c.position).filter(|id| !owned.contains(id)))
        });
        let description = hovered.map(|id| {
            world
                .borrow::<UniqueView<TraitsContext>>()
                .map(|ctx| ctx.descriptions[(id - 1) as usize].clone())
                .unwrap_or_else(|_| trait_name(id).to_owned())
        });
        if let Some(text) = description.as_ref().or(state.message.as_ref()) {
            components.extend(PanelText::paragraph(world, text, DESC_RECT));
        }
        let status = if trait_machine_locked(world, entity_id) {
            Some(OFFLINE_LABEL.to_owned())
        } else if machine_used(world, entity_id) {
            Some(used_label(world))
        } else if hovered.is_some_and(|id| live_effect_note(id).is_none()) {
            Some(UNAVAILABLE_LABEL.to_owned())
        } else {
            None
        };
        if let Some(text) = status {
            components.extend(PanelText::paragraph(world, &text, STATUS_RECT));
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
            } else if trait_machine_locked(world, entity_id) {
                Some(OFFLINE_LABEL.into())
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
        TRAIT_STRONG_METABOLISM => Some("25% less radiation damage, 50% less toxin damage"),
        TRAIT_POWER_PSI => Some("No burnout HP damage; failed casts still cost psi"),
        TRAIT_SPEEDY => Some("15% faster movement"),
        TRAIT_CYBER_ASSIMILATION => Some("Robots drop diagnostic modules that heal 15 HP"),
        TRAIT_SECURITY_EXPERT => Some("+2 effective Hack at security computers"),
        TRAIT_SPATIALLY_AWARE => Some("Full map layout on every deck"),
        TRAIT_SHARPSHOOTER => Some("35% more ranged damage (classic retail behavior)"),
        TRAIT_CYBERNETICALLY_ENHANCED => Some("Two distinct implants equipped simultaneously"),
        TRAIT_SMASHER => Some("Hold trigger 380 ms for +6 melee base damage"),
        TRAIT_LETHAL_WEAPON => Some("35% more melee damage"),
        TRAIT_TINKER => Some("Half-price weapon modification attempts"),
        TRAIT_TANK => Some("+5 max hit points"),
        TRAIT_NATURALLY_ABLE => Some("+8 cyber modules"),
        TRAIT_PACK_RAT => Some("+3 pack slots"),
        TRAIT_PHARMO_FRIENDLY => Some("20% healing, psi and hazard-item bonus"),
        TRAIT_REPLICATOR_EXPERT => Some("20% replicator discount"),
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
    fn hover_help_stays_in_its_well_and_does_not_include_stale_feedback() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let description = "Cybernetically Enhanced: An extra implant may be installed.";
        world.add_unique(TraitsContext {
            descriptions: std::array::from_fn(|_| description.to_owned()),
            header_label: FALLBACK_HEADER_LABEL.to_owned(),
            used_label: FALLBACK_USED_LABEL.to_owned(),
        });
        let machine = world.add_entity((dark::properties::PropTemplateId { template_id: 133 },));
        let components = TraitGui.get_components(
            &Some(GuiCursor {
                position: point2(MATRIX_X + 1.0, MATRIX_Y + 1.0),
                held_entity_id: None,
            }),
            machine,
            &world,
            &TraitGuiState {
                message: Some("Stale refusal".to_owned()),
            },
        );
        let mut description_lines = Vec::new();
        for component in &components {
            if let GuiComponent::Text {
                text,
                position,
                size,
                font,
                ..
            } = component
            {
                assert_ne!(text, "Stale refusal");
                if position.y >= DESC_Y && position.y < DESC_Y_MAX {
                    assert_eq!(font, crate::ui::MFD_FONT);
                    assert!(position.x + size.x <= 174.0);
                    assert!(position.y + size.y <= DESC_Y_MAX);
                    description_lines.push(text.as_str());
                }
            }
        }
        assert_eq!(description_lines.join(" "), description);
    }

    #[test]
    fn offline_station_refuses_supported_trait_until_unlocked() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let machine = world.add_entity((
            dark::properties::PropTemplateId { template_id: 60301 },
            dark::properties::PropLocked(true),
        ));
        let (state, effect) = TraitGui.handle_msg(
            machine,
            &world,
            &TraitGuiState::default(),
            &TraitGuiMsg::Pick(TRAIT_TANK),
        );
        assert!(matches!(effect, Effect::NoEffect));
        assert_eq!(state.message.as_deref(), Some(OFFLINE_LABEL));
        world.add_component(machine, dark::properties::PropLocked(false));
        let (_, effect) =
            TraitGui.handle_msg(machine, &world, &state, &TraitGuiMsg::Pick(TRAIT_TANK));
        assert!(matches!(effect, Effect::AcquireOsTrait { .. }));
    }

    #[test]
    fn installed_traits_leave_empty_grid_cells_without_hover_help() {
        let mut world = World::new();
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().add_os_trait(TRAIT_TANK);
        world.add_unique(quests);
        let machine = world.add_entity((dark::properties::PropTemplateId { template_id: 133 },));
        let components = TraitGui.get_components(
            &Some(GuiCursor {
                position: point2(
                    MATRIX_X + MATRIX_PITCH_X * 3.0 + 1.0,
                    MATRIX_Y + MATRIX_PITCH_Y + 1.0,
                ),
                held_entity_id: None,
            }),
            machine,
            &world,
            &TraitGuiState::default(),
        );
        assert_eq!(
            components
                .iter()
                .filter(|c| matches!(c, GuiComponent::Button { .. }))
                .count(),
            15
        );
        assert!(!components.iter().any(|c| matches!(c,
            GuiComponent::Text { position, .. } if position.y >= DESC_Y)));
    }

    #[test]
    fn installed_icon_exposes_its_description() {
        let mut world = World::new();
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().add_os_trait(TRAIT_TANK);
        world.add_unique(quests);
        let machine = world.add_entity((dark::properties::PropTemplateId { template_id: 133 },));
        let components = TraitGui.get_components(
            &Some(GuiCursor {
                position: point2(OWNED_X + 1.0, OWNED_Y + 1.0),
                held_entity_id: None,
            }),
            machine,
            &world,
            &TraitGuiState::default(),
        );
        assert!(components.iter().any(|component| matches!(
            component, GuiComponent::Text { text, position, .. }
                if text == "Tank" && position.y == DESC_Y
        )));
    }

    #[test]
    fn invalid_trait_does_not_consume_the_machine() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let machine = world.add_entity((dark::properties::PropTemplateId { template_id: 133 },));

        let (state, effect) = TraitGui.handle_msg(
            machine,
            &world,
            &TraitGuiState::default(),
            &TraitGuiMsg::Pick(17), // Invalid trait IDs never consume a machine.
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
