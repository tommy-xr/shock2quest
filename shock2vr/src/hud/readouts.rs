//! The use-mode readouts - decided ONCE, in panel pixels.
//!
//! The original SS2 HUD's two bottom readouts are the bio monitor (health and
//! psi) at the left and the ammo gauge at the right. In shooter mode flatscreen
//! draws their compact crops (BIO.PCX, AMMOBACK.PCX) as a screen overlay; in
//! **use mode** both expand to their full art (BIOFULL.PCX, AMMOFULL.PCX) and
//! the ammo panel's controls become clickable.
//!
//! The expanded pair is emitted by [`emit_use_mode`] onto the shared interface
//! canvas the pointer host owns, so flatscreen's cursor and the VR cyber
//! interface's controller ray meet the same readouts at the same pixels
//! (AGENTS.md section 3). Nothing here knows which presentation it draws for.
//!
//! Bio rects are panel-local - (0,0) is the bio panel's upper-left corner -
//! exactly as [`super::ammo_panel`]'s are for the ammo panel.

use cgmath::{Vector2, vec2};
use shipyard::World;

use super::ammo_panel::{self, AmmoReadout, ReadoutButton, ReadoutButtonSpec};
use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// The font the bio numbers use (the original's HUD font).
const FONT: &str = "mainfont.fon";

/// The bio panel's compact (BIO.PCX) and expanded (BIOFULL.PCX) art sizes.
/// BIO is the left crop of BIOFULL, so the bars and numbers below land
/// identically on either.
pub(crate) const BIO_SIZE: Vector2<f32> = vec2(128.0, 64.0);
pub(crate) const BIO_FULL_SIZE: Vector2<f32> = vec2(260.0, 64.0);

/// Health above psi, matching the original bio monitor: bars at panel x 8,
/// y-offsets 18 and 41 (the bar art HPBAR/PSIBAR.PCX is 80x14), numbers at
/// panel x 92.
const BAR_W: f32 = 80.0;
const BAR_H: f32 = 14.0;
const TEXT_W: f32 = 60.0;
pub(crate) const HEALTH_BAR: Rect = Rect::new(8.0, 18.0, BAR_W, BAR_H);
pub(crate) const PSI_BAR: Rect = Rect::new(8.0, 41.0, BAR_W, BAR_H);
pub(crate) const HEALTH_TEXT: Rect = Rect::new(92.0, 17.0, TEXT_W, BAR_H);
pub(crate) const PSI_TEXT: Rect = Rect::new(92.0, 40.0, TEXT_W, BAR_H);

/// Where the two expanded readouts sit on the 640x480 canvas. The bio panel
/// keeps the compact panel's anchor (the original meters rect is
/// `{{2,414},{262,478}}`; only the art widens), and the ammo panel expands
/// LEFT from the compact gauge's (544,414) by 166 px (AMMOBACK 94 ->
/// AMMOFULL 260).
pub(crate) const BIO_ORIGIN: Vector2<f32> = vec2(2.0, 414.0);
pub(crate) const AMMO_ORIGIN: Vector2<f32> = vec2(378.0, 414.0);

/// The system-menu affordance: the pause menu's only *discoverable* control in
/// VR, since the Menu button's long press advertises nothing until it is held.
/// IFBTN00.PCX is the original's blank 38x36 interface button.
///
/// Placed in the empty column between the interface's occupied regions - below
/// the inventory strip (`(2,0)`, 636x121), right of the left MFD slot
/// (`(2,124)`, 188x300) and its close gadget, left of the reserved right slot
/// (x 450), and above both bottom readouts (y 414) - and horizontally centred
/// on the canvas.
pub(crate) const SYSTEM_BUTTON: Rect = Rect::new(288.0, 372.0, 64.0, 36.0);

/// The system button as one spec, so [`emit_use_mode`] draws exactly what
/// [`buttons`] hit-tests.
fn system_button() -> ReadoutButtonSpec {
    ReadoutButtonSpec {
        button: ReadoutButton::SystemMenu,
        rect: SYSTEM_BUTTON,
        // The original's blank interface-button plate, widened from its
        // authored 38 px so the label sits inside the bevel rather than on it.
        texture: Some("IFBTN00.PCX"),
        text: Some("MENU".to_owned()),
    }
}

/// What the bio monitor says this frame. Presentation-agnostic, like
/// [`AmmoReadout`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct BioReadout {
    pub health_fraction: f32,
    pub psi_fraction: f32,
}

impl BioReadout {
    pub(crate) fn from_world(world: &World) -> Self {
        Self {
            health_fraction: super::get_health_percentage(world),
            psi_fraction: super::get_psi_percentage(world),
        }
    }
}

