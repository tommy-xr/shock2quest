//! The Developer screen's parameter rows, described once for every host.
//!
//! Category submenus and parameter rows, with Back and a host exit button,
//! all in canvas pixels on the shared
//! 640x480 frontend canvas. Two hosts draw it (the [`DeveloperScene`] reached
//! from the main menu, and the pause overlay's Developer page) and both call
//! these same functions, so the screen cannot drift between hosts - and
//! because placement is decided here, once, in canvas pixels, flatscreen and
//! VR render it identically by construction (AGENTS.md §3).
//!
//! The geometry rides the `GAMELOD.PCX` backdrop both hosts use for this
//! page: the header line, the dark list pane, and the framed button art in
//! the bottom-right corner (the decoded `GAMELODR.BIN` rects, shared with
//! [`crate::scenes::LoadGameScene`]). Rows live inside the pane, scrolling in
//! a gutter down its right edge so a registry larger than the pane stays
//! fully reachable; "Done" sits on the button art.
//!
//! [`DeveloperScene`]: crate::scenes::DeveloperScene

pub use super::dev_params_navigation::{
    DevParamsLocation, DevParamsNavigation, DevParamsRow, DevParamsSession,
};

use cgmath::{Vector2, vec2};
use dark::{importers::UI_LAYOUT_IMPORTER, map::MapRect};
use engine::assets::asset_cache::AssetCache;

use super::{
    HAlign, Rect, UiCanvas, VAlign,
    list_scroll::{self, ScrollHalf},
};
use crate::dev_params::{self, DevParamId, DevParamKind};

/// The frontend screens are authored on the original 640x480 canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;

/// Display font for the header, the arrows and "Done" (`METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
/// Small data font for the row labels and value readouts (`mainfont.fon`).
const ROW_FONT: &str = "mainfont.fon";

/// The backdrop this panel is laid out on, and its widget-rect layout file.
/// Shared with [`crate::scenes::LoadGameScene`], which owns the same art.
pub const BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
const LAYOUT_FILE: &str = "GAMELODR.BIN";

/// Indices into `GAMELODR.BIN`: header line, the dark list pane, the upper
/// framed button (the load screen's "Load"; the Developer screen puts its
/// debug-scene launcher there) and the bottom-right button art.
const HEADER_RECT_INDEX: usize = 0;
const LIST_RECT_INDEX: usize = 1;
const ACTION_RECT_INDEX: usize = 2;
const DONE_RECT_INDEX: usize = 3;

/// Decoded `GAMELODR.BIN` values, used when the layout file is absent - the
/// same fallbacks the load screen carries for the same art.
const FALLBACK_HEADER: Rect = Rect::new(261.0, 31.0, 202.0, 20.0);
const FALLBACK_LIST: Rect = Rect::new(261.0, 54.0, 202.0, 290.0);
const FALLBACK_ACTION: Rect = Rect::new(527.0, 161.0, 96.0, 62.0);
const FALLBACK_DONE: Rect = Rect::new(527.0, 405.0, 95.0, 62.0);

/// Canvas y where the backdrop paints its bordered name-entry field; rows
/// stop above it (see the sibling constant on the load screen). Public so the
/// other page drawn on this backdrop - the Developer screen's debug-scene
/// launcher - stops its own rows at the same painted border.
pub const FIELD_TOP_Y: f32 = 323.0;

/// The panel's widget rects, resolved from `GAMELODR.BIN`.
///
/// Read from the layout file rather than hardcoded, for the same reason the
/// load screen reads them: they describe where the *art* puts its widgets, so
/// an alternate authored layout has to move the rows, the arrows and "Done"
/// with the backdrop. Both hosts resolve this once per frame and hand it to
/// [`draw`] and [`hit`] alike, so drawing and hit-testing can never disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRects {
    header: Rect,
    list: Rect,
    action: Rect,
    done: Rect,
}

impl Default for PanelRects {
    fn default() -> Self {
        Self {
            header: FALLBACK_HEADER,
            list: FALLBACK_LIST,
            action: FALLBACK_ACTION,
            done: FALLBACK_DONE,
        }
    }
}

