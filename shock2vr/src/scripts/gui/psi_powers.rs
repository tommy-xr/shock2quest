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
pub(crate) fn icon_kind(trained: bool, selected: bool) -> u8 {
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
pub(crate) fn icon_basename(
    strings: &std::collections::HashMap<String, String>,
    power_id: i32,
) -> String {
    strings
        .get(&format!("psiicon{power_id}"))
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("picn{power_id:02}"))
}

/// The full texture path for a power's icon.
pub(crate) fn icon_texture(basename: &str, kind: u8) -> String {
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
        let (axis, forward) = self.axis();
        Effect::StepPsiSelection { axis, forward }
    }

    pub fn axis(self) -> (crate::psi::PsiSelectionAxis, bool) {
        use crate::psi::PsiSelectionAxis::{Power, Tier};
        match self {
            NavStep::TierPrev => (Tier, false),
            NavStep::TierNext => (Tier, true),
            NavStep::PowerPrev => (Power, false),
            NavStep::PowerNext => (Power, true),
        }
    }
}

/// How far the stick must come back before it can step again. The gap below
/// the engage threshold is hysteresis: a stick resting just under it cannot
/// chatter through the whole tier.
const NAV_RELEASE: f32 = 0.3;
/// How far the stick must be pushed before [`stick_lean`] previews a step.
const LEAN_START: f32 = 0.25;

/// How a stick push turns into steps.
pub struct StickNavTuning {
    /// How far the stick must be pushed (its larger axis) to step.
    engage: f32,
    /// The smaller axis may be at most this fraction of the larger one; an
    /// ambiguous diagonal waits until it leans clearly one way.
    axis_slop: f32,
    /// `engage` and `axis_slop` for a push down.
    down: (f32, f32),
    /// Seconds after a step's release in which the opposite step is ignored.
    settle: f32,
}

/// The psi selectors. Engaging late lets a thumb's arc settle on its
/// direction; steps need ~30 degrees of a cardinal (tan 30 = 0.58); and a
/// flicked stick springs back past centre, which must not step backwards.
/// A thumb pulling down curls toward the palm and falls short, so down
/// engages sooner and takes ~40 degrees (tan 40 = 0.84).
pub const PSI_STICK_NAV: StickNavTuning = StickNavTuning {
    engage: 0.75,
    axis_slop: 0.58,
    down: (0.65, 0.84),
    settle: 0.1,
};

/// GamePig games: the overworld walks on diagonals and the hoppers step as
/// fast as the thumb flicks.
pub const HANDHELD_STICK_NAV: StickNavTuning = StickNavTuning {
    engage: 0.5,
    axis_slop: 1.0,
    down: (0.5, 1.0),
    settle: 0.0,
};

impl StickNavTuning {
    /// `(engage, axis_slop)` for a push toward `step`.
    fn limits(&self, step: NavStep) -> (f32, f32) {
        match step {
            NavStep::TierPrev => self.down,
            _ => (self.engage, self.axis_slop),
        }
    }
}

/// [`stick_nav`]'s state between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum NavLatch {
    #[default]
    Free,
    /// Pushed, after `Some(step)`: the stick must come back before it steps.
    Held(Option<NavStep>),
    /// Released after a step; for the remaining seconds its opposite is ignored.
    Settling(NavStep, f32),
}

impl NavStep {
    fn opposite(self) -> Self {
        match self {
            NavStep::TierPrev => NavStep::TierNext,
            NavStep::TierNext => NavStep::TierPrev,
            NavStep::PowerPrev => NavStep::PowerNext,
            NavStep::PowerNext => NavStep::PowerPrev,
        }
    }
}

