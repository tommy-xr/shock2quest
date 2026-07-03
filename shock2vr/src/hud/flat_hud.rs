//! Flatscreen (non-VR) screen-space HUD.
//!
//! Where `virtual_arms` renders the HUD as world-space panels on the VR hands,
//! this draws a classic 2D overlay: a centered crosshair plus health/psi bars,
//! described on the shared [`UiCanvas`] at the original game's 640x480 virtual
//! resolution and rendered to a screen-space overlay. Built only in
//! `PresentationMode::Flat`. See `projects/flatscreen-and-vr-architecture.md`.

use cgmath::vec2;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::World;

use super::{
    get_health_percentage, get_psi_percentage, get_wielded_ammo, get_wielded_ammo_icon,
    get_wielded_ammo_type, get_wielded_psi_charge,
};
use crate::runtime_props::{PsiChargePhase, RuntimePropPsiCharge};
use crate::ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign};

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

// Health/PSI meters - positions matching the original SS2 bio-monitor
// layout: a block at (2, 414), with health ABOVE psi (y-offsets 18 and 41)
// and the numeric readouts at the block's x+92. The bar art
// (HPBAR/PSIBAR.PCX) is 80x14. We draw the compact bio-monitor (BIO.PCX,
// 128x64) as the backdrop; it is the left crop of the wider BIOFULL.PCX, so
// the icon/bar/readout offsets below land identically on it.
const METERS_X: f32 = 2.0;
const METERS_Y: f32 = 414.0;
const METERS_W: f32 = 128.0;
const METERS_H: f32 = 64.0;
const BAR_W: f32 = 80.0;
const BAR_H: f32 = 14.0;
const TEXT_W: f32 = 60.0;
const TEXT_SIZE: f32 = 16.0;

/// Bio-monitor backdrop (BIO.PCX, 128x64) the bars/numbers sit on, the
/// health/psi equivalent of the ammo gauge's AMMOBACK frame.
const METERS_BACKDROP: Rect = Rect::new(METERS_X, METERS_Y, METERS_W, METERS_H);

const HEALTH_BAR: Rect = Rect::new(METERS_X + 8.0, METERS_Y + 18.0, BAR_W, BAR_H); // (10, 432)
const PSI_BAR: Rect = Rect::new(METERS_X + 8.0, METERS_Y + 41.0, BAR_W, BAR_H); //   (10, 455)
const HEALTH_TEXT: Rect = Rect::new(METERS_X + 92.0, METERS_Y + 17.0, TEXT_W, BAR_H);
const PSI_TEXT: Rect = Rect::new(METERS_X + 92.0, METERS_Y + 40.0, TEXT_W, BAR_H);