impl PanelRects {
    fn from_layout(layout: Option<&[MapRect]>) -> Self {
        let at = |index: usize, fallback: Rect| {
            layout
                .and_then(|rects| rects.get(index))
                .map(|r| {
                    Rect::new(
                        r.ul_x as f32,
                        r.ul_y as f32,
                        r.width() as f32,
                        r.height() as f32,
                    )
                })
                .unwrap_or(fallback)
        };
        Self {
            header: at(HEADER_RECT_INDEX, FALLBACK_HEADER),
            list: at(LIST_RECT_INDEX, FALLBACK_LIST),
            action: at(ACTION_RECT_INDEX, FALLBACK_ACTION),
            done: at(DONE_RECT_INDEX, FALLBACK_DONE),
        }
    }

    /// Canvas center of the "Done" button, for hosts that need to reason
    /// about where it lands relative to their own widgets (the pause overlay
    /// asserts that its "Quit" button sits under it).
    pub fn done_center(&self) -> Vector2<f32> {
        self.done.center()
    }

    /// The header line, the list pane and the upper framed button, for a host
    /// that draws its own page on this same backdrop (the Developer screen's
    /// debug-scene launcher). Read here rather than re-resolved so every page
    /// of that screen rides one set of authored rects.
    pub fn header_rect(&self) -> Rect {
        self.header
    }

    pub fn list_rect(&self) -> Rect {
        self.list
    }

    pub fn action_rect(&self) -> Rect {
        self.action
    }

    pub fn done_rect(&self) -> Rect {
        self.done
    }
}

/// Resolve the panel's rects from the shipped layout file.
pub fn rects(asset_cache: &mut AssetCache) -> PanelRects {
    let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
    PanelRects::from_layout(layout.as_deref().map(|r| r.as_slice()))
}

/// Compact single-line rows; names keep most of the pane's width while
/// the value controls form a tight group at the right edge.
const ROW_PITCH: f32 = 28.0;
const ROW_H: f32 = 24.0;
const PARAM_FONT_SIZE: f32 = 10.0;
const CONTROL_GAP: f32 = 6.0;
/// Horizontal inset from the pane's edges, matching the load list's text
/// inset so the two screens' contents align inside the same art.
const TEXT_INSET: f32 = 8.0;
/// Width of the `<` / `>` hit regions.
const ARROW_W: f32 = 12.0;
/// Width of the value readout between the arrows.
const VALUE_W: f32 = 30.0;
/// The scroll gutter down the list pane's right edge. The rocker itself - its
/// geometry, its arrow art and its "an end that cannot move is inert" rule - is
/// [`list_scroll`]'s, shared with the debug-scene launcher.
const SCROLL_GUTTER_W: f32 = list_scroll::GUTTER_W;

/// Opacity for an element the pointer is not over.
const IDLE_OPACITY: f32 = 0.65;
/// Opacity for the element under the pointer.
const HOVER_OPACITY: f32 = 1.0;
/// Labels and values are readouts, not click targets: drawn steady, between
/// the two interactive levels, so the hover flow reads on the arrows.
const READOUT_OPACITY: f32 = 0.85;

/// What a click on the panel asks for. `Decrement`/`Increment` step the
/// parameter by its own declared step; `Done` leaves the screen (the host
/// decides where "back" goes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevParamsEvent {
    Decrement(DevParamId),
    Increment(DevParamId),
    /// Scroll the list one row towards the top of the registry.
    ScrollUp,
    /// Scroll the list one row towards its end.
    ScrollDown,
    Back,
    Enter(DevParamsLocation),
    Bulk(bool),
    Done,
}

/// A row's resolved rects, all derived from the shared layout constants.
struct RowRects {
    label: Rect,
    decrement: Rect,
    value: Rect,
    increment: Rect,
}