/// Edge-triggered stick navigation: one step per flick, whatever the frame
/// rate. Returns the step (if this frame is the rising edge) and the new latch
/// state. A push steps along one cardinal, never two.
pub fn stick_nav(
    stick: Vector2<f32>,
    latch: NavLatch,
    dt: f32,
    tuning: &StickNavTuning,
) -> (Option<NavStep>, NavLatch) {
    let (x, y) = (stick.x.abs(), stick.y.abs());
    let magnitude = x.max(y);
    let latch = match latch {
        NavLatch::Held(_) if magnitude >= NAV_RELEASE => return (None, latch),
        NavLatch::Held(Some(step)) if tuning.settle > 0.0 => {
            return (None, NavLatch::Settling(step, tuning.settle));
        }
        NavLatch::Held(_) => return (None, NavLatch::Free),
        NavLatch::Settling(step, left) if left > dt => NavLatch::Settling(step, left - dt),
        NavLatch::Settling(..) | NavLatch::Free => NavLatch::Free,
    };
    match aimed_step(stick, latch, tuning) {
        Some(step) if magnitude >= tuning.limits(step).0 => {
            (Some(step), NavLatch::Held(Some(step)))
        }
        _ => (None, latch),
    }
}

/// The step a push is heading for and how far along it is (1 = about to
/// step), so a selector can preview it. `None` while the stick is still held
/// from a step, and for any push [`stick_nav`] would not step.
pub fn stick_lean(
    stick: Vector2<f32>,
    latch: NavLatch,
    tuning: &StickNavTuning,
) -> Option<(NavStep, f32)> {
    let magnitude = stick.x.abs().max(stick.y.abs());
    if matches!(latch, NavLatch::Held(_)) || magnitude <= LEAN_START {
        return None;
    }
    let step = aimed_step(stick, latch, tuning)?;
    let engage = tuning.limits(step).0;
    Some((
        step,
        ((magnitude - LEAN_START) / (engage - LEAN_START)).min(1.0),
    ))
}

/// The step an unlatched push points at, whatever its length: `None` for an
/// ambiguous diagonal or a spring-back the latch is ignoring.
fn aimed_step(stick: Vector2<f32>, latch: NavLatch, tuning: &StickNavTuning) -> Option<NavStep> {
    let (x, y) = (stick.x.abs(), stick.y.abs());
    let step = dominant_step(stick);
    if x.min(y) > tuning.limits(step).1 * x.max(y) {
        return None;
    }
    match latch {
        NavLatch::Settling(last, _) if step == last.opposite() => None,
        _ => Some(step),
    }
}

/// Up/down walk tiers, left/right powers; a tie goes to the tiers.
fn dominant_step(stick: Vector2<f32>) -> NavStep {
    if stick.y.abs() >= stick.x.abs() {
        if stick.y > 0.0 {
            NavStep::TierNext
        } else {
            NavStep::TierPrev
        }
    } else if stick.x > 0.0 {
        NavStep::PowerNext
    } else {
        NavStep::PowerPrev
    }
}

// --- The panel --------------------------------------------------------------

pub struct PsiPowersGui;

/// The trainer reuses the selector's icon grid and help. Its tier browsing is
/// local, and clicking a discipline purchases it rather than selecting it.
pub struct PsiTrainerGui;

#[derive(Clone, Debug)]
pub struct PsiTrainerState {
    tier: i32,
    message: Option<String>,
}

impl Default for PsiTrainerState {
    fn default() -> Self {
        Self {
            tier: 1,
            message: None,
        }
    }
}

/// Only sell disciplines with a gameplay implementation. In particular,
/// setting a sustained status flag alone does not make that status useful.
fn purchasable_power(power: &crate::psi::PsiPowerInfo) -> bool {
    use crate::psi::*;
    if power.template_id == crate::psi_teleport::POWER {
        return true;
    }
    match power.power.activation_type {
        ACTIVATION_TYPE_INSTANT => matches!(
            power.template_id,
            PSI_HEAL_TEMPLATE_ID | CODEBREAKER_TEMPLATE_ID
        ),
        ACTIVATION_TYPE_SUSTAINED => {
            power.duration.is_some()
                && matches!(
                    power.template_id,
                    INVISO_TEMPLATE_ID
                        | BERSERK_TEMPLATE_ID
                        | STABILITY_TEMPLATE_ID
                        | -1107
                        | -1113
                        | -1114
                )
        }
        _ => !power.projectiles.is_empty(),
    }
}

