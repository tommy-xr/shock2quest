//! The Developer screen's Cheats page: a scrolling list of one-click
//! developer shortcuts, described once for every host.
//!
//! A sibling of [`crate::ui::dev_params_panel`], on the same `GAMELOD.PCX`
//! frame and the same [`PanelRects`], so a cheat row and a parameter row are
//! the same row on the same screen. Placement is decided here, once, in canvas
//! pixels, so flatscreen and VR render it identically by construction
//! (AGENTS.md §3).
//!
//! The page is reached from the **pause overlay's** Developer page only - the
//! frame's upper framed button, which the main menu's [`DeveloperScene`] uses
//! for its scene launcher. Cheats act on the running mission, so there is
//! nothing for them to do before one is loaded.
//!
//! **Adding a cheat is one entry in [`CHEATS`]** and nothing else: the list
//! pages and grows a scroll rocker on its own once the entries outrun the pane.
//!
//! [`DeveloperScene`]: crate::scenes::DeveloperScene

use cgmath::Vector2;

#[cfg(test)]
use cgmath::vec2;

use dark::properties::AIAlertLevel;

use super::{
    HAlign, UiCanvas, VAlign,
    dev_params_panel::{FIELD_TOP_Y, PanelRects},
    list_scroll::{self, ListGeometry, ListHit, ScrollHalf},
};

/// Display font for the header and "Done"; the small data font for the rows
/// that read as data - the pairing every list on this frame uses.
const MENU_FONT: &str = "metafont.fon";
const ROW_FONT: &str = "mainfont.fon";

/// Opacity for a widget the pointer is not over, and for the one it is -
/// [`dev_params_panel`](super::dev_params_panel)'s pair, so the two developer
/// pages highlight alike.
const IDLE_OPACITY: f32 = 0.65;
const HOVER_OPACITY: f32 = 1.0;

/// The header, and the label on the framed button that opens this page from
/// the parameter rows.
pub const HEADER_LABEL: &str = "Cheats";
pub const OPEN_LABEL: &str = "Cheats";

/// What a cheat row does when it is clicked. Deliberately a description
/// rather than an [`Effect`](crate::scripts::Effect): the `ui` modules
/// describe the screen, and the host turns the description into the effect it
/// applies - the same split `dev_params_panel` keeps between a clicked row
/// and the registry write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatAction {
    /// Rain these templates around the player, one per ring slot.
    Rain(&'static [i32]),
    /// Set every AI's alertness. `pin` holds it there instead of letting it
    /// decay, so the level is a floor rather than a nudge.
    Alertness { level: AIAlertLevel, pin: bool },
}

/// One entry in the list: the row's label and what clicking it does.
pub struct Cheat {
    label: &'static str,
    action: CheatAction,
}

impl Cheat {
    pub fn action(&self) -> CheatAction {
        self.action
    }
}

/// Every cheat, in screen order. Template ids come from `cargo dq` against the
/// gamesys.
pub static CHEATS: &[Cheat] = &[
    Cheat {
        label: "Rain weapons",
        // The four workhorse weapons, plus a clip for each gun that takes one.
        action: CheatAction::Rain(&[
            -928,  // Wrench
            -17,   // Pistol
            -19,   // Shotgun
            -18,   // Assault Rifle
            -1358, // Small Standard Clip
            -1358, // Small Standard Clip
            -1360, // Small AP Clip
            -42,   // Pellet Shot Box (shotgun shells)
        ]),
    },
    Cheat {
        label: "Rain modules + nanites",
        // The two things a test situation is usually short of.
        action: CheatAction::Rain(&[
            -938, // EXP Cookies (cyber modules)
            -938, -938, -938, //
            -89,  // 20 Nanites
            -89, -89, -89,
        ]),
    },
    // The AI alertness trio, already reachable on the desktop as Alt+G /
    // Alt+C. A headset has no keyboard, so this page is the only way to reach
    // them on the Quest.
    Cheat {
        label: "Hunt me (all AI)",
        // Pinned: the level never decays, so every AI keeps hunting the
        // player's live position until "Calm all" clears it. `High` maps to
        // attack behaviors, which assume the player is already in range.
        action: CheatAction::Alertness {
            level: AIAlertLevel::Moderate,
            pin: true,
        },
    },
    Cheat {
        label: "Alert all AI",
        // The same level unpinned: a nudge that decays again on its own.
        action: CheatAction::Alertness {
            level: AIAlertLevel::Moderate,
            pin: false,
        },
    },
    Cheat {
        label: "Calm all AI",
        action: CheatAction::Alertness {
            level: AIAlertLevel::Lowest,
            pin: false,
        },
    },
];