fn row_rects(rects: PanelRects, index: usize, len: usize) -> RowRects {
    let list = rects.list;
    let y = list.y + index as f32 * ROW_PITCH;
    // Leave the scroll gutter clear whenever it is in use, so a row's `>`
    // never sits under the rocker (they resolve through one hit test, so an
    // overlap is a genuine ambiguity, not just a visual one).
    // The gutter subsumes the right inset: the rocker's own art carries a dark
    // margin either side of its arrow, so charging the row for both as well
    // costs label width and ellipsizes names that otherwise fit.
    let right_margin = if self::list(rects).max_scroll(len) > 0 {
        SCROLL_GUTTER_W
    } else {
        TEXT_INSET
    };
    let right = list.x + list.w - right_margin;
    let label_x = list.x + TEXT_INSET;
    let increment_x = right - ARROW_W;
    let value_x = increment_x - VALUE_W;
    let decrement_x = value_x - ARROW_W;
    RowRects {
        label: Rect::new(label_x, y, decrement_x - CONTROL_GAP - label_x, ROW_H),
        decrement: Rect::new(decrement_x, y, ARROW_W, ROW_H),
        value: Rect::new(value_x, y, VALUE_W, ROW_H),
        increment: Rect::new(increment_x, y, ARROW_W, ROW_H),
    }
}

/// This page's list geometry - the paging and the gutter rocker every other
/// frontend list uses, at the parameter rows' own pitch. The rows themselves
/// are built by [`row_rects`] rather than the shared `row_rect`, because a
/// parameter row contains a name and three compact value controls.
fn list(rects: PanelRects) -> list_scroll::ListGeometry {
    list_scroll::ListGeometry {
        pane: rects.list,
        bottom_limit: FIELD_TOP_Y,
        row_h: ROW_PITCH,
        text_inset: TEXT_INSET,
    }
}

fn back_rect(rects: PanelRects) -> Rect {
    Rect::new(rects.list.x, rects.done.y, 96.0, rects.done.h)
}

fn wide_row(rects: PanelRects, slot: usize, len: usize) -> Rect {
    let row = row_rects(rects, slot, len);
    Rect::new(
        row.label.x,
        row.label.y,
        row.increment.x + row.increment.w - row.label.x,
        ROW_H,
    )
}

fn format_value(kind: &DevParamKind, value: f32, bool_labels: Option<[&str; 2]>) -> String {
    match kind {
        DevParamKind::Float { min, step, .. } => {
            // Use the grid's precision instead of spending narrow readout space
            // on trailing zeroes (50 ms must read "50", not ellipsized "50…").
            // Include min: a 0.1 grid anchored at 0.25 still needs two decimals.
            let precision = if *step <= 0.0 {
                2
            } else {
                (0..=4)
                    .find(|digits| {
                        let scale = 10_f32.powi(*digits);
                        [*min, *step].into_iter().all(|v| {
                            let scaled = v * scale;
                            (scaled - scaled.round()).abs() < 0.0001
                        })
                    })
                    .unwrap_or(4) as usize
            };
            format!("{value:.precision$}")
        }
        DevParamKind::Bool => {
            bool_labels.unwrap_or(["Off", "On"])[usize::from(value != 0.0)].to_owned()
        }
    }
}

