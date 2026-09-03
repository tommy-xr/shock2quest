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

use super::ammo_panel::{self, AmmoReadout, ReadoutButtonSpec};
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
    let at = |rect| ammo_panel::at(origin, rect);
    canvas
        // Backdrop first; the bars and numbers render on top of it.
        .image(Rect::new(origin.x, origin.y, size.x, size.y), art)
        .bar(at(HEALTH_BAR), "HPBAR.PCX", readout.health_fraction)
        .bar(at(PSI_BAR), "PSIBAR.PCX", readout.psi_fraction);
    let pct = |f: f32| (f.clamp(0.0, 1.0) * 100.0).round() as i32;
    canvas
        .text_native(
            at(HEALTH_TEXT),
            &format!("{}", pct(readout.health_fraction)),
            FONT,
            HAlign::Left,
            VAlign::Middle,
        )
        .text_native(
            at(PSI_TEXT),
            &format!("{}", pct(readout.psi_fraction)),
            FONT,
            HAlign::Left,
            VAlign::Middle,
        );
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
}

/// The readout's clickable controls on the 640x480 canvas.
///
/// The single source of truth for BOTH rendering and pointer hit-testing: the
/// rects come from the shared [`ammo_panel`] layout that drew them, mapped
/// through the same panel origin, so the drawn and clickable regions cannot
/// diverge - in either presentation.
pub(crate) fn buttons(readouts: &UseModeReadouts) -> Vec<ReadoutButtonSpec> {
    if readouts.ammo.is_empty() {
        // The gauge is not drawn, so nothing on it is clickable.
        return Vec::new();
    }
    ammo_panel::buttons(&readouts.ammo)
        .into_iter()
        .map(|spec| ReadoutButtonSpec {
            rect: ammo_panel::at(AMMO_ORIGIN, spec.rect),
            ..spec
        })
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

    #[test]
    fn the_bio_monitor_is_always_drawn() {
        // Backdrop + 2 bars + 2 numbers, with no weapon wielded.
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit_use_mode(&mut canvas, &readouts(None, false));
        assert_eq!(canvas.element_count(), 5);
    }

    #[test]
    fn a_wielded_clip_adds_the_ammo_gauge() {
        // ...plus the AMMOFULL backdrop + the round count.
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit_use_mode(&mut canvas, &readouts(Some(12), false));
        assert_eq!(canvas.element_count(), 7);
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

    /// A readout with nothing to say offers no controls, even though the
    /// composition is only ever drawn in use mode.
    #[test]
    fn an_empty_gauge_offers_no_buttons() {
        assert!(buttons(&readouts(None, true)).is_empty());
        assert_eq!(buttons(&readouts(Some(0), true)).len(), 1);
    }
}
