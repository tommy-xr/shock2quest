//! Flatscreen (non-VR) screen-space HUD.
//!
//! Where `virtual_arms` renders the HUD as world-space panels on the VR hands,
//! this draws a classic 2D overlay: a centered crosshair plus health/psi bars,
//! described on the shared [`UiCanvas`] at the original game's 640x480 virtual
//! resolution and rendered to a screen-space overlay. Built only in
//! `PresentationMode::Flat`. See `projects/flatscreen-and-vr-architecture.md`.

use cgmath::{Vector2, vec2};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::World;

use super::ammo_panel::{self, AmmoReadout};
use super::message_line;
use super::readouts::{self, BioReadout};
use super::{get_health_percentage, get_psi_percentage, get_wielded_psi_charge};
use crate::runtime_props::{PsiChargePhase, RuntimePropPsiCharge};
use crate::ui::{Rect, ScaleMode, UiCanvas};

/// The original SS2 HUD is authored against a 640x480 display.
const VIRTUAL_W: f32 = 640.0;
const VIRTUAL_H: f32 = 480.0;

const CROSSHAIR_SIZE: f32 = 32.0;
const CROSSHAIR: Rect = Rect::new(
    VIRTUAL_W / 2.0 - CROSSHAIR_SIZE / 2.0,
    VIRTUAL_H / 2.0 - CROSSHAIR_SIZE / 2.0,
    CROSSHAIR_SIZE,
    CROSSHAIR_SIZE,
);

/// Bio-monitor anchor: the compact BIO.PCX (128x64) at (2, 414), the left crop
/// of the wider BIOFULL.PCX the interface canvas expands to. The bars and
/// numbers inside it are laid out once in [`readouts`] in panel pixels.
const METERS_ORIGIN: Vector2<f32> = readouts::BIO_ORIGIN;

// Ammo gauge - matching the original SS2 HUD: the compact AMMOBACK.PCX
// (94x64) anchored at (544, 414), bottom-right, with the round count drawn
// over it.
const AMMO_X: f32 = 544.0;
const AMMO_Y: f32 = 414.0;
const AMMO_W: f32 = 94.0;
const AMMO_H: f32 = 64.0;
const AMMO_GAUGE: Rect = Rect::new(AMMO_X, AMMO_Y, AMMO_W, AMMO_H);

/// The ammo readout's panel origin. Its contents are laid out in AMMOFULL
/// panel pixels, and the compact AMMOBACK crop is that panel's right 94 px, so
/// the same origin places them over either backdrop.
const AMMO_PANEL_ORIGIN: Vector2<f32> = readouts::AMMO_ORIGIN;

// Psi overload meter - drawn center-screen below the crosshair while the psi
// amp's trigger is held on an overloadable power (and briefly after release,
// flashing the result). Uses the original meter art (res/iface, 64x16):
// LOADBACK = track with the end-zone baked at the right, LOADMETR = fill,
// LOADGOOD = overload success, LOADBURN = burnout. Drawn at 2x art size.
const OVERLOAD_W: f32 = 128.0;
const OVERLOAD_H: f32 = 32.0;
const OVERLOAD_METER: Rect = Rect::new(
    VIRTUAL_W / 2.0 - OVERLOAD_W / 2.0,
    VIRTUAL_H / 2.0 + CROSSHAIR_SIZE,
    OVERLOAD_W,
    OVERLOAD_H,
);