/// What a click on the page resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatsEvent {
    /// The index into [`CHEATS`] scrolled into the clicked row.
    Row(usize),
    Scroll(ScrollHalf),
    /// Leave the page, back to the parameter rows.
    Done,
}

/// The list's geometry: the same name list the debug-scene launcher uses, so
/// a cheat row and a scene row really are the same row.
fn list(rects: PanelRects) -> ListGeometry {
    list_scroll::name_list(rects.list_rect(), FIELD_TOP_Y)
}

/// The event at a canvas point, if any. Shared by the click and the hover
/// highlight, so the two can never disagree about where a row is.
pub fn hit(rects: PanelRects, scroll: usize, point: Vector2<f32>) -> Option<CheatsEvent> {
    if rects.done_rect().contains(point) {
        return Some(CheatsEvent::Done);
    }
    // A row carries the index of the cheat *scrolled into* it, never its own
    // slot; a rocker half that cannot move is inert. Both rules live in
    // `list_scroll`, so every list obeys the same ones.
    match list(rects).hit(CHEATS.len(), scroll, point)? {
        ListHit::Row(index) => Some(CheatsEvent::Row(index)),
        ListHit::Scroll(half) => Some(CheatsEvent::Scroll(half)),
    }
}

/// Describe the page onto the host's canvas: header, the scrolled rows, the
/// rocker if the list needs one, and "Done". The highlight resolves through
/// the very same [`hit`] the click does.
pub fn draw(
    canvas: &mut UiCanvas,
    rects: PanelRects,
    scroll: usize,
    pointer_canvas: Option<Vector2<f32>>,
) {
    let geometry = list(rects);
    let len = CHEATS.len();
    let hovered = pointer_canvas.and_then(|point| hit(rects, scroll, point));
    let opacity = |event: CheatsEvent| {
        if hovered == Some(event) {
            HOVER_OPACITY
        } else {
            IDLE_OPACITY
        }
    };

    canvas.text_native(
        rects.header_rect(),
        HEADER_LABEL,
        MENU_FONT,
        HAlign::Center,
        VAlign::Middle,
    );

    let rows = geometry.visible_rows(len, scroll);
    for (slot, index) in rows.clone().enumerate() {
        // Fitted, not plain: `text_native` does not shrink to its rect, so a
        // label that outgrows the pane would run over the frame - and over the
        // scroll gutter - instead of ellipsizing inside it.
        canvas
            .text_native_fit(
                geometry.text_rect(len, slot),
                CHEATS[index].label,
                ROW_FONT,
                HAlign::Left,
                VAlign::Middle,
            )
            .opacity(opacity(CheatsEvent::Row(index)));
    }

    if let Some(rocker) = geometry.rocker(len) {
        list_scroll::draw(
            canvas,
            &rocker,
            rows.start,
            geometry.max_scroll(len),
            match hovered {
                Some(CheatsEvent::Scroll(half)) => Some(half),
                _ => None,
            },
        );
    }

    canvas
        .text_native(
            rects.done_rect(),
            "Done",
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        )
        .opacity(opacity(CheatsEvent::Done));
}

/// What the host must act on after a click. Scrolling never reaches it - this
/// page absorbs that itself, exactly as [`dev_params_panel::activate`] absorbs
/// a parameter step.
///
/// [`dev_params_panel::activate`]: super::dev_params_panel::activate
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatsOutcome {
    /// Carry out this row's action.
    Act(CheatAction),
    /// Leave the page.
    Done,
}

