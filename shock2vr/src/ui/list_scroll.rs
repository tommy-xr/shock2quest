//! One scrolling list, described once for every screen that has one.
//!
//! A frontend list pane is always the same shape: rows at a fixed pitch, a page
//! of them bounded by the backdrop art, and - when the contents outrun the page
//! - a two-button rocker in a gutter down the pane's right edge. This module
//! owns that geometry, its enablement rule ("a half that cannot move is inert,
//! not just dimmed") and its arrow art, so the Developer parameter list and the
//! debug-scene launcher scroll the same way rather than each growing their own.
//!
//! Everything here is in canvas pixels and presentation-free: hosts hand the
//! result to their hit test and their draw alike, which is what keeps flatscreen
//! and VR identical (AGENTS.md §3).

use std::ops::Range;

use cgmath::Vector2;

use super::{Rect, UiCanvas};

/// The gutter the rocker lives in, and the height of each of its halves -
/// the authored 32x16 size of the arrow art, so it draws unstretched.
pub const GUTTER_W: f32 = 32.0;
const BUTTON_H: f32 = 16.0;

/// Breathing room between the down arrow and the line the page stops at, so
/// the arrow reads as sitting in the pane rather than resting on its edge.
const BOTTOM_GAP: f32 = 6.0;

/// The original scroll-arrow art for one half. The art carries the shading, so
/// the rocker draws fully opaque and picks a state instead of an opacity.
struct ArrowArt {
    /// Idle.
    norm: &'static str,
    /// Under the pointer.
    hlit: &'static str,
    /// A half that cannot move. This is the original's *pressed* plate; the
    /// rocker has no press-and-hold visual of its own, and its darkened arrow
    /// reads as unavailable, so it doubles as the inert state.
    down: &'static str,
}

const UP_ART: ArrowArt = ArrowArt {
    norm: "BUP_NORM.PCX",
    hlit: "BUP_HLIT.PCX",
    down: "BUP_DOWN.PCX",
};
const DOWN_ART: ArrowArt = ArrowArt {
    norm: "BDN_NORM.PCX",
    hlit: "BDN_HLIT.PCX",
    down: "BDN_DOWN.PCX",
};

/// The art a half shows. Named rather than positional so a state can't
/// silently swap with its neighbour.
fn art_for(art: &ArrowArt, enabled: bool, hovered: bool) -> &'static str {
    if !enabled {
        art.down
    } else if hovered {
        art.hlit
    } else {
        art.norm
    }
}

/// Which half of the rocker a point is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollHalf {
    Up,
    Down,
}

/// The rocker's two halves, stacked at the top and bottom of the gutter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rocker {
    pub up: Rect,
    pub down: Rect,
}

/// How many rows fit in `list` at `row_pitch`, stopping at `bottom_limit` (the
/// y where the backdrop starts painting something rows must clear) when the
/// pane runs past it.
pub fn rows_per_page(list: Rect, bottom_limit: f32, row_pitch: f32) -> usize {
    let usable = (list.y + list.h).min(bottom_limit) - list.y;
    (usable / row_pitch).floor().max(0.0) as usize
}

/// The furthest the list can scroll: the first-row index that puts the tail of
/// the contents against the bottom of the pane. Zero when everything fits at
/// once, which is also what hides the rocker.
pub fn max_scroll(items: usize, rows_per_page: usize) -> usize {
    items.saturating_sub(rows_per_page)
}

/// The item indices on screen at `scroll`, with `scroll` clamped to what the
/// pane can actually show.
///
/// The one place a screen row is tied to an item: draw and hit-test both walk
/// this range, so a row can never *show* one item and *act on* another - the
/// failure a positional row index invites the moment the list scrolls.
pub fn visible_rows(items: usize, rows_per_page: usize, scroll: usize) -> Range<usize> {
    let first = scroll.min(max_scroll(items, rows_per_page));
    let count = rows_per_page.min(items - first);
    first..first + count
}

