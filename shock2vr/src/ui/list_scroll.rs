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
            down: Rect::new(x, bottom - BUTTON_H, GUTTER_W, BUTTON_H),
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
pub fn apply(half: ScrollHalf, scroll: &mut usize, max_scroll: usize) {
    match half {
        ScrollHalf::Up => *scroll = scroll.saturating_sub(1),
        ScrollHalf::Down => *scroll = (*scroll + 1).min(max_scroll),
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
        assert_eq!(rocker.down.y + BUTTON_H, FIELD_TOP_Y);

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