// Ammo gauge - matching the original SS2 HUD: the compact AMMOBACK.PCX
// (94x64) anchored at (544, 414), bottom-right, with the round count drawn
// over it.
const AMMO_X: f32 = 544.0;
const AMMO_Y: f32 = 414.0;
const AMMO_W: f32 = 94.0;
const AMMO_H: f32 = 64.0;
const AMMO_GAUGE: Rect = Rect::new(AMMO_X, AMMO_Y, AMMO_W, AMMO_H);
const AMMO_TEXT: Rect = Rect::new(AMMO_X, AMMO_Y + 22.0, AMMO_W, 20.0); // centered over the gauge
const AMMO_TEXT_SIZE: f32 = 18.0;
// Selected ammo-type indicator: the projectile's object icon (P$ObjIcon) just
// left of the gauge, with its type label (std/he/ap) below the round count.
const AMMO_ICON: Rect = Rect::new(AMMO_X - 40.0, AMMO_Y + 16.0, 32.0, 32.0);
const AMMO_TYPE_TEXT: Rect = Rect::new(AMMO_X, AMMO_Y + 44.0, AMMO_W, 16.0);
const AMMO_TYPE_TEXT_SIZE: f32 = 12.0;

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
pub(crate) fn build_flat_hud_canvas(
    health_fraction: f32,
    psi_fraction: f32,
    psi_charge: Option<RuntimePropPsiCharge>,
    ammo: Option<i32>,
    ammo_icon: Option<String>,
    ammo_type: Option<String>,
) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(VIRTUAL_W, VIRTUAL_H));

    canvas
        .image(CROSSHAIR, "CROSSHAI.PCX")
        // Bio-monitor backdrop first; the bars + numbers render on top of it.
        .image(METERS_BACKDROP, "BIO.PCX")
        .bar(HEALTH_BAR, "HPBAR.PCX", health_fraction)
        .bar(PSI_BAR, "PSIBAR.PCX", psi_fraction);

    let health_pct = (health_fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
    let psi_pct = (psi_fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
    canvas
        .text(
            HEALTH_TEXT,
            &format!("{health_pct}"),
            "mainfont.fon",
            TEXT_SIZE,
            HAlign::Left,
            VAlign::Middle,
        )
        .text(
            PSI_TEXT,
            &format!("{psi_pct}"),
            "mainfont.fon",
            TEXT_SIZE,
            HAlign::Left,
            VAlign::Middle,
        );

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

    // Ammo gauge (only when a weapon with a clip is wielded).
    if let Some(rounds) = ammo {
        canvas.image(AMMO_GAUGE, "AMMOBACK.PCX").text(
            AMMO_TEXT,
            &format!("{rounds}"),
            "mainfont.fon",
            AMMO_TEXT_SIZE,
            HAlign::Center,
            VAlign::Middle,
        );

        // Selected ammo-type indicator: the projectile's icon + type label.
        if let Some(icon) = ammo_icon {
            canvas.image(AMMO_ICON, &icon);
        }
        if let Some(ammo_type) = ammo_type {
            canvas.text(
                AMMO_TYPE_TEXT,
                &ammo_type.to_ascii_uppercase(),
                "mainfont.fon",
                AMMO_TYPE_TEXT_SIZE,
                HAlign::Center,
                VAlign::Middle,
            );
        }
    }

    canvas
}

/// Build and render the flat HUD as screen-space scene objects.
pub(crate) fn create_flat_hud(
    asset_cache: &mut AssetCache,
    world: &World,
    screen_size: cgmath::Vector2<f32>,
) -> Vec<SceneObject> {
    let canvas = build_flat_hud_canvas(
        get_health_percentage(world),
        get_psi_percentage(world),
        get_wielded_psi_charge(world),
        get_wielded_ammo(world),
        get_wielded_ammo_icon(world),
        get_wielded_ammo_type(world),
    );
    // Keep the crosshair square and bars undistorted on non-4:3 windows.
    canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_has_crosshair_bio_backdrop_bars_and_readouts() {
        // Crosshair + bio backdrop + 2 bars + 2 stat numbers = 6 (no weapon).
        let canvas = build_flat_hud_canvas(1.0, 0.75, None, None, None, None);
        assert_eq!(canvas.element_count(), 6);
    }

    #[test]
    fn wielding_a_weapon_adds_the_ammo_gauge() {
        // ...plus the ammo backdrop + count when a clip is present.
        let canvas = build_flat_hud_canvas(1.0, 0.75, None, Some(12), None, None);
        assert_eq!(canvas.element_count(), 8);
    }

    #[test]
    fn ammo_type_adds_icon_and_label() {
        // ...plus the ammo-type icon + label when a type is selected.
        let canvas = build_flat_hud_canvas(
            1.0,
            0.75,
            None,
            Some(12),
            Some("STD_I.PCX".to_string()),
            Some("std".to_string()),
        );
        assert_eq!(canvas.element_count(), 10);
    }

    #[test]
    fn health_sits_above_psi() {
        // Matches the original SS2 HUD: health sits above psi (18 < 41).
        assert!(HEALTH_BAR.y < PSI_BAR.y);
    }

    #[test]
    fn crosshair_is_centered() {
        assert_eq!(CROSSHAIR.center(), vec2(VIRTUAL_W / 2.0, VIRTUAL_H / 2.0));
    }

    #[test]
    fn out_of_range_fractions_do_not_panic() {
        // Fills are clamped inside `UiCanvas::bar`.
        let _ = build_flat_hud_canvas(2.0, -1.0, None, None, None, None);
    }
}
