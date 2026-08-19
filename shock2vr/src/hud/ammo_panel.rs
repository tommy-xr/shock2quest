//! The ammo readout's layout - decided ONCE, in AMMOFULL panel pixels.
//!
//! The original SS2 ammo gauge is authored inside the 260x64 AMMOFULL.PCX
//! panel. Flatscreen draws that panel into the 640x480 HUD canvas at
//! [`crate::hud::flat_hud`]'s ammo origin; VR wears the same panel on the
//! right forearm, where it *is* the whole canvas. Both presentations emit the
//! readout from [`emit`] below, so the round count, ammo icon and type label
//! sit in the same place relative to the panel art in the headset as they do
//! on screen (AGENTS.md section 3).
//!
//! Rects are panel-local: (0,0) is the panel's upper-left corner.

use cgmath::{Vector2, vec2};
use shipyard::World;

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// The AMMOFULL panel's authored pixel size.
pub(crate) const PANEL_W: f32 = 260.0;
pub(crate) const PANEL_H: f32 = 64.0;
pub(crate) const PANEL_SIZE: Vector2<f32> = vec2(PANEL_W, PANEL_H);
/// The whole panel, for the backdrop art.
pub(crate) const PANEL: Rect = Rect::new(0.0, 0.0, PANEL_W, PANEL_H);

/// Round count, centered over the gauge (the AMMOBACK footprint at the panel's
/// right end).
pub(crate) const COUNT: Rect = Rect::new(166.0, 22.0, 94.0, 20.0);
/// Selected ammo type's object icon (P$ObjIcon), just left of the gauge.
pub(crate) const ICON: Rect = Rect::new(126.0, 16.0, 32.0, 32.0);
/// Ammo-type label (std/he/ap), below the count.
pub(crate) const TYPE_LABEL: Rect = Rect::new(166.0, 44.0, 94.0, 16.0);
/// The ammo-type cycle button (the original's `ammoarw` hotspot).
pub(crate) const CYCLE_BUTTON: Rect = Rect::new(186.0, 15.0, 12.0, 41.0);
/// Psi tier badge, in place of the ammo icon while the amp is wielded.
pub(crate) const PSI_TIER_BADGE: Rect = Rect::new(122.0, 22.0, 32.0, 19.0);
/// Psi discipline name. Longer than the ammo-type label, so it starts further
/// left.
pub(crate) const PSI_POWER_NAME: Rect = Rect::new(106.0, 44.0, 154.0, 16.0);

/// Place a panel-local rect into a canvas whose panel origin is `origin`.
pub(crate) const fn at(origin: Vector2<f32>, rect: Rect) -> Rect {
    Rect::new(origin.x + rect.x, origin.y + rect.y, rect.w, rect.h)
}

/// What the ammo readout says this frame. Presentation-agnostic: both the flat
/// HUD and the VR forearm panel build one of these from the world.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AmmoReadout {
    /// Selected psi power `(discipline, tier)` while the psi amp is wielded.
    /// The amp has no clip, so this *replaces* the ammo readout.
    pub psi_power: Option<(String, i32)>,
    /// Rounds in the wielded weapon's clip.
    pub ammo: Option<i32>,
    /// The selected ammo type's object-icon bitmap.
    pub ammo_icon: Option<String>,
    /// The selected ammo type's class tag ("std", "he", "ap").
    pub ammo_type: Option<String>,
    /// Whether to draw the ammo-type cycle arrow. Flat sets this only in use
    /// mode, where the pointer can actually click it; VR leaves it off (no
    /// forearm pointer affordance exists - that is an interaction-design
    /// decision, not a layout one).
    pub show_cycle_button: bool,
}

impl AmmoReadout {
    /// Read the wielded weapon's readout from the world. `show_cycle_button`
    /// is the caller's (presentation's) call.
    pub(crate) fn from_world(world: &World, show_cycle_button: bool) -> Self {
        Self {
            psi_power: super::get_wielded_psi_power(world),
            ammo: super::get_wielded_ammo(world),
            ammo_icon: super::get_wielded_ammo_icon(world),
            ammo_type: super::get_wielded_ammo_type(world),
            show_cycle_button,
        }
    }

    /// Nothing to say - no weapon with a clip and no psi power wielded. The
    /// gauge backdrop is not drawn at all in flat when this is true.
    pub(crate) fn is_empty(&self) -> bool {
        self.psi_power.is_none() && self.ammo.is_none()
    }
}