/// The event at a canvas point, if any. Shared by the click and the hover
/// highlight, so the two can never disagree about where a button is.
/// Build controls and readouts once. Both rendering and hit testing replay this
/// emit, so categories, bulk buttons and parameter arrows share exact geometry.
fn emit(
    rects: PanelRects,
    navigation: &DevParamsNavigation,
    exit_label: &str,
    pointer: Option<Vector2<f32>>,
) -> (UiCanvas, Vec<(Rect, DevParamsEvent)>) {
    let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
    let mut targets = Vec::new();
    let mut button = |canvas: &mut UiCanvas,
                      rect: Rect,
                      label: &str,
                      event: DevParamsEvent,
                      font: &str,
                      align: HAlign| {
        targets.push((rect, event));
        // Rocker art is emitted by list_scroll; its targets have no text.
        // Empty text has no glyph mesh and must not reach the renderer.
        if label.is_empty() {
            return;
        }
        let font_size = if matches!(
            event,
            DevParamsEvent::Increment(_) | DevParamsEvent::Decrement(_)
        ) {
            PARAM_FONT_SIZE
        } else {
            0.0
        };
        canvas
            .text_fit(rect, label, font, font_size, align, VAlign::Middle)
            .opacity(if pointer.is_some_and(|p| rect.contains(p)) {
                HOVER_OPACITY
            } else {
                IDLE_OPACITY
            });
    };
    let entries = navigation.rows();
    let len = entries.len();
    let geometry = list(rects);
    let visible = geometry.visible_rows(len, navigation.scroll());
    let title = if navigation.location.locked
        && navigation.location.category() == dev_params::DevCategory::Root
    {
        "Locked"
    } else {
        navigation.location.category().label()
    };
    canvas.text_native_fit(
        rects.header,
        title,
        MENU_FONT,
        HAlign::Center,
        VAlign::Middle,
    );
    // Use the interior of the backdrop's name field. Its borders are at
    // y=323 and y=344; inset the text on every side rather than straddling
    // that rule or the curved footer. The header already names this category.
    let breadcrumb = navigation.breadcrumb();
    if let Some((parents, _)) = breadcrumb.rsplit_once(" > ") {
        canvas.text_fit(
            Rect::new(
                rects.list.x + TEXT_INSET,
                FIELD_TOP_Y + 3.0,
                rects.list.w - TEXT_INSET * 2.0,
                14.0,
            ),
            parents,
            ROW_FONT,
            PARAM_FONT_SIZE,
            HAlign::Center,
            VAlign::Middle,
        );
    }
    for (slot, entry) in entries[visible.clone()].iter().enumerate() {
        let row = row_rects(rects, slot, len);
        match *entry {
            DevParamsRow::Category(location) => {
                let label = if location.category.is_none() {
                    "Locked"
                } else {
                    location.category().label()
                };
                let label = if dev_params::DevCategory::Visualizations.contains(location.category())
                {
                    let members: Vec<_> = navigation.params_under(location.category()).collect();
                    let on = members
                        .iter()
                        .filter(|&&id| dev_params::get_bool(id))
                        .count();
                    format!("{label} {on}/{} >", members.len())
                } else {
                    format!("{label} >")
                };
                button(
                    &mut canvas,
                    wide_row(rects, slot, len),
                    &label,
                    DevParamsEvent::Enter(location),
                    ROW_FONT,
                    HAlign::Left,
                );
            }
            DevParamsRow::Bulk => {
                let whole = wide_row(rects, slot, len);
                let half = whole.w / 2.0;
                button(
                    &mut canvas,
                    Rect::new(whole.x, whole.y, half, whole.h),
                    "All on",
                    DevParamsEvent::Bulk(true),
                    ROW_FONT,
                    HAlign::Center,
                );
                button(
                    &mut canvas,
                    Rect::new(whole.x + half, whole.y, half, whole.h),
                    "All off",
                    DevParamsEvent::Bulk(false),
                    ROW_FONT,
                    HAlign::Center,
                );
            }
            DevParamsRow::Parameter(id) => {
                let param = dev_params::spec(id);
                canvas
                    .text_fit(
                        row.label,
                        param.label,
                        ROW_FONT,
                        PARAM_FONT_SIZE,
                        HAlign::Left,
                        VAlign::Middle,
                    )
                    .opacity(READOUT_OPACITY);
                button(
                    &mut canvas,
                    row.decrement,
                    "<",
                    DevParamsEvent::Decrement(id),
                    ROW_FONT,
                    HAlign::Center,
                );
                canvas
                    .text_fit(
                        row.value,
                        &format_value(&param.kind, dev_params::get(id), param.bool_labels),
                        ROW_FONT,
                        PARAM_FONT_SIZE,
                        HAlign::Center,
                        VAlign::Middle,
                    )
                    .opacity(READOUT_OPACITY);
                button(
                    &mut canvas,
                    row.increment,
                    ">",
                    DevParamsEvent::Increment(id),
                    ROW_FONT,
                    HAlign::Center,
                );
            }
        }
    }
    if let Some(rocker) = geometry.rocker(len) {
        let max = geometry.max_scroll(len);
        let hovered =
            pointer.and_then(|point| list_scroll::hit(&rocker, visible.start, max, point));
        list_scroll::draw(&mut canvas, &rocker, visible.start, max, hovered);
        if visible.start > 0 {
            button(
                &mut canvas,
                rocker.up,
                "",
                DevParamsEvent::ScrollUp,
                MENU_FONT,
                HAlign::Center,
            );
        }
        if visible.start < max {
            button(
                &mut canvas,
                rocker.down,
                "",
                DevParamsEvent::ScrollDown,
                MENU_FONT,
                HAlign::Center,
            );
        }
    }
    button(
        &mut canvas,
        back_rect(rects),
        "Back",
        DevParamsEvent::Back,
        MENU_FONT,
        HAlign::Center,
    );
    button(
        &mut canvas,
        rects.done,
        exit_label,
        DevParamsEvent::Done,
        MENU_FONT,
        HAlign::Center,
    );
    (canvas, targets)
}