/// Build the flat HUD as a resolution-independent canvas for the given player
/// stat fractions and (optional) wielded-weapon ammo. Pure (no asset/GL
/// access), so it is unit-testable.
///
/// `use_mode` is the whole shooter/interface split: the original turns the
/// crosshair overlay off while the cursor is up (`ShockOverlayMouseMode`,
/// projects/flat-ui.md §2.1), and the bottom readouts move to the interface
/// canvas - so in use mode this draws neither.
pub(crate) fn build_flat_hud_canvas(
    use_mode: bool,
    health_fraction: f32,
    psi_fraction: f32,
    psi_charge: Option<RuntimePropPsiCharge>,
    ammo_readout: &AmmoReadout,
    messages: &[String],
) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(VIRTUAL_W, VIRTUAL_H));

    if !use_mode {
        canvas.image(CROSSHAIR, "CROSSHAI.PCX");
    }

    // Shooter mode draws the compact readouts here. In use mode both expand to
    // their full art and move onto the shared interface canvas the pointer host
    // owns (`readouts::emit_use_mode`), so the VR cyber interface carries the
    // same two panels the flat cursor clicks - one emit, not one per
    // presentation.
    if !use_mode {
        readouts::emit_bio(
            &mut canvas,
            METERS_ORIGIN,
            "BIO.PCX",
            readouts::BIO_SIZE,
            &BioReadout {
                health_fraction,
                psi_fraction,
            },
        );
    }

    // Psi overload meter (only while the amp is charging / flashing a result).
    if let Some(charge) = psi_charge {
        match charge.phase {
            PsiChargePhase::Charging => {
                canvas.image(OVERLOAD_METER, "LOADBACK.PCX").bar(
                    OVERLOAD_METER,
                    "LOADMETR.PCX",
                    charge.fraction,
                );
            }
            PsiChargePhase::Overloaded => {
                canvas.image(OVERLOAD_METER, "LOADGOOD.PCX");
            }
            PsiChargePhase::Burnout => {
                canvas.image(OVERLOAD_METER, "LOADBURN.PCX");
            }
        }
    }

    // Compact ammo gauge (a weapon with a clip, or the psi amp's selected
    // discipline in its place), over the AMMOBACK crop. Everything drawn inside
    // the panel is placed by the shared `ammo_panel` layout the VR forearm and
    // the expanded use-mode readout use.
    if !use_mode && !ammo_readout.is_empty() {
        canvas.image(AMMO_GAUGE, "AMMOBACK.PCX");
        ammo_panel::emit(&mut canvas, AMMO_PANEL_ORIGIN, ammo_readout);
    }

    // Status messages, placed by the shared `message_line` layout the VR head
    // panel draws with.
    message_line::emit(&mut canvas, message_line::flat_origin(), messages);

    canvas
}