/// The rocker in a gutter down the pane's right edge, or `None` when the whole
/// list fits on one page. `needed` is `max_scroll(..) > 0`.
pub fn rocker(list: Rect, bottom_limit: f32, needed: bool) -> Option<Rocker> {
    needed.then(|| {
        let x = list.x + list.w - GUTTER_W;
        let bottom = (list.y + list.h).min(bottom_limit);
        Rocker {
            up: Rect::new(x, list.y, GUTTER_W, BUTTON_H),
            down: Rect::new(x, bottom - BUTTON_H - BOTTOM_GAP, GUTTER_W, BUTTON_H),
        }
    })
}

/// Which half is at a canvas point, if any. `scroll` is the *clamped* offset
/// (i.e. `visible_rows(..).start`), so a half that cannot move is inert rather
/// than clickable-but-useless.
pub fn hit(
    rocker: &Rocker,
    scroll: usize,
    max_scroll: usize,
    point: Vector2<f32>,
) -> Option<ScrollHalf> {
    if scroll > 0 && rocker.up.contains(point) {
        return Some(ScrollHalf::Up);
    }
    if scroll < max_scroll && rocker.down.contains(point) {
        return Some(ScrollHalf::Down);
    }
    None
}

/// Move `scroll` by one row, stopping at either end.
///
/// The offset is clamped into range *before* it moves, so an offset left over
/// from a longer list (or a taller pane) does not need several dead `Up`
/// clicks to unstick: the display already clamps via [`visible_rows`], and
/// this keeps the stored offset agreeing with what is drawn.
pub fn apply(half: ScrollHalf, scroll: &mut usize, max_scroll: usize) {
    *scroll = (*scroll).min(max_scroll);
    match half {
        ScrollHalf::Up => *scroll = scroll.saturating_sub(1),
        ScrollHalf::Down => *scroll = (*scroll + 1).min(max_scroll),
    }
}

/// What a point on a list pane landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListHit {
    /// The item index scrolled into the row under the point - never the row's
    /// own slot, which would act on whatever *used* to sit there.
    Row(usize),
    Scroll(ScrollHalf),
}

/// One list pane's geometry: everything a screen needs to turn "a pane, a row
/// height and a number of items" into rows, a rocker and a hit test.
///
/// The primitives above are shared; this is the *composition* of them, which
/// was written out once per screen until three lists had their own copy. A
/// screen now states its pane and its pitch and gets the same paging, the same
/// gutter, and the same slot-to-index mapping as every other list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListGeometry {
    /// The pane the rows live in, from the backdrop's authored layout.
    pub pane: Rect,
    /// The y the rows must stop at - where the backdrop starts painting
    /// something they have to clear.
    pub bottom_limit: f32,
    /// Distance between row tops, and each row's height.
    pub row_h: f32,
    /// Horizontal inset of a row's text from the row.
    pub text_inset: f32,
}

impl ListGeometry {
    pub fn rows_per_page(&self) -> usize {
        rows_per_page(self.pane, self.bottom_limit, self.row_h)
    }

    pub fn max_scroll(&self, len: usize) -> usize {
        max_scroll(len, self.rows_per_page())
    }

    pub fn visible_rows(&self, len: usize, scroll: usize) -> Range<usize> {
        visible_rows(len, self.rows_per_page(), scroll)
    }

    pub fn rocker(&self, len: usize) -> Option<Rocker> {
        rocker(self.pane, self.bottom_limit, self.max_scroll(len) > 0)
    }

    /// The `slot`-th visible row. Rows stop short of the scroll gutter exactly
    /// when [`Self::rocker`] is `Some` - the same expression decides both, so a
    /// row and the rocker can never claim the same point.
    pub fn row_rect(&self, len: usize, slot: usize) -> Rect {
        let gutter = if self.max_scroll(len) > 0 {
            GUTTER_W
        } else {
            0.0
        };
        Rect::new(
            self.pane.x,
            self.pane.y + slot as f32 * self.row_h,
            (self.pane.w - gutter).max(0.0),
            self.row_h,
        )
    }

    /// The rect a row's text is drawn in: the row, inset on both edges.
    pub fn text_rect(&self, len: usize, slot: usize) -> Rect {
        let row = self.row_rect(len, slot);
        Rect::new(
            row.x + self.text_inset,
            row.y,
            (row.w - 2.0 * self.text_inset).max(0.0),
            row.h,
        )
    }

