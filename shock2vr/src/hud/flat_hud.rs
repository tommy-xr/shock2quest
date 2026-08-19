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
use super::{
    get_health_percentage, get_psi_percentage, get_wielded_ammo, get_wielded_psi_charge,
    get_wielded_psi_power,
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

// Use-mode expanded readouts (flat UI 5). Anchors match the original layout:
//  - BIOFULL: the original meters rect is {{2,414},{262,478}} - the bio
//    panel stays at (2,414); only the backdrop art widens 128->260 (BIO.PCX is
//    the left crop of BIOFULL.PCX, so the bars/numbers land identically). The
//    right half is baked research/query/map + nanite/cyber chrome (art stub).
//  - AMMOFULL: use ("mouse") mode expands the ammo panel LEFT by
//    166 (AMMOBACK 94 -> AMMOFULL 260), UL = (378,414); the
//    ammo-type CYCLE button is {{186,15},{198,56}} panel-local
//    (art ammoarw0/1), i.e. canvas (564,429,12,41). The round count/icon stay
//    in the panel's right gauge (the AMMOBACK footprint), so their compact
//    offsets are reused.
const METERS_FULL_W: f32 = 260.0;
const AMMO_FULL_MODE_DX: f32 = 166.0;
const AMMO_FULL_X: f32 = AMMO_X - AMMO_FULL_MODE_DX; // 378
const AMMO_FULL_GAUGE: Rect = Rect::new(AMMO_FULL_X, AMMO_Y, METERS_FULL_W, AMMO_H);
/// Where the AMMOFULL panel's upper-left corner lands on the 640x480 HUD
/// canvas. Everything *inside* the panel (round count, ammo icon and label,
/// psi discipline, the cycle button) is laid out once in [`ammo_panel`] in
/// panel pixels and placed relative to this - the VR forearm draws the same
/// panel with its own origin.
const AMMO_PANEL_ORIGIN: Vector2<f32> = vec2(AMMO_FULL_X, AMMO_Y);
/// The AMMOFULL ammo-type cycle button (the original's cycle hotspot, ammoarw
/// art). Clicking it cycles the wielded weapon's ammo type. Exposed so the
/// flat pointer host can hit-test the same rect it is drawn at.
pub(crate) const AMMO_CYCLE_BUTTON: Rect =
    ammo_panel::at(AMMO_PANEL_ORIGIN, ammo_panel::CYCLE_BUTTON);

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
/// access), so it is unit-testable. `crosshair` is false in use mode - the
/// original turns the crosshair overlay off while the cursor is up
/// (`ShockOverlayMouseMode`, projects/flat-ui.md §2.1).
pub(crate) fn build_flat_hud_canvas(
    crosshair: bool,
    use_mode: bool,
    health_fraction: f32,
    psi_fraction: f32,
    psi_charge: Option<RuntimePropPsiCharge>,
    ammo_readout: &AmmoReadout,
) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(VIRTUAL_W, VIRTUAL_H));

    // Use mode expands the compact bottom readouts in place (flat UI 5): the
    // bio panel widens to BIOFULL and the ammo gauge to AMMOFULL. The bars,
    // numbers, and ammo count/icon keep their compact offsets (the FULL art is
    // a superset with the same crops), so only the backdrops swap.
    let (meters_backdrop, meters_art) = if use_mode {
        (
            Rect::new(METERS_X, METERS_Y, METERS_FULL_W, METERS_H),
            "BIOFULL.PCX",
        )
    } else {
        (METERS_BACKDROP, "BIO.PCX")
    };
    let (ammo_backdrop, ammo_art) = if use_mode {
        (AMMO_FULL_GAUGE, "AMMOFULL.PCX")
    } else {
        (AMMO_GAUGE, "AMMOBACK.PCX")
    };

    if crosshair {
        canvas.image(CROSSHAIR, "CROSSHAI.PCX");
    }
    canvas
        // Bio-monitor backdrop first; the bars + numbers render on top of it.
        .image(meters_backdrop, meters_art)
        .bar(HEALTH_BAR, "HPBAR.PCX", health_fraction)
        .bar(PSI_BAR, "PSIBAR.PCX", psi_fraction);

    let health_pct = (health_fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
    let psi_pct = (psi_fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
    canvas
        .text_native(
            HEALTH_TEXT,
            &format!("{health_pct}"),
            "mainfont.fon",
            HAlign::Left,
            VAlign::Middle,
        )
        .text_native(
            PSI_TEXT,
            &format!("{psi_pct}"),
            "mainfont.fon",
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

    // Ammo gauge (a weapon with a clip, or the psi amp's selected discipline
    // in its place). The backdrop art is flat's to choose - use mode expands
    // it - but everything drawn inside the panel is placed by the shared
    // `ammo_panel` layout the VR forearm uses.
    if !ammo_readout.is_empty() {
        canvas.image(ammo_backdrop, ammo_art);
        ammo_panel::emit(&mut canvas, AMMO_PANEL_ORIGIN, ammo_readout);
    }

    canvas
}

/// Build and render the flat HUD as screen-space scene objects. `use_mode`
/// (Tab metagame mode) expands the compact readouts to BIOFULL/AMMOFULL.
pub(crate) fn create_flat_hud(
    asset_cache: &mut AssetCache,
    world: &World,
    screen_size: cgmath::Vector2<f32>,
    crosshair: bool,
    use_mode: bool,
) -> Vec<SceneObject> {
    let canvas = build_flat_hud_canvas(
        crosshair,
        use_mode,
        get_health_percentage(world),
        get_psi_percentage(world),
        get_wielded_psi_charge(world),
        // The same predicate the pointer hit-test uses, so drawn == clickable.
        &AmmoReadout::from_world(world, ammo_cycle_button_visible(world, use_mode)),
    );
    // Keep the crosshair square and bars undistorted on non-4:3 windows.
    canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
}

/// Whether the wielded weapon may cycle ammo (empty, with 2+ selectable
/// projectile types). Uses the same predicate as `cycle_ammo`.
pub(crate) fn can_cycle_wielded_ammo(world: &World) -> bool {
    let Some(weapon) = crate::wielded_weapon::wielded_weapon(world) else {
        return false;
    };
    crate::scripts::script_util::can_cycle_ammo(world, weapon)
}

/// The single source of truth for whether the AMMOFULL ammo-cycle button is
/// shown/active this frame - used for BOTH rendering (via `create_flat_hud`'s
/// `can_cycle_ammo`) and pointer hit-testing (`mission_core`), so the drawn and
/// clickable regions never diverge. Requires use mode, a wielded gun with a
/// empty clip, 2+ ammo types, and no psi-amp display (which replaces the ammo
/// section - `build_flat_hud_canvas`'s psi-power early return).
pub(crate) fn ammo_cycle_button_visible(world: &World, use_mode: bool) -> bool {
    use_mode
        && get_wielded_psi_power(world).is_none()
        && get_wielded_ammo(world).is_some()
        && can_cycle_wielded_ammo(world)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AmmoReadout` for the common test shapes.
    fn readout(
        ammo: Option<i32>,
        ammo_icon: Option<&str>,
        ammo_type: Option<&str>,
        show_cycle_button: bool,
    ) -> AmmoReadout {
        AmmoReadout {
            psi_power: None,
            ammo,
            ammo_icon: ammo_icon.map(str::to_string),
            ammo_type: ammo_type.map(str::to_string),
            show_cycle_button,
        }
    }

    #[test]
    fn canvas_has_crosshair_bio_backdrop_bars_and_readouts() {
        // Crosshair + bio backdrop + 2 bars + 2 stat numbers = 6 (no weapon).
        let canvas = build_flat_hud_canvas(
            true,
            false,
            1.0,
            0.75,
            None,
            &readout(None, None, None, false),
        );
        assert_eq!(canvas.element_count(), 6);
    }

    #[test]
    fn wielding_a_weapon_adds_the_ammo_gauge() {
        // ...plus the ammo backdrop + count when a clip is present.
        let canvas = build_flat_hud_canvas(
            true,
            false,
            1.0,
            0.75,
            None,
            &readout(Some(12), None, None, false),
        );
        assert_eq!(canvas.element_count(), 8);
    }

    #[test]
    fn ammo_type_adds_icon_and_label() {
        // ...plus the ammo-type icon + label when a type is selected.
        let canvas = build_flat_hud_canvas(
            true,
            false,
            1.0,
            0.75,
            None,
            &readout(Some(12), Some("STD_I.PCX"), Some("std"), false),
        );
        assert_eq!(canvas.element_count(), 10);
    }

    #[test]
    fn psi_amp_shows_discipline_instead_of_clip() {
        // Base 6 + gauge backdrop + tier badge + tier count + name = 10;
        // the clip readout is suppressed even though the amp has ammo=0.
        let canvas = build_flat_hud_canvas(
            true,
            false,
            1.0,
            0.75,
            None,
            &AmmoReadout {
                psi_power: Some(("Projected Cryokinesis".to_string(), 1)),
                ammo: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(canvas.element_count(), 10);
    }

    #[test]
    fn use_mode_expands_readouts_and_adds_the_ammo_cycle_button() {
        // Shooter with a multi-ammo weapon: crosshair + bio + 2 bars + 2
        // numbers + ammo backdrop + count = 8; NO cycle button (the caller
        // gates it on use mode, see `ammo_cycle_button_visible`).
        let shooter = build_flat_hud_canvas(
            true,
            false,
            1.0,
            0.75,
            None,
            &readout(Some(12), None, None, false),
        );
        assert_eq!(shooter.element_count(), 8);
        // Use mode (crosshair off) with an empty multi-ammo weapon: the same
        // readouts (expanded backdrops swap in place, same element count) PLUS
        // the AMMOFULL cycle button = 8 - 1 (crosshair) + 1 (cycle) = 8.
        let use_mode = build_flat_hud_canvas(
            false,
            true,
            1.0,
            0.75,
            None,
            &readout(Some(0), None, None, true),
        );
        assert_eq!(use_mode.element_count(), 8);
        // The cycle button only appears when the weapon can actually cycle.
        let single_ammo = build_flat_hud_canvas(
            false,
            true,
            1.0,
            0.75,
            None,
            &readout(Some(0), None, None, false),
        );
        assert_eq!(single_ammo.element_count(), 7);
    }

    #[test]
    fn ammo_cycle_button_sits_in_the_ammofull_panel() {
        // The cycle button (the original's cycle hotspot) is inside the expanded
        // AMMOFULL gauge and left of the compact AMMOBACK footprint.
        assert!(AMMO_FULL_GAUGE.x <= AMMO_CYCLE_BUTTON.x);
        assert!(AMMO_CYCLE_BUTTON.x + AMMO_CYCLE_BUTTON.w <= AMMO_FULL_GAUGE.x + AMMO_FULL_GAUGE.w);
    }

    /// The shared panel layout must keep landing where the flat HUD authored
    /// it: the ammo readout moved into `ammo_panel` in panel-local pixels, and
    /// these are the absolute canvas rects it replaced.
    #[test]
    fn shared_panel_layout_reproduces_the_authored_flat_rects() {
        let placed = |rect| ammo_panel::at(AMMO_PANEL_ORIGIN, rect);
        assert_eq!(
            placed(ammo_panel::CYCLE_BUTTON),
            Rect::new(564.0, 429.0, 12.0, 41.0)
        );
        assert_eq!(
            placed(ammo_panel::COUNT),
            Rect::new(544.0, 436.0, 94.0, 20.0)
        );
        assert_eq!(
            placed(ammo_panel::ICON),
            Rect::new(504.0, 430.0, 32.0, 32.0)
        );
        assert_eq!(
            placed(ammo_panel::TYPE_LABEL),
            Rect::new(544.0, 458.0, 94.0, 16.0)
        );
        assert_eq!(
            placed(ammo_panel::PSI_TIER_BADGE),
            Rect::new(500.0, 436.0, 32.0, 19.0)
        );
        assert_eq!(
            placed(ammo_panel::PSI_POWER_NAME),
            Rect::new(484.0, 458.0, 154.0, 16.0)
        );
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
    fn use_mode_hides_the_crosshair() {
        // The original turns the crosshair overlay off while the cursor is
        // up (ShockOverlayMouseMode) - one fewer element than shooter mode.
        let empty = readout(None, None, None, false);
        let shooter = build_flat_hud_canvas(true, false, 1.0, 0.75, None, &empty);
        let use_mode = build_flat_hud_canvas(false, false, 1.0, 0.75, None, &empty);
        assert_eq!(use_mode.element_count(), shooter.element_count() - 1);
    }

    #[test]
    fn out_of_range_fractions_do_not_panic() {
        // Fills are clamped inside `UiCanvas::bar`.
        let _ = build_flat_hud_canvas(
            true,
            false,
            2.0,
            -1.0,
            None,
            &readout(None, None, None, false),
        );
    }
}