/// Emit the bio monitor into `canvas` with its panel's upper-left corner at
/// `origin`. `art`/`size` are the caller's backdrop (compact BIO or expanded
/// BIOFULL); every *placement* lives here.
pub(crate) fn emit_bio(
    canvas: &mut UiCanvas,
    origin: Vector2<f32>,
    art: &str,
    size: Vector2<f32>,
    readout: &BioReadout,
) {
    // Backdrop first; the bars and numbers render on top of it.
    canvas.image(Rect::new(origin.x, origin.y, size.x, size.y), art);
    emit_bio_overlays(canvas, origin, readout);
}

/// The bio monitor's bars and numbers alone, with the panel's upper-left corner
/// at `origin` - everything [`emit_bio`] draws except the backdrop art.
fn emit_bio_overlays(canvas: &mut UiCanvas, origin: Vector2<f32>, readout: &BioReadout) {
    emit_health(canvas, origin, readout.health_fraction);
    let at = |rect| ammo_panel::at(origin, rect);
    canvas
        .bar(at(PSI_BAR), "PSIBAR.PCX", readout.psi_fraction)
        .text_native(
            at(PSI_TEXT),
            &percent(readout.psi_fraction),
            FONT,
            HAlign::Left,
            VAlign::Middle,
        );
}

/// The health row's bar and number, with the bio panel's upper-left corner at
/// `origin`.
///
/// Split out for the VR wrist watch, which wears only this row (health is the
/// one stat on the watch; psi lives on the psi amp's own readout) - so the
/// watch and the interface place the bar and the number by the same rects, off
/// the same panel corner, and cannot drift (AGENTS.md section 3).
fn emit_health(canvas: &mut UiCanvas, origin: Vector2<f32>, fraction: f32) {
    let at = |rect| ammo_panel::at(origin, rect);
    canvas
        .bar(at(HEALTH_BAR), "HPBAR.PCX", fraction)
        .text_native(
            at(HEALTH_TEXT),
            &percent(fraction),
            FONT,
            HAlign::Left,
            VAlign::Middle,
        );
}

fn percent(fraction: f32) -> String {
    format!("{}", (fraction.clamp(0.0, 1.0) * 100.0).round() as i32)
}

/// The wrist watch's face: the compact bio plate's HEALTH row, cropped out of
/// BIO.PCX (whose psi row sits below it), with the live bar and number on top.
///
/// The crop's top and bottom edges are the bands of art between the plate's
/// bezel and the psi row; its left and right are the plate's own, so the
/// cross-icon well, the bar recess and the number well all survive. Outside
/// this box BIO.PCX is palette index 0 - the cyan key colour, which ordinary
/// UI art does not treat as transparent - so a taller crop would frame the
/// watch in cyan.
pub(crate) const WATCH_CROP: Rect = Rect::new(0.0, 15.0, 128.0, 21.0);

/// The watch canvas is exactly its crop.
pub(crate) const WATCH_CROP_SIZE: Vector2<f32> = vec2(WATCH_CROP.w, WATCH_CROP.h);

/// The watch face as its own canvas: the cropped plate at canvas origin, with
/// the health row's own elements placed by [`emit_health`] off the panel corner
/// the crop was taken from (hence the negative origin, exactly as the flat HUD
/// anchors the ammo panel's contents over the compact AMMOBACK crop).
pub(crate) fn build_watch_canvas(readout: &BioReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(WATCH_CROP_SIZE);
    canvas.cropped_image(
        Rect::new(0.0, 0.0, WATCH_CROP.w, WATCH_CROP.h),
        "BIO.PCX",
        WATCH_CROP,
        BIO_SIZE,
    );
    emit_health(
        &mut canvas,
        vec2(-WATCH_CROP.x, -WATCH_CROP.y),
        readout.health_fraction,
    );
    canvas
}

/// Both use-mode readouts, as the interface canvas carries them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct UseModeReadouts {
    pub bio: BioReadout,
    pub ammo: AmmoReadout,
}

impl UseModeReadouts {
    /// Read both readouts from the world. The ammo panel's controls are always
    /// shown here: this composition is drawn only in use mode, which is exactly
    /// when a pointer can reach them.
    pub(crate) fn from_world(world: &World) -> Self {
        Self {
            bio: BioReadout::from_world(world),
            ammo: AmmoReadout::from_world(world, true),
        }
    }
}