    /// What is at a canvas point: a row's item index, a live rocker half, or
    /// nothing. The rocker is tested first, and a half that cannot move is
    /// inert rather than falling through to the row beneath it.
    pub fn hit(&self, len: usize, scroll: usize, point: Vector2<f32>) -> Option<ListHit> {
        let rows = self.visible_rows(len, scroll);
        if let Some(rocker) = self.rocker(len) {
            if let Some(half) = hit(&rocker, rows.start, self.max_scroll(len), point) {
                return Some(ListHit::Scroll(half));
            }
        }
        (0..rows.len())
            .find(|slot| self.row_rect(len, *slot).contains(point))
            .map(|slot| ListHit::Row(rows.start + slot))
    }
}

/// Describe the rocker onto a host's canvas. `scroll` is the clamped offset and
/// `hovered` the result of the host's own hit test, so the highlight and the
/// click can never disagree.
pub fn draw(
    canvas: &mut UiCanvas,
    rocker: &Rocker,
    scroll: usize,
    max_scroll: usize,
    hovered: Option<ScrollHalf>,
) {
    for (half, rect, art, enabled) in [
        (ScrollHalf::Up, rocker.up, &UP_ART, scroll > 0),
        (
            ScrollHalf::Down,
            rocker.down,
            &DOWN_ART,
            scroll < max_scroll,
        ),
    ] {
        canvas.image(rect, art_for(art, enabled, hovered == Some(half)));
    }
}

#[cfg(test)]
mod tests {
    use cgmath::vec2;

    use super::*;

    const LIST: Rect = Rect::new(261.0, 54.0, 202.0, 290.0);
    const FIELD_TOP_Y: f32 = 323.0;

    #[test]
    fn a_page_stops_at_the_painted_field_not_the_pane_bottom() {
        assert_eq!(rows_per_page(LIST, FIELD_TOP_Y, 19.0), 14);
        // A pane clear of the field uses its full height...
        assert_eq!(
            rows_per_page(Rect::new(261.0, 20.0, 202.0, 190.0), FIELD_TOP_Y, 19.0),
            10
        );
        // ...and one starting below it shows nothing rather than underflowing.
        assert_eq!(
            rows_per_page(Rect::new(261.0, 400.0, 202.0, 60.0), FIELD_TOP_Y, 19.0),
            0
        );
    }

    #[test]
    fn the_visible_window_follows_the_scroll_and_clamps_at_the_end() {
        assert_eq!(visible_rows(20, 5, 0), 0..5);
        assert_eq!(visible_rows(20, 5, 3), 3..8);
        assert_eq!(visible_rows(20, 5, 15), 15..20);
        // An over-scroll cannot walk the list off its end.
        assert_eq!(visible_rows(20, 5, 99), 15..20);
        // Everything fitting means no scrolling at all.
        assert_eq!(max_scroll(3, 5), 0);
        assert_eq!(visible_rows(3, 5, 4), 0..3);
    }

    #[test]
    fn the_rocker_sits_in_the_gutter_and_its_ends_are_inert() {
        let rocker = rocker(LIST, FIELD_TOP_Y, true).expect("this list scrolls");
        assert_eq!(rocker.up.x, LIST.x + LIST.w - GUTTER_W);
        assert_eq!(rocker.up.y, LIST.y);
        assert_eq!(rocker.down.x, rocker.up.x);
        assert_eq!(rocker.down.y + BUTTON_H + BOTTOM_GAP, FIELD_TOP_Y);

        let max = 4;
        assert_eq!(hit(&rocker, 0, max, rocker.up.center()), None);
        assert_eq!(
            hit(&rocker, 0, max, rocker.down.center()),
            Some(ScrollHalf::Down)
        );
        assert_eq!(hit(&rocker, max, max, rocker.down.center()), None);
        assert_eq!(
            hit(&rocker, max, max, rocker.up.center()),
            Some(ScrollHalf::Up)
        );
        assert_eq!(hit(&rocker, 1, max, vec2(0.0, 0.0)), None);

        // A list that fits has no rocker at all.
        assert_eq!(rocker_none(), None);
    }

    fn rocker_none() -> Option<Rocker> {
        rocker(LIST, FIELD_TOP_Y, false)
    }

