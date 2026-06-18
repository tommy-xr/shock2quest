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

use super::{get_health_percentage, get_psi_percentage};
use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

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

const BAR_W: f32 = 120.0;
const BAR_H: f32 = 12.0;
const HEALTH_BAR: Rect = Rect::new(20.0, 448.0, BAR_W, BAR_H); // near the bottom edge
const PSI_BAR: Rect = Rect::new(20.0, 430.0, BAR_W, BAR_H); // stacked just above health

const HEALTH_TEXT: Rect = Rect::new(148.0, 444.0, 80.0, 16.0); // right of the bars
const HEALTH_TEXT_SIZE: f32 = 16.0;

/// Build the flat HUD as a resolution-independent canvas for the given player
/// stat fractions. Pure (no asset/GL access), so it is unit-testable.
pub(crate) fn build_flat_hud_canvas(health_fraction: f32, psi_fraction: f32) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(VIRTUAL_W, VIRTUAL_H));

    canvas
        .image(CROSSHAIR, "CROSSHAI.PCX")
        .bar(HEALTH_BAR, "HPBAR.PCX", health_fraction)
        .bar(PSI_BAR, "PSIBAR.PCX", psi_fraction);

    let pct = (health_fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
    canvas.text(
        HEALTH_TEXT,
        &format!("{pct}"),
        "mainfont.fon",
        HEALTH_TEXT_SIZE,
        HAlign::Left,
        VAlign::Middle,
    );

    canvas
}

/// Build and render the flat HUD as screen-space scene objects.
pub(crate) fn create_flat_hud(
    asset_cache: &mut AssetCache,
    world: &World,
    screen_size: cgmath::Vector2<f32>,
) -> Vec<SceneObject> {
    let canvas = build_flat_hud_canvas(get_health_percentage(world), get_psi_percentage(world));
    canvas.render_screen_space(asset_cache, screen_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_has_crosshair_two_bars_and_readout() {
        let canvas = build_flat_hud_canvas(1.0, 0.75);
        assert_eq!(canvas.element_count(), 4);
    }

    #[test]
    fn crosshair_is_centered() {
        assert_eq!(CROSSHAIR.center(), vec2(VIRTUAL_W / 2.0, VIRTUAL_H / 2.0));
    }

    #[test]
    fn out_of_range_fractions_do_not_panic() {
        // Fills are clamped inside `UiCanvas::bar`.
        let _ = build_flat_hud_canvas(2.0, -1.0);
    }
}