/// Emit the expanded bio + ammo readouts along the bottom of the shared
/// 640x480 interface canvas.
pub(crate) fn emit_use_mode(canvas: &mut UiCanvas, readouts: &UseModeReadouts) {
    emit_bio(
        canvas,
        BIO_ORIGIN,
        "BIOFULL.PCX",
        BIO_FULL_SIZE,
        &readouts.bio,
    );
    // No weapon with a clip and no psi power wielded: the gauge is not drawn
    // at all, exactly as in shooter mode.
    if !readouts.ammo.is_empty() {
        canvas.image(
            Rect::new(
                AMMO_ORIGIN.x,
                AMMO_ORIGIN.y,
                ammo_panel::PANEL_W,
                ammo_panel::PANEL_H,
            ),
            "AMMOFULL.PCX",
        );
        ammo_panel::emit(canvas, AMMO_ORIGIN, &readouts.ammo);
    }
    // Always: the way out of the game does not depend on what is wielded.
    let system = system_button();
    ammo_panel::draw_button(canvas, &system, system.rect);
}

/// The readout's clickable controls on the 640x480 canvas.
///
/// The single source of truth for BOTH rendering and pointer hit-testing: the
/// rects come from the shared [`ammo_panel`] layout that drew them, mapped
/// through the same panel origin, so the drawn and clickable regions cannot
/// diverge - in either presentation.
pub(crate) fn buttons(readouts: &UseModeReadouts) -> Vec<ReadoutButtonSpec> {
    // Drawn unconditionally above, so it is clickable unconditionally.
    let system = system_button();
    if readouts.ammo.is_empty() {
        // The gauge is not drawn, so nothing on it is clickable.
        return vec![system];
    }
    ammo_panel::buttons(&readouts.ammo)
        .into_iter()
        .map(|spec| ReadoutButtonSpec {
            rect: ammo_panel::at(AMMO_ORIGIN, spec.rect),
            ..spec
        })
        .chain([system])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readouts(ammo: Option<i32>, cycle: bool) -> UseModeReadouts {
        UseModeReadouts {
            bio: BioReadout {
                health_fraction: 1.0,
                psi_fraction: 0.75,
            },
            ammo: AmmoReadout {
                ammo,
                can_cycle_ammo: cycle,
                show_buttons: cycle,
                ..Default::default()
            },
        }
    }

    /// The system button's art + label, drawn whatever else the canvas shows.
    const SYSTEM_ELEMENTS: usize = 2;

    #[test]
    fn the_bio_monitor_is_always_drawn() {
        // Backdrop + 2 bars + 2 numbers, with no weapon wielded.
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit_use_mode(&mut canvas, &readouts(None, false));
        assert_eq!(canvas.element_count(), 5 + SYSTEM_ELEMENTS);
    }

    #[test]
    fn a_wielded_clip_adds_the_ammo_gauge() {
        // ...plus the AMMOFULL backdrop + the round count.
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit_use_mode(&mut canvas, &readouts(Some(12), false));
        assert_eq!(canvas.element_count(), 7 + SYSTEM_ELEMENTS);
    }

    /// The interface's way to the pause menu is on the canvas whatever is
    /// wielded, and is hit-tested at exactly the rect it was drawn at - one
    /// list for both, so a VR ray reaches what a flat cursor does.
    #[test]
    fn the_system_button_is_drawn_and_clickable_at_the_same_rect() {
        for readouts in [readouts(None, false), readouts(Some(12), true)] {
            let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
            emit_use_mode(&mut canvas, &readouts);

            let drawn: Vec<_> = canvas
                .elements()
                .iter()
                .filter(|element| element.rect() == SYSTEM_BUTTON)
                .collect();
            assert_eq!(drawn.len(), 2, "art + label");

            let clickable = buttons(&readouts)
                .into_iter()
                .find(|spec| spec.button == ReadoutButton::SystemMenu)
                .expect("the system button is always clickable");
            assert_eq!(clickable.rect, SYSTEM_BUTTON);
        }
    }

    /// It sits in the canvas's one free column: clear of the strip, the left
    /// MFD slot, the reserved right slot and both bottom readouts.
    #[test]
    fn the_system_button_collides_with_nothing_on_the_canvas() {
        assert!(SYSTEM_BUTTON.y >= 121.0, "below the inventory strip");
        assert!(SYSTEM_BUTTON.x >= 190.0, "right of the left MFD slot");
        assert!(
            SYSTEM_BUTTON.x + SYSTEM_BUTTON.w <= 450.0,
            "left of the right slot"
        );
        assert!(
            SYSTEM_BUTTON.y + SYSTEM_BUTTON.h <= BIO_ORIGIN.y,
            "above the bottom readouts"
        );
    }

    #[test]
    fn health_sits_above_psi() {
        assert!(HEALTH_BAR.y < PSI_BAR.y);
    }

    /// Both readouts sit inside the canvas - the interface panel presents the
    /// whole 640x480, so an off-canvas readout would simply be missing in VR.
    #[test]
    fn both_readouts_land_inside_the_canvas() {
        for (origin, w) in [
            (BIO_ORIGIN, BIO_FULL_SIZE.x),
            (AMMO_ORIGIN, ammo_panel::PANEL_W),
        ] {
            assert!(origin.x >= 0.0 && origin.x + w <= 640.0, "{origin:?}");
            assert!(origin.y >= 0.0 && origin.y + ammo_panel::PANEL_H <= 480.0);
        }
    }

    /// Every control the layout offers lands inside the drawn AMMOFULL panel.
    #[test]
    fn every_readout_button_sits_in_the_ammofull_panel() {
        for rect in [
            ammo_panel::CYCLE_BUTTON,
            ammo_panel::SETTING_BUTTON,
            ammo_panel::RELOAD_BUTTON,
            ammo_panel::PSI_TIER_PREV,
            ammo_panel::PSI_TIER_NEXT,
            ammo_panel::PSI_POWER_PREV,
            ammo_panel::PSI_POWER_NEXT,
        ] {
            let placed = ammo_panel::at(AMMO_ORIGIN, rect);
            assert!(AMMO_ORIGIN.x <= placed.x, "{placed:?}");
            assert!(placed.x + placed.w <= AMMO_ORIGIN.x + ammo_panel::PANEL_W);
        }
    }

    /// The VR wrist watch and the interface canvas draw ONE health layout: the
    /// watch's bar and number land at the interface's rects, shifted by the
    /// crop it wears. A watch that re-derived its own bar would drift from the
    /// interface silently (issue #1268).
    #[test]
    fn the_watch_is_the_interface_health_row_shifted_by_its_crop() {
        let bio = BioReadout {
            health_fraction: 0.4,
            psi_fraction: 0.9,
        };

        let mut interface = UiCanvas::new(vec2(640.0, 480.0));
        emit_bio(
            &mut interface,
            BIO_ORIGIN,
            "BIOFULL.PCX",
            BIO_FULL_SIZE,
            &bio,
        );

        let watch = build_watch_canvas(&bio);
        assert_eq!(watch.size(), WATCH_CROP_SIZE);
        // The plate crop, then the health bar and number - health only.
        assert_eq!(watch.element_count(), 3);

        // Element 0 of the interface is its backdrop; 1 and 2 are the health
        // row, which is what the watch shows after its own plate.
        for (on_watch, on_panel) in watch.elements()[1..]
            .iter()
            .zip(&interface.elements()[1..3])
        {
            let (w, panel) = (on_watch.rect(), on_panel.rect());
            assert_eq!(
                w.x + BIO_ORIGIN.x + WATCH_CROP.x,
                panel.x,
                "{w:?} {panel:?}"
            );
            assert_eq!(
                w.y + BIO_ORIGIN.y + WATCH_CROP.y,
                panel.y,
                "{w:?} {panel:?}"
            );
            assert_eq!((w.w, w.h), (panel.w, panel.h));
        }
    }

    /// Everything the watch draws lands inside the crop it wears - a bar or a
    /// number placed off the cropped plate would simply hang in the air on the
    /// player's wrist.
    #[test]
    fn the_watch_face_contains_its_own_readout() {
        let watch = build_watch_canvas(&BioReadout {
            health_fraction: 1.0,
            psi_fraction: 0.0,
        });
        for element in &watch.elements()[1..] {
            let rect = element.rect();
            assert!(rect.x >= 0.0 && rect.y >= 0.0, "{rect:?}");
            assert!(rect.y + rect.h <= WATCH_CROP.h, "{rect:?}");
        }
        // The health number's widget box runs past the compact plate's right
        // edge (the original authors it against the wider BIOFULL); it is
        // left-aligned, so the glyphs sit in the number well regardless.
        assert!(HEALTH_BAR.x + HEALTH_BAR.w <= WATCH_CROP.w);
        assert!(HEALTH_TEXT.x < WATCH_CROP.w);
    }

    /// The crop is the health row: below the plate's cyan-keyed bezel and above
    /// the psi row the watch deliberately drops.
    #[test]
    fn the_watch_crop_holds_the_health_row_and_not_the_psi_row() {
        assert!(WATCH_CROP.y < HEALTH_BAR.y);
        assert!(HEALTH_BAR.y + HEALTH_BAR.h <= WATCH_CROP.y + WATCH_CROP.h);
        assert!(WATCH_CROP.y + WATCH_CROP.h <= PSI_BAR.y);
        assert!(WATCH_CROP.x + WATCH_CROP.w <= BIO_SIZE.x);
    }

    /// A readout with nothing to say offers no controls, even though the
    /// composition is only ever drawn in use mode.
    #[test]
    fn an_empty_gauge_offers_no_gauge_buttons() {
        let gauge = |r| {
            buttons(&r)
                .into_iter()
                .filter(|spec| spec.button != ReadoutButton::SystemMenu)
                .count()
        };
        assert_eq!(gauge(readouts(None, true)), 0);
        assert_eq!(gauge(readouts(Some(0), true)), 1);
    }
}
