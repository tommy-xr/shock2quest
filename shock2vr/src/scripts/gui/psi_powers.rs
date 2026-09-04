//! Psi power selection MFD - the panel the AMMOFULL readout's power badge
//! opens, and the psi-amp hand's lower face button opens in VR.
//!
//! The original's psi screen: a `PSI.PCX` backdrop under a five-tab tier strip
//! (`PSI1.PCX`..`PSI5.PCX`, one per browsed tier) over a 2x4 grid of
//! discipline icons, with a help panel underneath reading whatever the cursor
//! is over. Power ids are authored in blocks of eight per tier - `(tier-1)*8`
//! is the tier's neural-capacity marker (drawn, not selectable) and
//! `(tier-1)*8 + 1..7` are its seven disciplines - so the grid IS the id
//! block, cell by cell.
//!
//! Each icon ships as three variants, `{basename}_0/_1/_2`: untrained,
//! trained, and selected. The selection is the icon's own `_2` art, so there
//! is no separate highlight to keep in sync with it.
//!
//! The canvas is presentation-agnostic - placement is decided once here, in
//! panel pixels (AGENTS.md section 3) - and `FlatUiHost` presents that one
//! component list in flat's MFD slot and in VR's cyber-interface panel slot
//! alike. Neither presentation draws a close button of its own: the host
//! already carries one beside the panel slot.
//!
//! The *browsed* tier is not GUI state. It lives in the world, as
//! [`crate::psi::PsiPanelTier`], because the mission has to snap it whenever
//! the selection moves under the panel (a stick flick, `CyclePsiPower`), and a
//! GUI's own state is reachable only from its own messages.

use cgmath::{Vector2, Vector3, vec2};
use shipyard::{EntityId, UniqueView, World};

use crate::gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::psi::{
    GlobalPsiPowers, POWERS_PER_TIER, PlayerPsiKnownPowers, PsiPanelTier, PsiPowerSelection,
};
use crate::scripts::Effect;
use crate::ui::Rect;

/// `PSI.PCX`'s authored size. The panel is drawn at the art's own size, which
/// is four pixels shorter than the settings MFD's 188x300 backdrop.
const PANEL_W: f32 = 188.0;
const PANEL_H: f32 = 296.0;

/// How many discipline tiers the screen pages through.
pub const TIERS: i32 = 5;

/// The tier strip's art (`PSI<tier>.PCX`, 144x24) and the five tabs cut out of
/// it. The art is drawn at its native size; the clickable strip is the row of
/// tabs inside it, which is what the backdrop's five raised keys measure.
const TIER_STRIP_POS: (f32, f32) = (32.0, 11.0);
const TIER_STRIP_SIZE: (f32, f32) = (144.0, 24.0);
const TIER_STRIP: Rect = Rect::new(32.0, 10.0, 142.0, 18.0);

/// Left edge of each column. A point is in column 1 iff its x is at or past
/// column 1's origin - a half-plane, so the gutter between the icons still
/// belongs to a column rather than swallowing clicks.
const COLUMN_X: [f32; 2] = [36.0, 105.0];
/// The first row's top edge, and the row pitch.
const ROW_Y: f32 = 34.0;
const ROW_H: f32 = 30.0;
const ROWS: usize = 4;
/// An icon's authored size (`PICN<nn>_<kind>.PCX`).
const ICON_SIZE: (f32, f32) = (66.0, 30.0);

/// The help text panel, under the grid.
const HELP: Rect = Rect::new(15.0, 162.0, 159.0, 102.0);
const LINE_H: f32 = 11.0;
/// Approximate characters per line at the help width, the same conservative
/// greedy-wrap budget the log reader uses (26 chars at 136 px), scaled up.
const HELP_WRAP: usize = 30;

const BACKDROP: &str = "iface/psi.pcx";

/// `psihelp.str` keys: the icon basename for a power id, the power's own help
/// text (line 1 is its discipline name), and the tier strip's help line.
const TIER_STRIP_HELP_KEY: &str = "buytext0";