/// Apply a clicked event, absorbing the scrolling into the host's `scroll`
/// offset and reporting only what the host has to do.
pub fn activate(
    rects: PanelRects,
    event: CheatsEvent,
    scroll: &mut usize,
) -> Option<CheatsOutcome> {
    match event {
        CheatsEvent::Row(index) => Some(CheatsOutcome::Act(CHEATS[index].action)),
        CheatsEvent::Scroll(half) => {
            list_scroll::apply(half, scroll, list(rects).max_scroll(CHEATS.len()));
            None
        }
        CheatsEvent::Done => Some(CheatsOutcome::Done),
    }
}

#[cfg(test)]
mod tests {
    use super::{super::UiElement, *};

    /// Every shipped cheat is reachable by scrolling, and each row carries its
    /// OWN action - the seam a positional row index would break.
    #[test]
    fn every_cheat_is_reachable_and_carries_its_own_action() {
        let rects = PanelRects::default();
        let geometry = list(rects);
        let max = geometry.max_scroll(CHEATS.len());

        for index in 0..CHEATS.len() {
            let scroll = index.min(max);
            let slot = index - scroll;
            let point = geometry.row_rect(CHEATS.len(), slot).center();
            assert_eq!(
                hit(rects, scroll, point),
                Some(CheatsEvent::Row(index)),
                "cheat {index} ({}) must be clickable at scroll {scroll}",
                CHEATS[index].label
            );

            let mut scroll = scroll;
            assert_eq!(
                activate(rects, CheatsEvent::Row(index), &mut scroll),
                Some(CheatsOutcome::Act(CHEATS[index].action)),
            );
        }
    }

    /// "Done" is the host's business; scrolling is not.
    #[test]
    fn done_leaves_and_scrolling_is_absorbed() {
        let rects = PanelRects::default();
        let mut scroll = 0;

        assert_eq!(
            hit(rects, 0, rects.done_rect().center()),
            Some(CheatsEvent::Done)
        );
        assert_eq!(
            activate(rects, CheatsEvent::Done, &mut scroll),
            Some(CheatsOutcome::Done)
        );

        // Nothing to scroll to with the shipped list, so the offset holds.
        assert_eq!(
            activate(rects, CheatsEvent::Scroll(ScrollHalf::Down), &mut scroll),
            None
        );
        assert_eq!(scroll, list(rects).max_scroll(CHEATS.len()).min(1));
    }

    /// Every row stays inside the pane, clear of the backdrop's painted field.
    #[test]
    fn rows_stay_inside_the_pane() {
        let rects = PanelRects::default();
        let geometry = list(rects);
        let pane = rects.list_rect();

        for slot in 0..geometry.visible_rows(CHEATS.len(), 0).len() {
            let row = geometry.row_rect(CHEATS.len(), slot);
            assert!(row.x >= pane.x, "{row:?}");
            assert!(row.x + row.w <= pane.x + pane.w, "{row:?}");
            assert!(row.y >= pane.y, "{row:?}");
            assert!(row.y + row.h <= FIELD_TOP_Y, "slot {slot}: {row:?}");
        }
    }

    /// A click on bare backdrop does nothing - the page has no catch-all.
    #[test]
    fn clicking_bare_backdrop_does_nothing() {
        let rects = PanelRects::default();
        // Bottom-left of the frame: outside the pane, the rocker and Done.
        assert_eq!(hit(rects, 0, vec2(5.0, 475.0)), None);
    }

    /// The page renders on the developer frame, with a row per cheat.
    #[test]
    fn the_page_draws_a_row_per_cheat() {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        draw(&mut canvas, PanelRects::default(), 0, None);
        let drawn: Vec<&str> = canvas
            .elements()
            .iter()
            .filter_map(|element| match element {
                UiElement::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        for cheat in CHEATS {
            assert!(drawn.contains(&cheat.label), "missing row: {}", cheat.label);
        }
        assert!(drawn.contains(&HEADER_LABEL));
        assert!(drawn.contains(&"Done"));
    }
}