pub fn hit(
    rects: PanelRects,
    navigation: &DevParamsNavigation,
    point: Vector2<f32>,
) -> Option<DevParamsEvent> {
    emit(rects, navigation, "Done", None)
        .1
        .into_iter()
        .rev()
        .find(|(rect, _)| rect.contains(point))
        .map(|(_, event)| event)
}

pub fn draw(
    canvas: &mut UiCanvas,
    rects: PanelRects,
    navigation: &DevParamsNavigation,
    pointer_canvas: Option<Vector2<f32>>,
    exit_label: &str,
) {
    let (content, _) = emit(rects, navigation, exit_label, pointer_canvas);
    // Replay the same resolved widgets with the host's hover position.
    for element in content.into_elements() {
        canvas.push(element);
    }
}

/// Apply a click to the registry or session navigation.
/// Returns `true` when the event was [`DevParamsEvent::Done`] - the one thing
/// the host must act on (leave the screen); the steps and the scrolling are
/// absorbed here so both hosts stay a one-liner.
pub fn activate(
    rects: PanelRects,
    event: DevParamsEvent,
    navigation: &mut DevParamsNavigation,
) -> bool {
    let step_by = |id: DevParamId, direction: f32| match dev_params::spec(id).kind {
        DevParamKind::Float { step, .. } => {
            // `set` clamps into range and snaps to the step grid, so walking
            // off either end just pins to it.
            dev_params::set(id, dev_params::get(id) + direction * step);
        }
        // A switch has no grid to walk: either arrow flips it, so the row
        // behaves the same whichever side the pointer lands on.
        DevParamKind::Bool => {
            dev_params::set(id, if dev_params::get_bool(id) { 0.0 } else { 1.0 });
        }
    };
    match event {
        DevParamsEvent::Decrement(id) => {
            step_by(id, -1.0);
            false
        }
        DevParamsEvent::Increment(id) => {
            step_by(id, 1.0);
            false
        }
        DevParamsEvent::ScrollUp => {
            let max = list(rects).max_scroll(navigation.rows().len());
            list_scroll::apply(ScrollHalf::Up, navigation.scroll_mut(), max);
            false
        }
        DevParamsEvent::ScrollDown => {
            let max = list(rects).max_scroll(navigation.rows().len());
            list_scroll::apply(ScrollHalf::Down, navigation.scroll_mut(), max);
            false
        }
        DevParamsEvent::Enter(location) => {
            navigation.enter(location);
            false
        }
        DevParamsEvent::Back => !navigation.back(),
        DevParamsEvent::Bulk(enabled) => {
            for id in navigation.bulk_members() {
                dev_params::set(id, if enabled { 1.0 } else { 0.0 });
            }
            false
        }
        DevParamsEvent::Done => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dev_params::DevCategory;

    #[test]
    fn every_param_is_reachable_only_in_its_locked_or_ordinary_category() {
        let rects = PanelRects::default();
        for (id, param) in dev_params::all() {
            let mut nav = DevParamsNavigation::default();
            nav.enter(DevParamsLocation {
                category: Some(param.category),
                locked: param.locked,
            });
            let entries = nav.rows();
            let index = entries
                .iter()
                .position(|r| *r == DevParamsRow::Parameter(id))
                .unwrap();
            *nav.scroll_mut() = index;
            let visible = list(rects).visible_rows(entries.len(), nav.scroll());
            assert!(visible.contains(&index));
            let row = row_rects(rects, index - visible.start, entries.len());
            assert_eq!(
                hit(rects, &nav, row.increment.center()),
                Some(DevParamsEvent::Increment(id))
            );
            assert_eq!(
                hit(rects, &nav, row.decrement.center()),
                Some(DevParamsEvent::Decrement(id))
            );
            assert!(row.label.y + ROW_H <= FIELD_TOP_Y);
            assert_eq!(hit(rects, &nav, row.label.center()), None);
            nav.location.locked = !param.locked;
            assert!(!nav.rows().contains(&DevParamsRow::Parameter(id)));
        }
    }

    #[test]
    fn category_controls_drill_in_and_back_without_changing_other_scroll_positions() {
        let rects = PanelRects {
            list: Rect::new(261.0, 54.0, 202.0, 100.0),
            ..PanelRects::default()
        };
        let mut nav = DevParamsNavigation::default();
        let root = nav;
        let first = hit(rects, &nav, wide_row(rects, 0, nav.rows().len()).center()).unwrap();
        assert!(matches!(first, DevParamsEvent::Enter(_)));
        assert!(!activate(rects, first, &mut nav));
        assert_eq!(nav.location.category(), DevCategory::Visualizations);
        assert!(!activate(rects, DevParamsEvent::Back, &mut nav));
        assert_eq!(nav.location.category(), root.location.category());
        assert!(activate(rects, DevParamsEvent::Back, &mut nav));
        let body = DevParamsLocation {
            category: Some(DevCategory::Body),
            locked: false,
        };
        nav.enter(body);
        for _ in 0..20 {
            activate(rects, DevParamsEvent::ScrollDown, &mut nav);
        }
        let bottom = nav.scroll();
        assert!(bottom > 0, "exercise a real nonzero scroll position");
        assert_eq!(bottom, list(rects).max_scroll(nav.rows().len()));
        nav.back();
        assert_eq!(nav.scroll(), 0);
        nav.enter(body);
        assert_eq!(nav.scroll(), bottom);
        let before_exit = nav;
        assert!(activate(rects, DevParamsEvent::Done, &mut nav));
        assert_eq!(nav, before_exit);
    }

    #[test]
    fn category_tree_reaches_every_parameter_and_has_no_empty_branches() {
        fn walk(nav: DevParamsNavigation, found: &mut Vec<DevParamId>) {
            for row in nav.rows() {
                match row {
                    DevParamsRow::Category(location) => {
                        let mut child = nav;
                        child.enter(location);
                        assert!(!child.rows().is_empty());
                        walk(child, found);
                    }
                    DevParamsRow::Parameter(id) => {
                        assert!(!found.contains(&id), "duplicate parameter in tree");
                        found.push(id);
                    }
                    DevParamsRow::Bulk => {}
                }
            }
        }
        let mut found = Vec::new();
        walk(DevParamsNavigation::default(), &mut found);
        assert_eq!(found.len(), dev_params::PARAMS.len());
    }

    #[test]
    fn bulk_members_are_only_visualization_bools() {
        let mut nav = DevParamsNavigation::default();
        for category in DevCategory::ALL {
            nav.enter(DevParamsLocation {
                category: Some(category),
                locked: false,
            });
            let members = nav.bulk_members();
            assert_eq!(
                !members.is_empty(),
                DevCategory::Visualizations.contains(category)
            );
            for id in members {
                assert_eq!(dev_params::spec(id).kind, DevParamKind::Bool);
                assert!(!dev_params::spec(id).locked);
            }
        }
        nav.enter(DevParamsLocation {
            category: Some(DevCategory::Interaction),
            locked: false,
        });
        assert_eq!(nav.bulk_members().len(), 7);
        assert!(!nav.bulk_members().contains(&dev_params::GLOVE_FIT_VISIBLE));
    }

    #[test]
    fn alternate_layout_moves_controls_and_reserves_scroll_gutter() {
        let rects = PanelRects::from_layout(Some(&[
            MapRect::new(10, 0, 110, 50),
            MapRect::new(20, 60, 300, 160),
            MapRect::new(350, 100, 396, 160),
            MapRect::new(400, 400, 500, 450),
        ]));
        let nav = DevParamsNavigation::default();
        assert_eq!(
            hit(rects, &nav, rects.done.center()),
            Some(DevParamsEvent::Done)
        );
        assert_eq!(hit(rects, &nav, FALLBACK_DONE.center()), None);
        let rocker = list(rects).rocker(nav.rows().len()).unwrap();
        assert_eq!(hit(rects, &nav, rocker.up.center()), None);
        assert_eq!(
            hit(rects, &nav, rocker.down.center()),
            Some(DevParamsEvent::ScrollDown)
        );
        let row = wide_row(rects, 0, nav.rows().len());
        assert!(row.x + row.w <= rocker.up.x);
        assert_eq!(PanelRects::from_layout(None), PanelRects::default());
    }

    #[test]
    fn locked_breadcrumb_and_back_preserve_the_filter_until_locked_root() {
        let mut nav = DevParamsNavigation::default();
        nav.enter(DevParamsLocation {
            category: Some(DevCategory::Melee),
            locked: true,
        });
        assert_eq!(nav.breadcrumb(), "Developer > Locked > Weapons > Melee");
        assert!(nav.back());
        assert!(nav.location.locked);
        assert!(nav.back());
        assert_eq!(nav.breadcrumb(), "Developer > Locked");
        assert!(nav.back());
        assert_eq!(nav, DevParamsNavigation::default());
    }

    #[test]
    fn scrolling_emits_targets_without_empty_text_meshes() {
        let mut nav = DevParamsNavigation::default();
        nav.enter(DevParamsLocation {
            category: Some(DevCategory::Interaction),
            locked: false,
        });
        let rects = PanelRects {
            list: Rect::new(261.0, 54.0, 202.0, 100.0),
            ..PanelRects::default()
        };
        let (canvas, targets) = emit(rects, &nav, "Resume", None);
        assert!(
            targets
                .iter()
                .any(|(_, event)| *event == DevParamsEvent::ScrollDown)
        );
        for element in canvas.elements() {
            if let super::super::UiElement::Text { text, .. } = element {
                assert!(!text.is_empty(), "empty text cannot build a glyph mesh");
            }
        }
        let row = row_rects(rects, 1, nav.rows().len());
        assert!(row.label.w > ARROW_W * 2.0 + VALUE_W);
        assert!(row.label.x + row.label.w < row.decrement.x);
        assert_eq!(row.label.center().y, row.value.center().y);
    }

    #[test]
    fn values_format_without_float_grid_noise_and_preserve_bool_labels() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        assert_eq!(format_value(&kind, 0.719_999_97, None), "0.72");
        for (id, value, expected) in [
            (dev_params::THROW_SMOOTHING_MS, 50.0, "50"),
            (dev_params::THROW_SMOOTHING_MS, 100.0, "100"),
            (dev_params::THROW_STRENGTH_BONUS, 0.25, "0.25"),
            (dev_params::VR_GLOVE_RADIUS, 0.055, "0.055"),
        ] {
            assert_eq!(
                format_value(&dev_params::spec(id).kind, value, None),
                expected
            );
        }
        assert_eq!(
            format_value(
                &DevParamKind::Float {
                    min: 0.25,
                    max: 2.0,
                    step: 0.1
                },
                0.35,
                None
            ),
            "0.35"
        );
        assert_eq!(format_value(&DevParamKind::Bool, 1.0, None), "On");
        assert_eq!(
            format_value(&DevParamKind::Bool, 0.0, Some(["Aim", "Grip"])),
            "Aim"
        );
    }
}