/// `/v1/ui` labels, so a client clicks a tab or a power by meaning rather than
/// by pixel: `psi_tier_<n>` and `psi_power_<id>`.
fn tier_tab_label(tier: i32) -> String {
    format!("psi_tier_{tier}")
}
fn power_cell_label(power_id: i32) -> String {
    format!("psi_power_{power_id}")
}

/// A tab's rect: five equal slices of [`TIER_STRIP`]. Hit-testing inverts
/// this, and the panel's buttons are drawn at it, so hover and click are the
/// same region by construction.
fn tab_rect(tab: i32) -> Rect {
    let width = TIER_STRIP.w / TIERS as f32;
    Rect::new(
        TIER_STRIP.x + tab as f32 * width,
        TIER_STRIP.y,
        width,
        TIER_STRIP.h,
    )
}

/// The tier strip tab under a panel-local point, if any.
fn tab_at(x: f32, y: f32) -> Option<i32> {
    if !TIER_STRIP.contains_half_open(vec2(x, y)) {
        return None;
    }
    let tab = ((x - TIER_STRIP.x) / (TIER_STRIP.w / TIERS as f32)).floor() as i32;
    Some(tab.clamp(0, TIERS - 1) + 1)
}

/// The grid cell (column, row) under a panel-local point, if any.
///
/// The authored column split is a half-plane at column 1's origin, and the rows
/// are a fixed pitch - but a point in the gutter beside an icon is not on any
/// cell, so the candidate is confirmed against the icon's own [`cell_rect`].
/// That is the rect the cell's button is drawn at, so the hover help and the
/// click can never disagree about which cell is under the cursor (AGENTS.md
/// section 3: one placement, not two).
fn cell_at(x: f32, y: f32) -> Option<(usize, usize)> {
    let column = if x >= COLUMN_X[1] { 1 } else { 0 };
    let row = ((y - ROW_Y) / ROW_H).floor();
    if row < 0.0 || row as usize >= ROWS {
        return None;
    }
    let cell = (column, row as usize);
    cell_rect(cell.0, cell.1)
        .contains_half_open(vec2(x, y))
        .then_some(cell)
}

/// The power id a cell of `tier` shows. Reading order is column-major: the
/// left column top-to-bottom, then the right, so cell (0,0) is the block's
/// first id - the tier's capacity marker.
fn power_id_at_cell(tier: i32, column: usize, row: usize) -> i32 {
    (tier - 1) * POWERS_PER_TIER + (column * ROWS + row) as i32
}

/// Where a cell's icon is drawn.
fn cell_rect(column: usize, row: usize) -> Rect {
    Rect::new(
        COLUMN_X[column],
        ROW_Y + row as f32 * ROW_H,
        ICON_SIZE.0,
        ICON_SIZE.1,
    )
}

/// Whether a power id is a tier's capacity marker rather than a discipline.
/// Markers are drawn but never selectable - there is no power behind them.
fn is_tier_marker(power_id: i32) -> bool {
    power_id % POWERS_PER_TIER == 0
}

/// The icon variant a cell draws: `_2` selected, `_1` trained, `_0` otherwise.
fn icon_kind(trained: bool, selected: bool) -> u8 {
    if selected {
        2
    } else if trained {
        1
    } else {
        0
    }
}

/// A power id's icon basename, from `psihelp.str`'s `psiicon<id>` entry.
///
/// The ids and the art files are NOT the same numbering - power 6's icon is
/// `picn07`, power 7's is `picn21` - so the positional fallback is a
/// last-resort guess for a data install missing the entry, right for some ids
/// and wrong for others. Every shipped install carries all forty entries, so it
/// does not fire in practice.
fn icon_basename(strings: &std::collections::HashMap<String, String>, power_id: i32) -> String {
    strings
        .get(&format!("psiicon{power_id}"))
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("picn{power_id:02}"))
}

/// The full texture path for a power's icon.
fn icon_texture(basename: &str, kind: u8) -> String {
    format!("iface/{basename}_{kind}.pcx")
}

// --- Stick navigation -------------------------------------------------------

/// A step the captured thumbstick asks for while the panel is docked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavStep {
    TierPrev,
    TierNext,
    PowerPrev,
    PowerNext,
}