/// Authored PSICOST column 0 buys the tier; columns 1..7 buy disciplines.
/// Shared by the display, input handler and authoritative effect applier.
pub(crate) fn psi_power_quote(world: &World, template_id: i32) -> Option<i32> {
    let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().ok()?;
    let power = powers.0.iter().find(|p| p.template_id == template_id)?;
    let tier = power.tier();
    let slot = power.power.power_id % POWERS_PER_TIER;
    if !(1..=TIERS).contains(&tier) || slot <= 0 || !purchasable_power(power) {
        return None;
    }
    let known = world.borrow::<UniqueView<PlayerPsiKnownPowers>>().ok()?;
    let quests = world
        .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
        .ok()?;
    if known.0.contains(&template_id) || quests.player_stats().psi_tier < tier {
        return None;
    }
    let costs = crate::difficulty::trainer_costs(world)?;
    let price = costs.psi_cost[(tier - 1) as usize][slot as usize];
    (price >= 0).then_some(price)
}

impl Gui<PsiTrainerState, PsiPowersGuiMsg> for PsiTrainerGui {
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        _entity: EntityId,
        world: &World,
        state: &PsiTrainerState,
    ) -> Vec<GuiComponent<PsiPowersGuiMsg>> {
        // The purchase panel has its own authored geometry (shktrpsi.cpp):
        // row-major icons at x=15/80, points at y=167, help at y=190.
        // The amp/character selector uses a different, column-major layout.
        let mut components = vec![
            gui::image("iface/psitrain.pcx")
                .with_size(vec2(PANEL_W, PANEL_H))
                .with_alpha(1.0),
        ];
        components.push(
            gui::image(&format!("iface/psi{}.pcx", state.tier))
                .with_rect(Rect::new(12.0, 11.0, 144.0, 24.0))
                .with_alpha(1.0),
        );
        for tab in 0..TIERS {
            components.push(
                gui::button(PsiPowersGuiMsg::BrowseTier(tab + 1))
                    .with_image("iface/psitrain.pcx")
                    .with_alpha(0.0)
                    .with_label(&tier_tab_label(tab + 1))
                    .with_rect(Rect::new(
                        12.0 + tab as f32 * 142.0 / 5.0,
                        10.0,
                        142.0 / 5.0,
                        18.0,
                    )),
            );
        }
        let quests = world
            .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
            .unwrap();
        let stats = quests.player_stats();
        let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().unwrap();
        let known = world.borrow::<UniqueView<PlayerPsiKnownPowers>>().unwrap();
        let strings_view = world.borrow::<UniqueView<GlobalPsiStrings>>();
        let empty = std::collections::HashMap::new();
        let strings = strings_view.as_ref().map_or(&empty, |s| &s.0);
        let costs = crate::difficulty::trainer_costs(world);
        let mut help = None;
        for slot in 0..POWERS_PER_TIER {
            let id = (state.tier - 1) * POWERS_PER_TIER + slot;
            let rect = Rect::new(
                15.0 + (slot % 2) as f32 * 65.0,
                34.0 + (slot / 2) as f32 * 30.0,
                66.0,
                30.0,
            );
            let power = power_index(&powers.0, id).map(|index| &powers.0[index]);
            let owned = if slot == 0 {
                stats.psi_tier >= state.tier
            } else {
                power.is_some_and(|p| known.0.contains(&p.template_id))
            };
            let quote = if slot == 0 {
                costs.as_ref().and_then(|c| {
                    super::upgrade_quote(c, stats, super::TrainerTarget::PsiTier(state.tier))
                })
            } else {
                power.and_then(|p| psi_power_quote(world, p.template_id))
            };
            let kind = if owned {
                2
            } else if quote.is_some_and(|cost| cost <= stats.cyber_modules) {
                1
            } else {
                0
            };
            components.push(
                gui::image(&icon_texture(&icon_basename(strings, id), kind))
                    .with_rect(rect)
                    .with_alpha(1.0),
            );
            // One-pixel artwork overlap between columns is not a second hit:
            // each button stops at the next column's start.
            components.push(
                gui::button(PsiPowersGuiMsg::SelectPower(id))
                    .with_image("iface/psitrain.pcx")
                    .with_alpha(0.0)
                    .with_label(&if slot == 0 {
                        format!("buy_psi_tier_{}", state.tier)
                    } else {
                        power_cell_label(id)
                    })
                    .with_rect(Rect { w: 65.0, ..rect }),
            );
            if !owned {
                let price = costs
                    .as_ref()
                    .map(|c| c.psi_cost[(state.tier - 1) as usize][slot as usize]);
                if let Some(price) = price.filter(|price| *price >= 0) {
                    components.push(super::PanelText::text(
                        &price.to_string(),
                        Rect::new(rect.x + 44.0, rect.y + 15.0, 21.0, 14.0),
                    ));
                }
            }
            if stats.psi_tier >= state.tier && kind == 1 {
                components.push(
                    gui::image("iface/psit10.pcx")
                        .with_rect(rect)
                        .with_alpha(1.0),
                );
            }
            if cursor.as_ref().is_some_and(|c| {
                Rect { w: 65.0, ..rect }.contains_half_open(vec2(c.position.x, c.position.y))
            }) {
                help = strings.get(&format!("psi{id}")).cloned();
            }
        }
        if stats.psi_tier < state.tier {
            components.push(
                gui::image("iface/psibuy10.pcx")
                    .with_rect(Rect::new(15.0, 34.0, 139.0, 123.0))
                    .with_alpha(1.0),
            );
        }
        components.push(super::PanelText::text(
            &super::PanelText::string(world, "misc", "TrainPoints", "Modules"),
            Rect::new(16.0, 167.0, 50.0, 14.0),
        ));
        components.push(super::PanelText::text(
            &stats.cyber_modules.to_string(),
            Rect::new(68.0, 167.0, 80.0, 14.0),
        ));
        if let Some(help) = help.as_ref().or(state.message.as_ref()) {
            components.extend(super::PanelText::paragraph(
                world,
                help,
                Rect::new(12.0, 190.0, 159.0, 97.0),
            ));
        }
        components
    }
    fn get_config(&self) -> GuiConfig {
        PsiPowersGui.get_config()
    }
    fn handle_msg(
        &self,
        _entity: EntityId,
        world: &World,
        state: &PsiTrainerState,
        msg: &PsiPowersGuiMsg,
    ) -> (PsiTrainerState, Effect) {
        if let PsiPowersGuiMsg::BrowseTier(tier) = msg {
            return (
                PsiTrainerState {
                    tier: (*tier).clamp(1, TIERS),
                    message: None,
                },
                Effect::NoEffect,
            );
        }
        let PsiPowersGuiMsg::SelectPower(id) = msg else {
            unreachable!()
        };
        let quests = world
            .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
            .unwrap();
        let stats = quests.player_stats();
        let (cost, effect) = if is_tier_marker(*id) {
            let tier = id / POWERS_PER_TIER + 1;
            let target = super::TrainerTarget::PsiTier(tier);
            (
                crate::difficulty::trainer_costs(world)
                    .and_then(|c| super::upgrade_quote(&c, stats, target)),
                Effect::TrainerPurchase { target },
            )
        } else {
            let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().unwrap();
            let Some(index) = power_index(&powers.0, *id) else {
                return (state.clone(), Effect::NoEffect);
            };
            let template_id = powers.0[index].template_id;
            (
                psi_power_quote(world, template_id),
                Effect::PurchasePsiPower { template_id },
            )
        };
        let (message, effect) = match cost {
            Some(cost) if stats.cyber_modules >= cost => {
                (format!("Purchased (-{} cm)", cost), effect)
            }
            Some(_) => ("Insufficient cyber modules".into(), Effect::NoEffect),
            None => ("Owned, locked or unavailable".into(), Effect::NoEffect),
        };
        (
            PsiTrainerState {
                message: Some(message),
                ..state.clone()
            },
            effect,
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct PsiPowersGuiState;

#[derive(Clone, Debug)]
pub enum PsiPowersGuiMsg {
    /// Browse a tier. Changes nothing the amp will cast.
    BrowseTier(i32),
    /// Select the power with this id, if the player is trained in it.
    SelectPower(i32),
    ClearTeleportMarker,
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

/// Shared retail psi canvas. The character sheet browses its own tier without
/// changing the amp selector's navigation state or selected power.
pub(crate) fn psi_panel_components(
    cursor: &Option<GuiCursor>,
    world: &World,
    tier_override: Option<i32>,
) -> Vec<GuiComponent<PsiPowersGuiMsg>> {
    // Archive-qualified for the same reason the settings MFD qualifies its
    // backdrop: obj.crf and iface.crf collide on plain basenames.
    let mut components: Vec<GuiComponent<PsiPowersGuiMsg>> = vec![
        gui::image(BACKDROP)
            .with_alpha(1.0)
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

    let tier = tier_override.unwrap_or(browsed.0).clamp(1, TIERS);
    let selected_id = crate::psi_amp_selection::target(world)
        .and_then(|amp| crate::psi_amp_selection::selected_power(world, amp))
        .map(|p| p.power.power_id)
        .or_else(|| powers.0.get(selection.index).map(|p| p.power.power_id));

    // The tier strip: one piece of art per browsed tier, over five
    // invisible tabs. A zero-alpha button is the established "hit target
    // over backdrop art" convention - the tabs are painted into the art,
    // so they must be clickable without painting anything over them.
    components.push(
        gui::image(&format!("iface/psi{tier}.pcx"))
            .with_alpha(1.0)
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
                    .with_alpha(1.0)
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

    if tier_override.is_none() && crate::psi_teleport::marker(world).is_some() {
        components.push(
            gui::button(PsiPowersGuiMsg::ClearTeleportMarker)
                .with_label("CLEAR MARKER")
                .with_position(vec2(15.0, 270.0))
                .with_size(vec2(159.0, 19.0)),
        );
        components.push(
            gui::text("CLEAR MARKER")
                .with_position(vec2(48.0, 274.0))
                .with_size(vec2(100.0, 11.0)),
        );
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

impl Gui<PsiPowersGuiState, PsiPowersGuiMsg> for PsiPowersGui {
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        _state: &PsiPowersGuiState,
    ) -> Vec<GuiComponent<PsiPowersGuiMsg>> {
        psi_panel_components(cursor, world, None)
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
            PsiPowersGuiMsg::ClearTeleportMarker => Effect::ClearPsiTeleport,
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
                    sound_tags: Vec::new(),
                });
            }
        }
        powers
    }

    #[test]
    fn quantum_relocation_is_purchasable_without_a_projectile_link() {
        let power = PsiPowerInfo {
            template_id: crate::psi_teleport::POWER,
            name: "Teleport".into(),
            power: PropPsiPower {
                power_id: 35,
                activation_type: 3,
                psi_cost: 5,
                data: [0.0; 4],
            },
            ..registry().remove(0)
        };
        assert!(power.projectiles.is_empty());
        assert!(purchasable_power(&power));
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

    #[test]
    fn discipline_purchase_uses_its_own_price_and_requires_its_tier() {
        let world = world_with(1, 1, &[]);
        world.add_unique(crate::quest_info::QuestInfo::new());
        let mut costs = dark::gamesys::TrainerCostTables {
            stat_cost: [[0; 5]; 5],
            tech_cost: [[0; 6]; 5],
            weapon_cost: [[0; 6]; 4],
            psi_cost: [[7; 8]; 5],
        };
        costs.psi_cost[0][0] = 99;
        costs.psi_cost[0][1] = 3;
        world.add_unique(crate::mission::GlobalTrainerCosts(Some(costs)));
        {
            let mut powers = world
                .borrow::<shipyard::UniqueViewMut<GlobalPsiPowers>>()
                .unwrap();
            powers.0[0].projectiles.push((
                -1,
                dark::properties::ProjectileOptions {
                    order: 0,
                    setting: 0,
                },
            ));
        }
        assert_eq!(
            psi_power_quote(&world, -1001),
            None,
            "a locked tier cannot sell its power"
        );
        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .psi_tier = 1;
        assert_eq!(
            psi_power_quote(&world, -1001),
            Some(3),
            "use the discipline column, not capacity column"
        );
        assert_eq!(
            psi_power_quote(&world, -1002),
            None,
            "an unimplemented power cannot consume modules"
        );
        world
            .borrow::<shipyard::UniqueViewMut<PlayerPsiKnownPowers>>()
            .unwrap()
            .0
            .insert(-1001);
        assert_eq!(
            psi_power_quote(&world, -1001),
            None,
            "already learned powers cannot be charged twice"
        );
    }

    #[test]
    fn trainer_browsing_does_not_move_the_amp_selection() {
        let world = world_with(1, 1, &[1]);
        let (state, effect) = PsiTrainerGui.handle_msg(
            EntityId::dead(),
            &world,
            &PsiTrainerState::default(),
            &PsiPowersGuiMsg::BrowseTier(4),
        );
        assert_eq!(state.tier, 4);
        assert!(matches!(effect, Effect::NoEffect));
        assert_eq!(world.borrow::<UniqueView<PsiPanelTier>>().unwrap().0, 1);
        assert_eq!(
            world
                .borrow::<UniqueView<PsiPowerSelection>>()
                .unwrap()
                .index,
            0
        );
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
    fn character_sheet_browses_without_changing_amp_tier_or_selection() {
        let world = world_with(1, 2, &[1]);
        let before = world
            .borrow::<UniqueView<PsiPowerSelection>>()
            .unwrap()
            .index;
        let components = psi_panel_components(&None, &world, Some(5));
        assert!(
            images(&components)
                .iter()
                .any(|(texture, _)| texture == "iface/psi5.pcx")
        );
        assert_eq!(world.borrow::<UniqueView<PsiPanelTier>>().unwrap().0, 2);
        assert_eq!(
            world
                .borrow::<UniqueView<PsiPowerSelection>>()
                .unwrap()
                .index,
            before
        );
    }

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
            pressed: false,
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

    const DT: f32 = 1.0 / 60.0;

    fn psi(stick: Vector2<f32>, latch: NavLatch) -> (Option<NavStep>, NavLatch) {
        stick_nav(stick, latch, DT, &PSI_STICK_NAV)
    }

    /// One step per flick: holding the stick over cannot walk the whole tier,
    /// and the stick has to come back past the release threshold first.
    #[test]
    fn stick_navigation_is_edge_triggered_with_hysteresis() {
        let (step, latch) = psi(vec2(0.0, 1.0), NavLatch::Free);
        assert_eq!(step, Some(NavStep::TierNext));
        // Held, even just above the release threshold: no more steps.
        assert_eq!(psi(vec2(0.0, 1.0), latch), (None, latch));
        assert_eq!(psi(vec2(0.0, 0.4), latch), (None, latch));
        let (step, latch) = psi(vec2(0.0, 0.0), latch);
        assert_eq!(step, None);
        // Scrolling on in the same direction is not held back.
        assert_eq!(psi(vec2(0.0, 1.0), latch).0, Some(NavStep::TierNext));
    }

    /// A flicked stick springs back past centre; that overshoot is not a
    /// flick the other way - but a real one, a moment later, is.
    #[test]
    fn spring_back_overshoot_does_not_step_backwards() {
        let (_, latch) = psi(vec2(0.0, 1.0), NavLatch::Free);
        let (_, mut latch) = psi(vec2(0.0, 0.1), latch);
        let (step, after_bounce) = psi(vec2(0.0, -0.8), latch);
        assert_eq!(step, None);
        for _ in 0..7 {
            latch = psi(vec2(0.0, 0.0), latch).1;
        }
        assert_eq!(latch, NavLatch::Free);
        assert_ne!(after_bounce, NavLatch::Free);
        assert_eq!(psi(vec2(0.0, -1.0), latch).0, Some(NavStep::TierPrev));
    }

    /// A selector that opens with the stick latched has no bounce to wait out.
    #[test]
    fn releasing_an_opening_latch_steps_at_once() {
        let (step, latch) = psi(vec2(0.0, 0.0), NavLatch::Held(None));
        assert_eq!((step, latch), (None, NavLatch::Free));
        assert_eq!(psi(vec2(0.0, -1.0), latch).0, Some(NavStep::TierPrev));
    }

    #[test]
    fn a_push_steps_only_near_a_cardinal() {
        let free = NavLatch::Free;
        assert_eq!(psi(vec2(1.0, 0.0), free).0, Some(NavStep::PowerNext));
        assert_eq!(psi(vec2(-1.0, 0.0), free).0, Some(NavStep::PowerPrev));
        // Within ~30 degrees of a cardinal still steps along it.
        assert_eq!(psi(vec2(0.9, 0.45), free).0, Some(NavStep::PowerNext));
        assert_eq!(psi(vec2(0.45, -0.9), free).0, Some(NavStep::TierPrev));
        // Below the engage threshold nothing happens at all - a resting stick
        // must not browse.
        assert_eq!(psi(vec2(0.4, 0.4), free), (None, free));
    }

    /// A thumb sweeping right arcs through "up" first; the step waits until
    /// the push is both far enough and clearly one way.
    #[test]
    fn a_push_commits_only_once_it_is_far_and_unambiguous() {
        let free = NavLatch::Free;
        // Early in the arc: leaning up, but not pushed far enough yet.
        assert_eq!(psi(vec2(0.3, 0.55), free), (None, free));
        // Fully pushed but diagonal: no step, and nothing latched.
        assert_eq!(psi(vec2(0.9, 0.8), free), (None, free));
        // Settled on the intended direction.
        assert_eq!(psi(vec2(0.9, 0.2), free).0, Some(NavStep::PowerNext));
    }

    /// A thumb pulling down curls toward the palm and falls short, so down
    /// steps sooner and drifts wider than the other three.
    #[test]
    fn a_down_push_is_more_forgiving() {
        let free = NavLatch::Free;
        assert_eq!(psi(vec2(0.0, -0.7), free).0, Some(NavStep::TierPrev));
        assert_eq!(psi(vec2(0.0, 0.7), free), (None, free));
        // ~37 degrees off the cardinal: down only.
        assert_eq!(psi(vec2(0.6, -0.8), free).0, Some(NavStep::TierPrev));
        assert_eq!(psi(vec2(0.6, 0.8), free), (None, free));
        assert_eq!(
            stick_lean(vec2(0.0, -0.7), free, &PSI_STICK_NAV),
            Some((NavStep::TierPrev, 1.0))
        );
    }

    /// GamePig keeps its looser feel: diagonals walk the overworld, and a
    /// reverse hop right after a release is a real one.
    #[test]
    fn handheld_navigation_takes_diagonals_and_quick_reversals() {
        let handheld = |stick, latch| stick_nav(stick, latch, DT, &HANDHELD_STICK_NAV);
        assert_eq!(
            handheld(vec2(0.6, 0.55), NavLatch::Free).0,
            Some(NavStep::PowerNext)
        );
        let (_, latch) = handheld(vec2(0.0, 1.0), NavLatch::Free);
        let (_, latch) = handheld(vec2(0.0, 0.0), latch);
        assert_eq!(handheld(vec2(0.0, -0.6), latch).0, Some(NavStep::TierPrev));
        assert_eq!(
            HANDHELD_STICK_NAV.limits(NavStep::TierPrev),
            HANDHELD_STICK_NAV.limits(NavStep::TierNext)
        );
    }

    /// The lean grows from nothing to its fullest as the push nears
    /// the engage threshold, and never previews what will not step.
    #[test]
    fn stick_lean_previews_the_step_a_push_is_heading_for() {
        let lean = |stick, latch| stick_lean(stick, latch, &PSI_STICK_NAV);
        assert_eq!(lean(vec2(0.2, 0.0), NavLatch::Free), None);
        let (step, amount) = lean(vec2(0.5, 0.1), NavLatch::Free).unwrap();
        assert_eq!(step, NavStep::PowerNext);
        assert!((amount - 0.5).abs() < 1e-6);
        assert_eq!(
            lean(vec2(0.0, -1.0), NavLatch::Free),
            Some((NavStep::TierPrev, 1.0))
        );
        // Still held from the step it just took.
        assert_eq!(
            lean(vec2(1.0, 0.0), NavLatch::Held(Some(NavStep::PowerNext))),
            None
        );
        // Nor is a diagonal that will not step.
        assert_eq!(lean(vec2(0.6, 0.5), NavLatch::Free), None);
        // A spring-back is not previewed; a push the same way is.
        let settling = NavLatch::Settling(NavStep::TierNext, 0.05);
        assert_eq!(lean(vec2(0.0, -0.6), settling), None);
        assert_eq!(lean(vec2(0.0, 0.6), settling).unwrap().0, NavStep::TierNext);
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