/// Build and render the flat HUD as screen-space scene objects. In `use_mode`
/// (Tab metagame mode) the bottom readouts are drawn by the interface canvas
/// instead, expanded to BIOFULL/AMMOFULL, so the HUD leaves them out.
pub(crate) fn create_flat_hud(
    asset_cache: &mut AssetCache,
    world: &World,
    screen_size: cgmath::Vector2<f32>,
    use_mode: bool,
    messages: &[String],
) -> Vec<SceneObject> {
    let mut canvas = build_flat_hud_canvas(
        use_mode,
        get_health_percentage(world),
        get_psi_percentage(world),
        get_wielded_psi_charge(world),
        // The compact gauge has no clickable controls: the pointer only exists
        // in use mode, where the interface canvas draws the expanded panel.
        &AmmoReadout::from_world(world, false),
        messages,
    );
    if !use_mode {
        super::hazards::emit(
            &mut canvas,
            super::hazards::SCREEN_ORIGIN,
            &super::hazards::HazardReadout::from_world(world),
            false,
        );
    }
    // Keep the crosshair square and bars undistorted on non-4:3 windows.
    canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AmmoReadout` for the common test shapes. `cycle` stands in for a
    /// multi-ammo weapon; nothing else offers a button.
    fn readout(
        ammo: Option<i32>,
        ammo_icon: Option<&str>,
        ammo_type: Option<&str>,
        cycle: bool,
    ) -> AmmoReadout {
        AmmoReadout {
            ammo,
            ammo_icon: ammo_icon.map(str::to_string),
            ammo_type: ammo_type.map(str::to_string),
            can_cycle_ammo: cycle,
            show_buttons: cycle,
            ..Default::default()
        }
    }

    #[test]
    fn canvas_has_crosshair_bio_backdrop_bars_and_readouts() {
        // Crosshair + bio backdrop + 2 bars + 2 stat numbers = 6 (no weapon).
        let canvas = build_flat_hud_canvas(
            false,
            1.0,
            0.75,
            None,
            &readout(None, None, None, false),
            &[],
        );
        assert_eq!(canvas.element_count(), 6);
    }

    #[test]
    fn wielding_a_weapon_adds_the_ammo_gauge() {
        // ...plus the ammo backdrop + count when a clip is present.
        let canvas = build_flat_hud_canvas(
            false,
            1.0,
            0.75,
            None,
            &readout(Some(12), None, None, false),
            &[],
        );
        assert_eq!(canvas.element_count(), 8);
    }

    #[test]
    fn ammo_type_adds_icon_and_label() {
        // ...plus the ammo-type icon + label when a type is selected.
        let canvas = build_flat_hud_canvas(
            false,
            1.0,
            0.75,
            None,
            &readout(Some(12), Some("STD_I.PCX"), Some("std"), false),
            &[],
        );
        assert_eq!(canvas.element_count(), 10);
    }

    #[test]
    fn psi_amp_shows_discipline_instead_of_clip() {
        // Base 6 + gauge backdrop + tier badge + discipline name = 9; the clip
        // readout is suppressed even though the amp has ammo=0.
        let canvas = build_flat_hud_canvas(
            false,
            1.0,
            0.75,
            None,
            &AmmoReadout {
                psi_power: Some(("Projected Cryokinesis".to_string(), 1)),
                ammo: Some(0),
                ..Default::default()
            },
            &[],
        );
        assert_eq!(canvas.element_count(), 9);
    }

    /// Use mode hands the bottom readouts to the interface canvas and turns
    /// the crosshair off (the original's `ShockOverlayMouseMode`), so the HUD
    /// keeps only the overlays that are its own - here, none.
    #[test]
    fn use_mode_leaves_the_bottom_readouts_to_the_interface_canvas() {
        let shooter = build_flat_hud_canvas(
            false,
            1.0,
            0.75,
            None,
            &readout(Some(12), None, None, false),
            &[],
        );
        assert_eq!(shooter.element_count(), 8);
        let use_mode = build_flat_hud_canvas(
            true,
            1.0,
            0.75,
            None,
            &readout(Some(12), None, None, false),
            &[],
        );
        assert_eq!(use_mode.element_count(), 0);
    }

    /// The psi overload meter is the HUD's own overlay and stays up in use
    /// mode - it is not part of the readout pair that moved.
    #[test]
    fn the_overload_meter_survives_use_mode() {
        let canvas = build_flat_hud_canvas(
            true,
            1.0,
            0.75,
            Some(RuntimePropPsiCharge {
                phase: PsiChargePhase::Charging,
                fraction: 0.5,
            }),
            &readout(Some(12), None, None, false),
            &[],
        );
        // The charging meter is a backdrop + its fill.
        assert_eq!(canvas.element_count(), 2);
    }

    /// The readout proper stays inside the COMPACT gauge too, so shooter mode
    /// is not drawing half the readout onto bare 3D view.
    #[test]
    fn the_readout_sits_inside_the_compact_gauge() {
        for rect in [
            ammo_panel::COUNT,
            ammo_panel::ICON,
            ammo_panel::TYPE_LABEL,
            ammo_panel::CONDITION,
            ammo_panel::PSI_TIER_BADGE,
            ammo_panel::PSI_POWER_NAME,
        ] {
            let placed = ammo_panel::at(AMMO_PANEL_ORIGIN, rect);
            assert!(AMMO_GAUGE.x <= placed.x, "{placed:?} left of AMMOBACK");
            assert!(
                placed.x + placed.w <= AMMO_GAUGE.x + AMMO_GAUGE.w,
                "{placed:?} right of AMMOBACK"
            );
        }
    }

    #[test]
    fn crosshair_is_centered() {
        assert_eq!(CROSSHAIR.center(), vec2(VIRTUAL_W / 2.0, VIRTUAL_H / 2.0));
    }

    /// A status message adds one text element per line, at the shared
    /// `message_line` block's origin on the HUD canvas.
    #[test]
    fn a_status_message_adds_a_line_to_the_hud() {
        let empty = readout(None, None, None, false);
        let base = build_flat_hud_canvas(false, 1.0, 0.75, None, &empty, &[]);
        let with_message = build_flat_hud_canvas(
            false,
            1.0,
            0.75,
            None,
            &empty,
            &["This lift has been taken offline for repairs.".to_string()],
        );

        assert_eq!(with_message.element_count(), base.element_count() + 1);
        let line = with_message.elements().last().unwrap().rect();
        assert_eq!(vec2(line.x, line.y), message_line::flat_origin());
    }

    #[test]
    fn out_of_range_fractions_do_not_panic() {
        // Fills are clamped inside `UiCanvas::bar`.
        let _ = build_flat_hud_canvas(
            false,
            2.0,
            -1.0,
            None,
            &readout(None, None, None, false),
            &[],
        );
    }
}