impl NavStep {
    /// The selection step this flick means. Up/down walk tiers, left/right
    /// walk the powers inside one - the same two axes the flat readout's four
    /// arrows drive, so the stick and the arrows cannot diverge.
    pub fn effect(self) -> Effect {
        use crate::psi::PsiSelectionAxis::{Power, Tier};
        let (axis, forward) = match self {
            NavStep::TierPrev => (Tier, false),
            NavStep::TierNext => (Tier, true),
            NavStep::PowerPrev => (Power, false),
            NavStep::PowerNext => (Power, true),
        };
        Effect::StepPsiSelection { axis, forward }
    }
}

/// How far the stick must be pushed to register a step, and how far it must
/// come back before the next one. The gap is hysteresis: a stick resting just
/// under the threshold cannot chatter through the whole tier.
const NAV_ENGAGE: f32 = 0.5;
const NAV_RELEASE: f32 = 0.3;

/// Edge-triggered stick navigation: one step per flick, whatever the frame
/// rate. Returns the step (if this frame is the rising edge) and the new latch
/// state. The dominant axis wins, so a diagonal push is never both.
pub fn stick_nav(stick: Vector2<f32>, latched: bool) -> (Option<NavStep>, bool) {
    let (x, y) = (stick.x, stick.y);
    let magnitude = x.abs().max(y.abs());
    if latched {
        return (None, magnitude >= NAV_RELEASE);
    }
    if magnitude < NAV_ENGAGE {
        return (None, false);
    }
    let step = if y.abs() >= x.abs() {
        if y > 0.0 {
            NavStep::TierNext
        } else {
            NavStep::TierPrev
        }
    } else if x > 0.0 {
        NavStep::PowerNext
    } else {
        NavStep::PowerPrev
    };
    (Some(step), true)
}

// --- The panel --------------------------------------------------------------

pub struct PsiPowersGui;

#[derive(Clone, Debug, Default)]
pub struct PsiPowersGuiState;

#[derive(Clone, Debug)]
pub enum PsiPowersGuiMsg {
    /// Browse a tier. Changes nothing the amp will cast.
    BrowseTier(i32),
    /// Select the power with this id, if the player is trained in it.
    SelectPower(i32),
}

/// The strings the panel reads: the whole `psihelp.str` table, kept as a
/// unique because the panel has no asset cache at draw time.
#[derive(shipyard::Unique, Clone, Default)]
pub struct GlobalPsiStrings(pub std::collections::HashMap<String, String>);

/// The registry index of a power id, if the gamesys authors one. Tier markers
/// have none - they are art, not powers.
pub fn power_index(powers: &[crate::psi::PsiPowerInfo], power_id: i32) -> Option<usize> {
    powers.iter().position(|p| p.power.power_id == power_id)
}