/// Emit the readout's elements into `canvas`, with the AMMOFULL panel's
/// upper-left corner at `origin` in that canvas's pixel space. The backdrop
/// art is the caller's (flat swaps AMMOBACK/AMMOFULL with use mode); every
/// *placement* lives here.
pub(crate) fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, readout: &AmmoReadout) {
    // The psi amp shows its selected discipline where a gun shows its clip.
    if let Some((power_name, tier)) = &readout.psi_power {
        let tier = *tier;
        canvas
            .image(
                at(origin, PSI_TIER_BADGE),
                &format!("AmPsi{}1.PCX", tier.clamp(1, 5)),
            )
            .text_native(
                at(origin, COUNT),
                &format!("{tier}"),
                "mainfont.fon",
                HAlign::Center,
                VAlign::Middle,
            )
            .text_native(
                at(origin, PSI_POWER_NAME),
                &power_name.to_ascii_uppercase(),
                "mainfont.fon",
                HAlign::Center,
                VAlign::Middle,
            );
        return;
    }

    let Some(rounds) = readout.ammo else {
        return;
    };
    canvas.text_native(
        at(origin, COUNT),
        &format!("{rounds}"),
        "mainfont.fon",
        HAlign::Center,
        VAlign::Middle,
    );
    if let Some(icon) = &readout.ammo_icon {
        canvas.image(at(origin, ICON), icon);
    }
    if let Some(ammo_type) = &readout.ammo_type {
        canvas.text_native(
            at(origin, TYPE_LABEL),
            &ammo_type.to_ascii_uppercase(),
            "mainfont.fon",
            HAlign::Center,
            VAlign::Middle,
        );
    }
    if readout.show_cycle_button {
        canvas.image(at(origin, CYCLE_BUTTON), "ammoarw0.pcx");
    }
}

/// The VR forearm panel's canvas: the AMMOFULL art with the readout composited
/// on it. The panel *is* the canvas here, so the readout is emitted at panel
/// origin (0,0) - flat emits the same elements at its own panel origin. Pure
/// (no asset/GL access), so it is unit-testable like `build_flat_hud_canvas`.
pub(crate) fn build_forearm_panel_canvas(readout: &AmmoReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(PANEL_SIZE);
    canvas.image(PANEL, "AMMOFULL.PCX");
    emit(&mut canvas, vec2(0.0, 0.0), readout);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gun(ammo: i32, icon: Option<&str>, ammo_type: Option<&str>) -> AmmoReadout {
        AmmoReadout {
            ammo: Some(ammo),
            ammo_icon: icon.map(str::to_string),
            ammo_type: ammo_type.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn the_empty_handed_forearm_panel_is_just_the_backdrop() {
        let canvas = build_forearm_panel_canvas(&AmmoReadout::default());
        assert_eq!(canvas.element_count(), 1);
        assert_eq!(canvas.size(), PANEL_SIZE);
    }

    #[test]
    fn a_wielded_gun_adds_its_count_icon_and_type_to_the_forearm() {
        // Backdrop + count = 2; + icon + type label = 4.
        assert_eq!(
            build_forearm_panel_canvas(&gun(12, None, None)).element_count(),
            2
        );
        assert_eq!(
            build_forearm_panel_canvas(&gun(12, Some("STD_I.PCX"), Some("std"))).element_count(),
            4
        );
    }

    #[test]
    fn the_forearm_count_tracks_the_clip() {
        // What the panel actually says, so a consumed round shows up here and
        // not just in a screenshot.
        let text_of = |readout| {
            build_forearm_panel_canvas(&readout)
                .elements()
                .iter()
                .find_map(|element| match element {
                    crate::ui::UiElement::Text { text, position, .. }
                        if *position == vec2(COUNT.x, COUNT.y) =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
        };
        assert_eq!(text_of(gun(12, None, None)).as_deref(), Some("12"));
        assert_eq!(text_of(gun(11, None, None)).as_deref(), Some("11"));
        assert_eq!(text_of(AmmoReadout::default()), None);
    }

    #[test]
    fn the_psi_amp_shows_its_discipline_on_the_forearm_too() {
        // Backdrop + tier badge + tier count + discipline name = 4, and the
        // amp's meaningless clip is suppressed exactly as in flat.
        let canvas = build_forearm_panel_canvas(&AmmoReadout {
            psi_power: Some(("Projected Cryokinesis".to_string(), 1)),
            ammo: Some(0),
            ..Default::default()
        });
        assert_eq!(canvas.element_count(), 4);
    }

    /// Every readout element is authored inside the panel, so the VR forearm
    /// - where the panel IS the whole canvas - shows all of them.
    #[test]
    fn every_element_sits_inside_the_panel() {
        for rect in [
            COUNT,
            ICON,
            TYPE_LABEL,
            CYCLE_BUTTON,
            PSI_TIER_BADGE,
            PSI_POWER_NAME,
        ] {
            assert!(
                rect.x >= 0.0 && rect.y >= 0.0,
                "{rect:?} starts outside the panel"
            );
            assert!(
                rect.x + rect.w <= PANEL_W,
                "{rect:?} overflows the panel width"
            );
            assert!(
                rect.y + rect.h <= PANEL_H,
                "{rect:?} overflows the panel height"
            );
        }
    }

    #[test]
    fn placing_the_panel_offsets_position_but_not_size() {
        let placed = at(vec2(378.0, 414.0), COUNT);
        assert_eq!(
            placed,
            Rect::new(378.0 + COUNT.x, 414.0 + COUNT.y, COUNT.w, COUNT.h)
        );
    }
}