    #[test]
    fn each_half_shows_the_art_for_its_state() {
        assert_eq!(art_for(&UP_ART, true, false), "BUP_NORM.PCX");
        assert_eq!(art_for(&UP_ART, true, true), "BUP_HLIT.PCX");
        assert_eq!(art_for(&DOWN_ART, true, false), "BDN_NORM.PCX");
        assert_eq!(art_for(&DOWN_ART, true, true), "BDN_HLIT.PCX");
        // A half that cannot move shows the darkened plate whether or not the
        // pointer is over it - it is not hit-testable either.
        assert_eq!(art_for(&UP_ART, false, false), "BUP_DOWN.PCX");
        assert_eq!(art_for(&UP_ART, false, true), "BUP_DOWN.PCX");
        assert_eq!(art_for(&DOWN_ART, false, true), "BDN_DOWN.PCX");
    }

    const GEOMETRY: ListGeometry = ListGeometry {
        pane: LIST,
        bottom_limit: FIELD_TOP_Y,
        row_h: 19.0,
        text_inset: 8.0,
    };

    /// The composition every list shares: rows page, the gutter appears with
    /// the rocker, and a row reports the item scrolled into it.
    #[test]
    fn the_geometry_maps_slots_to_the_scrolled_item() {
        let short = GEOMETRY.rows_per_page() - 1;
        let long = GEOMETRY.rows_per_page() + 5;

        // A list that fits: no rocker, and rows span the full pane.
        assert!(GEOMETRY.rocker(short).is_none());
        assert_eq!(GEOMETRY.row_rect(short, 0).w, LIST.w);
        assert_eq!(
            GEOMETRY.hit(short, 0, GEOMETRY.row_rect(short, 0).center()),
            Some(ListHit::Row(0))
        );

        // A list that does not: the rows make room for the gutter...
        assert!(GEOMETRY.rocker(long).is_some());
        assert_eq!(GEOMETRY.row_rect(long, 0).w, LIST.w - GUTTER_W);
        // ...the top row follows the offset...
        assert_eq!(
            GEOMETRY.hit(long, 3, GEOMETRY.row_rect(long, 0).center()),
            Some(ListHit::Row(3))
        );
        // ...an over-scroll clamps rather than walking off the end...
        assert_eq!(
            GEOMETRY.hit(long, 99, GEOMETRY.row_rect(long, 0).center()),
            Some(ListHit::Row(GEOMETRY.max_scroll(long)))
        );
        // ...and the rocker takes its own gutter, with inert ends.
        let rocker = GEOMETRY.rocker(long).unwrap();
        assert_eq!(
            GEOMETRY.hit(long, 0, rocker.down.center()),
            Some(ListHit::Scroll(ScrollHalf::Down))
        );
        assert_eq!(GEOMETRY.hit(long, 0, rocker.up.center()), None);
    }

    /// Every row of a full page clears the painted field, not just the pane.
    #[test]
    fn a_full_page_of_rows_stays_above_the_field() {
        let len = GEOMETRY.rows_per_page() + 5;
        for slot in 0..GEOMETRY.rows_per_page() {
            let row = GEOMETRY.row_rect(len, slot);
            assert!(row.y >= LIST.y, "{row:?}");
            assert!(row.y + row.h <= FIELD_TOP_Y, "slot {slot}: {row:?}");
        }
    }

    /// An offset stranded past the end (a shorter list, a shallower pane)
    /// must not need dead clicks to unstick.
    #[test]
    fn an_over_scroll_is_clamped_before_it_moves() {
        let mut scroll = 40;
        apply(ScrollHalf::Up, &mut scroll, 5);
        assert_eq!(scroll, 4, "one Up from a stranded offset must move a row");

        let mut scroll = 40;
        apply(ScrollHalf::Down, &mut scroll, 5);
        assert_eq!(scroll, 5);
    }

    #[test]
    fn scrolling_stops_at_both_ends() {
        let max = 3;
        let mut scroll = 0;
        apply(ScrollHalf::Up, &mut scroll, max);
        assert_eq!(scroll, 0);
        for _ in 0..max + 3 {
            apply(ScrollHalf::Down, &mut scroll, max);
        }
        assert_eq!(scroll, max);
        apply(ScrollHalf::Up, &mut scroll, max);
        assert_eq!(scroll, max - 1);
    }
}