impl Gui<PsiPowersGuiState, PsiPowersGuiMsg> for PsiPowersGui {
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        _state: &PsiPowersGuiState,
    ) -> Vec<GuiComponent<PsiPowersGuiMsg>> {
        // Archive-qualified for the same reason the settings MFD qualifies its
        // backdrop: obj.crf and iface.crf collide on plain basenames.
        let mut components: Vec<GuiComponent<PsiPowersGuiMsg>> = vec![
            gui::image(BACKDROP)
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        let (Ok(powers), Ok(known), Ok(selection), Ok(browsed)) = (
            world.borrow::<UniqueView<GlobalPsiPowers>>(),
            world.borrow::<UniqueView<PlayerPsiKnownPowers>>(),
            world.borrow::<UniqueView<PsiPowerSelection>>(),
            world.borrow::<UniqueView<PsiPanelTier>>(),
        ) else {
            return components;
        };
        // Borrowed, never cloned: the table is the whole ~120-entry
        // `psihelp.str`, and this runs every frame the panel is visible.
        let strings_view = world.borrow::<UniqueView<GlobalPsiStrings>>();
        let no_strings = std::collections::HashMap::new();
        let strings = strings_view.as_ref().map_or(&no_strings, |s| &s.0);

        let tier = browsed.0.clamp(1, TIERS);
        let selected_id = powers.0.get(selection.index).map(|p| p.power.power_id);

        // The tier strip: one piece of art per browsed tier, over five
        // invisible tabs. A zero-alpha button is the established "hit target
        // over backdrop art" convention - the tabs are painted into the art,
        // so they must be clickable without painting anything over them.
        components.push(
            gui::image(&format!("iface/psi{tier}.pcx"))
                .with_position(vec2(TIER_STRIP_POS.0, TIER_STRIP_POS.1))
                .with_size(vec2(TIER_STRIP_SIZE.0, TIER_STRIP_SIZE.1)),
        );
        for tab in 0..TIERS {
            let rect = tab_rect(tab);
            components.push(
                gui::button(PsiPowersGuiMsg::BrowseTier(tab + 1))
                    .with_image(BACKDROP)
                    .with_alpha(0.0)
                    .with_label(&tier_tab_label(tab + 1))
                    .with_position(vec2(rect.x, rect.y))
                    .with_size(vec2(rect.w, rect.h)),
            );
        }

        // The grid. Every cell draws its icon; only a real power is clickable,
        // so the tier marker is art and nothing else.
        for column in 0..2usize {
            for row in 0..ROWS {
                let power_id = power_id_at_cell(tier, column, row);
                let rect = cell_rect(column, row);
                // The tier's capacity marker is art: it names no discipline,
                // so it is drawn and never made clickable.
                let index = if is_tier_marker(power_id) {
                    None
                } else {
                    power_index(&powers.0, power_id)
                };
                let trained = match index {
                    Some(index) => known.0.contains(&powers.0[index].template_id),
                    // A tier's capacity marker has no power behind it; it reads
                    // as earned once anything in the tier is trained.
                    None => powers
                        .0
                        .iter()
                        .any(|p| p.tier() == tier && known.0.contains(&p.template_id)),
                };
                let kind = icon_kind(trained, Some(power_id) == selected_id);
                components.push(
                    gui::image(&icon_texture(&icon_basename(strings, power_id), kind))
                        .with_position(vec2(rect.x, rect.y))
                        .with_size(vec2(rect.w, rect.h)),
                );
                if index.is_some() {
                    components.push(
                        gui::button(PsiPowersGuiMsg::SelectPower(power_id))
                            .with_image(BACKDROP)
                            .with_alpha(0.0)
                            .with_label(&power_cell_label(power_id))
                            .with_position(vec2(rect.x, rect.y))
                            .with_size(vec2(rect.w, rect.h)),
                    );
                }
            }
        }

        // The help panel reads whatever the cursor is over - a discipline's
        // own entry, or the strip's one line about the tabs.
        if let Some(help) = cursor
            .as_ref()
            .and_then(|cursor| help_key(tier, cursor.position.x, cursor.position.y))
            .and_then(|key| strings.get(&key))
        {
            let lines = (HELP.h / LINE_H).floor() as usize;
            for (idx, line) in super::media::wrap_text(help, HELP_WRAP)
                .iter()
                .take(lines)
                .enumerate()
            {
                if line.is_empty() {
                    continue;
                }
                components.push(
                    gui::text(line)
                        .with_position(vec2(HELP.x, HELP.y + idx as f32 * LINE_H))
                        .with_size(vec2(HELP.w, LINE_H)),
                );
            }
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(PANEL_W, PANEL_H),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        _world: &World,
        state: &PsiPowersGuiState,
        msg: &PsiPowersGuiMsg,
    ) -> (PsiPowersGuiState, Effect) {
        let effect = match msg {
            PsiPowersGuiMsg::BrowseTier(tier) => Effect::SetPsiBrowsedTier { tier: *tier },
            // The trained-only guard is the effect's, not the panel's: the same
            // rule has to hold for every way in (a click, the stick, HTTP).
            PsiPowersGuiMsg::SelectPower(power_id) => Effect::SelectPsiPower {
                power_id: *power_id,
            },
        };
        (state.clone(), effect)
    }
}

/// The `psihelp.str` key describing what is under a panel-local point: a
/// discipline's own entry over a cell, the tabs' one line over the strip.
fn help_key(tier: i32, x: f32, y: f32) -> Option<String> {
    if tab_at(x, y).is_some() {
        return Some(TIER_STRIP_HELP_KEY.to_owned());
    }
    let (column, row) = cell_at(x, y)?;
    Some(format!("psi{}", power_id_at_cell(tier, column, row)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The authored grid region the eight cells are cut from. Nothing draws or
    /// hit-tests it - the cells are the hit targets - but every cell must stay
    /// inside it, which is what this asserts against.
    const GRID: Rect = Rect::new(32.0, 32.0, 142.0, 125.0);
    use crate::psi::PsiPowerInfo;
    use dark::properties::PropPsiPower;
    use std::collections::{HashMap, HashSet};

    /// A registry in the shape the gamesys authors: seven disciplines per
    /// tier at ids `(tier-1)*8 + 1..7`.
    fn registry() -> Vec<PsiPowerInfo> {
        let mut powers = Vec::new();
        for tier in 1..=TIERS {
            for slot in 1..POWERS_PER_TIER {
                let power_id = (tier - 1) * POWERS_PER_TIER + slot;
                powers.push(PsiPowerInfo {
                    template_id: -1000 - power_id,
                    name: format!("Power {power_id}"),
                    display_name: None,
                    power: PropPsiPower {
                        power_id,
                        activation_type: 0,
                        psi_cost: tier,
                        data: [0.0; 4],
                    },
                    projectiles: Vec::new(),
                    overloadable: false,
                    duration: None,
                });
            }
        }
        powers
    }

    fn world_with(selected_power_id: i32, browsed_tier: i32, trained: &[i32]) -> World {
        let powers = registry();
        let known: HashSet<i32> = powers
            .iter()
            .filter(|p| trained.contains(&p.power.power_id))
            .map(|p| p.template_id)
            .collect();
        let index = power_index(&powers, selected_power_id).unwrap();
        let world = World::new();
        world.add_unique(GlobalPsiPowers(powers));
        world.add_unique(PlayerPsiKnownPowers(known));
        world.add_unique(PsiPowerSelection { index });
        world.add_unique(PsiPanelTier(browsed_tier));
        world.add_unique(GlobalPsiStrings(HashMap::from([
            ("psiicon1".to_owned(), "picn01".to_owned()),
            (
                "psi1".to_owned(),
                "Some Discipline\n\nIt does a thing.".to_owned(),
            ),
            (
                TIER_STRIP_HELP_KEY.to_owned(),
                "These buttons switch between the different Tiers.".to_owned(),
            ),
        ])));
        world
    }

    fn components(world: &World) -> Vec<GuiComponent<PsiPowersGuiMsg>> {
        PsiPowersGui.get_components(&None, EntityId::dead(), world, &PsiPowersGuiState)
    }

    fn images(components: &[GuiComponent<PsiPowersGuiMsg>]) -> Vec<(String, Vector2<f32>)> {
        components
            .iter()
            .filter_map(|c| match c {
                GuiComponent::Image {
                    texture, position, ..
                } => Some((texture.clone(), *position)),
                _ => None,
            })
            .collect()
    }

    fn labels(components: &[GuiComponent<PsiPowersGuiMsg>]) -> Vec<String> {
        components
            .iter()
            .filter_map(|c| match c {
                GuiComponent::Button { label, .. } => label.clone(),
                _ => None,
            })
            .collect()
    }

    fn texts(components: &[GuiComponent<PsiPowersGuiMsg>]) -> String {
        components
            .iter()
            .filter_map(|c| match c {
                GuiComponent::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Every drawn element stays inside the panel, and the help panel clears
    /// the grid above it.
    #[test]
    fn the_layout_fits_inside_the_panel() {
        for rect in [TIER_STRIP, GRID, HELP] {
            assert!(rect.x >= 0.0 && rect.x + rect.w <= PANEL_W, "{rect:?}");
            assert!(rect.y >= 0.0 && rect.y + rect.h <= PANEL_H, "{rect:?}");
        }
        assert!(GRID.y + GRID.h <= HELP.y, "the help panel clears the grid");
        for column in 0..2usize {
            let last = cell_rect(column, ROWS - 1);
            assert!(
                last.y + last.h <= GRID.y + GRID.h,
                "row {} spills",
                ROWS - 1
            );
            assert!(
                cell_rect(column, 0).x + ICON_SIZE.0 <= GRID.x + GRID.w,
                "column {column} spills"
            );
        }
        // The two columns cannot overlap, or one icon would cover the other.
        assert!(COLUMN_X[0] + ICON_SIZE.0 <= COLUMN_X[1]);
    }

    #[test]
    fn a_tab_is_picked_by_where_the_strip_was_clicked() {
        let y = TIER_STRIP.y + 1.0;
        assert_eq!(tab_at(TIER_STRIP.x, y), Some(1));
        assert_eq!(tab_at(TIER_STRIP.x + TIER_STRIP.w - 0.5, y), Some(5));
        // Each of the five tabs is reachable, in order, at its own center.
        let tab_w = TIER_STRIP.w / TIERS as f32;
        for tab in 0..TIERS {
            let x = TIER_STRIP.x + (tab as f32 + 0.5) * tab_w;
            assert_eq!(tab_at(x, y), Some(tab + 1), "tab {tab}");
        }
        // Outside the strip is not a tab at all.
        assert_eq!(tab_at(TIER_STRIP.x - 1.0, y), None);
        assert_eq!(
            tab_at(TIER_STRIP.x + 1.0, TIER_STRIP.y + TIER_STRIP.h),
            None
        );
    }

    /// The column comes from the authored half-plane and the row from the
    /// pitch, but the hit is then confirmed against the icon's own rect - so a
    /// point in the gutter beside an icon is on no cell at all.
    #[test]
    fn a_cell_is_picked_by_column_half_plane_and_row_pitch() {
        assert_eq!(cell_at(COLUMN_X[0], ROW_Y), Some((0, 0)));
        assert_eq!(cell_at(COLUMN_X[1], ROW_Y), Some((1, 0)));
        for row in 0..ROWS {
            let y = ROW_Y + row as f32 * ROW_H + 1.0;
            assert_eq!(cell_at(COLUMN_X[1] + 1.0, y), Some((1, row)), "row {row}");
        }
        // The gutter between the two columns is nobody's cell.
        assert_eq!(cell_at(COLUMN_X[0] + ICON_SIZE.0, ROW_Y), None);
        assert_eq!(cell_at(COLUMN_X[1] - 0.5, ROW_Y), None);
        // Above the first row and outside the grid entirely.
        assert_eq!(cell_at(COLUMN_X[0], GRID.y), None);
        assert_eq!(cell_at(GRID.x - 1.0, ROW_Y), None);
        assert_eq!(cell_at(COLUMN_X[0], GRID.y + GRID.h), None);
    }

    /// One hit test, not two: the region that shows a cell's help is exactly
    /// the region whose click selects it (AGENTS.md section 3 - placement
    /// decided once). The single exception is the tier's capacity marker, which
    /// has a `psihelp.str` entry of its own but no discipline to select, so it
    /// reads out under the cursor and stays unclickable.
    #[test]
    fn the_hover_and_the_click_agree_everywhere_on_the_panel() {
        let world = world_with(1, 1, &[1, 2, 3, 4, 5, 6, 7]);
        let components = components(&world);
        let button_at = |x: f32, y: f32| -> Option<String> {
            components
                .iter()
                .rev()
                .find_map(|c| match c {
                    GuiComponent::Button {
                        label,
                        position,
                        size,
                        ..
                    } => Rect::new(position.x, position.y, size.x, size.y)
                        .contains_half_open(vec2(x, y))
                        .then(|| label.clone()),
                    _ => None,
                })
                .flatten()
        };
        // Sample the whole panel on a 2px lattice.
        let mut x = 0.0;
        while x < PANEL_W {
            let mut y = 0.0;
            while y < PANEL_H {
                let hovered = help_key(1, x, y).map(|key| {
                    if key == TIER_STRIP_HELP_KEY {
                        "tab".to_owned()
                    } else {
                        key
                    }
                });
                if hovered.as_deref() == Some(&format!("psi{}", power_id_at_cell(1, 0, 0))) {
                    assert_eq!(button_at(x, y), None, "the marker is never clickable");
                    y += 2.0;
                    continue;
                }
                let clicked = button_at(x, y).map(|label| {
                    if label.starts_with("psi_tier_") {
                        "tab".to_owned()
                    } else {
                        format!("psi{}", label.trim_start_matches("psi_power_"))
                    }
                });
                assert_eq!(hovered, clicked, "at ({x}, {y})");
                y += 2.0;
            }
            x += 2.0;
        }
    }

    /// The grid is the tier's id block, read column-major, and cell (0,0) is
    /// the block's capacity marker.
    #[test]
    fn the_grid_is_the_tiers_id_block() {
        assert_eq!(power_id_at_cell(1, 0, 0), 0);
        assert_eq!(power_id_at_cell(1, 0, 3), 3);
        assert_eq!(power_id_at_cell(1, 1, 0), 4);
        assert_eq!(power_id_at_cell(1, 1, 3), 7);
        assert_eq!(power_id_at_cell(5, 0, 0), 32);
        assert_eq!(power_id_at_cell(5, 1, 3), 39);
        assert!(is_tier_marker(power_id_at_cell(3, 0, 0)));
        for tier in 1..=TIERS {
            for column in 0..2usize {
                for row in 0..ROWS {
                    let id = power_id_at_cell(tier, column, row);
                    assert!((column, row) == (0, 0) || !is_tier_marker(id), "{id}");
                }
            }
        }
    }

    /// The marker cell is drawn but carries no button, so it cannot be
    /// selected however precisely it is clicked.
    #[test]
    fn the_tier_marker_cell_is_not_selectable() {
        let world = world_with(1, 1, &[1, 2]);
        let labels = labels(&components(&world));
        assert!(!labels.contains(&power_cell_label(0)), "{labels:?}");
        for id in 1..POWERS_PER_TIER {
            assert!(labels.contains(&power_cell_label(id)), "power {id}");
        }
    }

    /// Untrained, trained, selected - one variant of the icon's own art each,
    /// with no separate highlight to fall out of sync.
    #[test]
    fn each_cell_draws_the_variant_matching_its_state() {
        assert_eq!(icon_kind(false, false), 0);
        assert_eq!(icon_kind(true, false), 1);
        assert_eq!(icon_kind(true, true), 2);

        let world = world_with(2, 1, &[1, 2]);
        let drawn = images(&components(&world));
        let at = |id: i32| {
            let (column, row) = (0..2usize)
                .flat_map(|c| (0..ROWS).map(move |r| (c, r)))
                .find(|(c, r)| power_id_at_cell(1, *c, *r) == id)
                .unwrap();
            let rect = cell_rect(column, row);
            drawn
                .iter()
                .find(|(_, pos)| *pos == vec2(rect.x, rect.y))
                .map(|(texture, _)| texture.clone())
                .unwrap()
        };
        assert_eq!(at(2), icon_texture("picn02", 2), "the selected power");
        assert_eq!(at(1), icon_texture("picn01", 1), "a trained power");
        assert_eq!(at(3), icon_texture("picn03", 0), "an untrained power");
    }

    /// The icon basename comes from the strings table where it has one - the
    /// power ids and the art files are not the same numbering.
    #[test]
    fn an_icon_basename_prefers_the_strings_table() {
        let strings = HashMap::from([("psiicon6".to_owned(), "picn07".to_owned())]);
        assert_eq!(icon_basename(&strings, 6), "picn07");
        assert_eq!(icon_basename(&strings, 11), "picn11");
        assert_eq!(icon_basename(&HashMap::new(), 4), "picn04");
    }

    /// The panel draws the tier it is told to browse, not the selection's.
    #[test]
    fn the_strip_art_and_the_grid_follow_the_browsed_tier() {
        let world = world_with(1, 4, &[1]);
        let drawn = images(&components(&world));
        assert!(
            drawn.iter().any(|(t, _)| t == "iface/psi4.pcx"),
            "{drawn:?}"
        );
        let labels = labels(&components(&world));
        assert!(labels.contains(&power_cell_label(25)), "{labels:?}");
        assert!(!labels.contains(&power_cell_label(1)));
    }

    /// The help panel reads what the cursor is over - a discipline's entry, or
    /// the strip's one line about the tabs.
    #[test]
    fn the_help_panel_reads_what_the_cursor_is_over() {
        assert_eq!(
            help_key(1, COLUMN_X[0] + 1.0, ROW_Y + ROW_H + 1.0),
            Some("psi1".to_owned())
        );
        assert_eq!(
            help_key(2, COLUMN_X[0] + 1.0, ROW_Y + ROW_H + 1.0),
            Some("psi9".to_owned())
        );
        assert_eq!(
            help_key(1, TIER_STRIP.x + 1.0, TIER_STRIP.y + 1.0),
            Some(TIER_STRIP_HELP_KEY.to_owned())
        );
        assert_eq!(help_key(1, 2.0, 280.0), None);

        let world = world_with(1, 1, &[1]);
        let over_power = Some(GuiCursor {
            position: cgmath::Point2::new(COLUMN_X[0] + 1.0, ROW_Y + ROW_H + 1.0),
            held_entity_id: None,
        });
        let drawn =
            PsiPowersGui.get_components(&over_power, EntityId::dead(), &world, &PsiPowersGuiState);
        assert!(
            texts(&drawn).contains("Some Discipline"),
            "{:?}",
            texts(&drawn)
        );
        // Nothing hovered draws no help at all.
        assert!(texts(&components(&world)).is_empty());
    }

    #[test]
    fn clicking_a_tab_browses_and_clicking_a_power_selects() {
        let world = world_with(1, 1, &[1]);
        let (_, effect) = PsiPowersGui.handle_msg(
            EntityId::dead(),
            &world,
            &PsiPowersGuiState,
            &PsiPowersGuiMsg::BrowseTier(3),
        );
        assert!(matches!(effect, Effect::SetPsiBrowsedTier { tier: 3 }));

        let (_, effect) = PsiPowersGui.handle_msg(
            EntityId::dead(),
            &world,
            &PsiPowersGuiState,
            &PsiPowersGuiMsg::SelectPower(5),
        );
        assert!(matches!(effect, Effect::SelectPsiPower { power_id: 5 }));
    }

    /// One step per flick: holding the stick over cannot walk the whole tier,
    /// and the stick has to come back past the release threshold first.
    #[test]
    fn stick_navigation_is_edge_triggered_with_hysteresis() {
        let (step, latched) = stick_nav(vec2(0.0, 1.0), false);
        assert_eq!(step, Some(NavStep::TierNext));
        assert!(latched);
        // Held: nothing more happens, and the latch survives.
        assert_eq!(stick_nav(vec2(0.0, 1.0), true), (None, true));
        // Released past the hysteresis gap: unlatched, still no step.
        assert_eq!(stick_nav(vec2(0.0, 0.4), true), (None, true));
        assert_eq!(stick_nav(vec2(0.0, 0.0), true), (None, false));
        // ...and only then does the next flick register.
        assert_eq!(stick_nav(vec2(0.0, -1.0), false).0, Some(NavStep::TierPrev));
    }

    #[test]
    fn the_dominant_stick_axis_decides_the_step() {
        assert_eq!(stick_nav(vec2(1.0, 0.0), false).0, Some(NavStep::PowerNext));
        assert_eq!(
            stick_nav(vec2(-1.0, 0.0), false).0,
            Some(NavStep::PowerPrev)
        );
        // A diagonal is one step, never two: the larger component wins.
        assert_eq!(stick_nav(vec2(0.9, 0.6), false).0, Some(NavStep::PowerNext));
        assert_eq!(stick_nav(vec2(0.6, 0.9), false).0, Some(NavStep::TierNext));
        // Below the engage threshold nothing happens at all - a resting stick
        // must not browse.
        assert_eq!(stick_nav(vec2(0.4, 0.4), false), (None, false));
    }

    /// The stick drives the same two axes the readout's arrows do.
    #[test]
    fn a_nav_step_is_a_selection_step_on_the_matching_axis() {
        use crate::psi::PsiSelectionAxis;
        assert!(matches!(
            NavStep::TierNext.effect(),
            Effect::StepPsiSelection {
                axis: PsiSelectionAxis::Tier,
                forward: true
            }
        ));
        assert!(matches!(
            NavStep::PowerPrev.effect(),
            Effect::StepPsiSelection {
                axis: PsiSelectionAxis::Power,
                forward: false
            }
        ));
    }
}
