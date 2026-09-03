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
//! Rects are panel-local: (0,0) is the panel's upper-left corner. Two regions
//! matter, both measured off the shipped art: the **gauge well** - the lit
//! recess at panel x 176..249, y 13..55, which is exactly what the compact
//! AMMOBACK.PCX (94x64) crop shows - holds the readout itself, and the
//! **use-mode button column** left of it (from x 117, present only on the
//! expanded AMMOFULL art, whose recess runs 117..249 unbroken) holds the
//! controls the cursor clicks. Everything drawn therefore stays inside the
//! gauge well, so the compact panel is not missing half its readout and the
//! buttons do not land on top of it.

use cgmath::{Vector2, vec2};
use shipyard::World;

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// The AMMOFULL panel's authored pixel size.
pub(crate) const PANEL_W: f32 = 260.0;
pub(crate) const PANEL_H: f32 = 64.0;
pub(crate) const PANEL_SIZE: Vector2<f32> = vec2(PANEL_W, PANEL_H);

/// Selected ammo type's object icon (P$ObjIcon), at the gauge well's left -
/// right of the cycle arrow, which claims the well's leading 12 px.
pub(crate) const ICON: Rect = Rect::new(200.0, 18.0, 24.0, 24.0);
/// Round count, right of the ammo icon inside the gauge well.
pub(crate) const COUNT: Rect = Rect::new(224.0, 20.0, 25.0, 20.0);
/// Ammo-type label (std/he/ap), across the bottom of the gauge well.
pub(crate) const TYPE_LABEL: Rect = Rect::new(198.0, 42.0, 51.0, 13.0);

/// The ammo-type cycle button (the original's `ammoarw` hotspot, 12x41 art).
pub(crate) const CYCLE_BUTTON: Rect = Rect::new(186.0, 15.0, 12.0, 41.0);
/// The fire-mode SETTING button. Its label is the gun's current mode header
/// (`P$SHead1`/`P$SHead2` -> "NORM"/"BURST"/"AUTO"); a click cycles the mode.
pub(crate) const SETTING_BUTTON: Rect = Rect::new(118.0, 15.0, 66.0, 20.0);
/// The RELOAD button - the same thing the Reload key does.
pub(crate) const RELOAD_BUTTON: Rect = Rect::new(118.0, 37.0, 66.0, 20.0);

/// Psi tier badge (AmPsi&lt;tier&gt;1.PCX, 32x19), in the gauge well while the
/// amp is wielded - the amp has no clip, so this replaces the ammo readout.
pub(crate) const PSI_TIER_BADGE: Rect = Rect::new(177.0, 16.0, 32.0, 19.0);
/// Psi discipline name, under the badge and between the power arrows.
pub(crate) const PSI_POWER_NAME: Rect = Rect::new(177.0, 37.0, 60.0, 18.0);
/// Tier step buttons (`left`/`right` art, 18x18), in the button column.
pub(crate) const PSI_TIER_PREV: Rect = Rect::new(118.0, 15.0, 18.0, 18.0);
pub(crate) const PSI_TIER_NEXT: Rect = Rect::new(136.0, 15.0, 18.0, 18.0);
/// Power step buttons (`pleft`/`pright` art, 12x41), flanking the power name.
pub(crate) const PSI_POWER_PREV: Rect = Rect::new(157.0, 15.0, 12.0, 41.0);
pub(crate) const PSI_POWER_NEXT: Rect = Rect::new(237.0, 15.0, 12.0, 41.0);

/// The font every readout label uses (the original's HUD font).
const FONT: &str = "mainfont.fon";

/// A clickable control on the ammo readout. The layout owns the set, so the
/// drawn button and the hit-tested rect can never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadoutButton {
    /// Cycle the wielded weapon's ammo type.
    CycleAmmo,
    /// Cycle the wielded gun's fire mode.
    GunSetting,
    /// Reload the wielded gun with its current ammo type.
    Reload,
    PsiTierPrev,
    PsiTierNext,
    PsiPowerPrev,
    PsiPowerNext,
}

impl ReadoutButton {
    /// Stable semantic id, so `/v1/ui` clients click by meaning.
    pub fn label(self) -> &'static str {
        match self {
            ReadoutButton::CycleAmmo => "cycle_ammo",
            ReadoutButton::GunSetting => "gun_setting",
            ReadoutButton::Reload => "reload",
            ReadoutButton::PsiTierPrev => "psi_tier_prev",
            ReadoutButton::PsiTierNext => "psi_tier_next",
            ReadoutButton::PsiPowerPrev => "psi_power_prev",
            ReadoutButton::PsiPowerNext => "psi_power_next",
        }
    }
}

/// One placed readout button: what it does, where it is, and what it shows.
/// `rect` is panel-local as produced by [`buttons`]; [`crate::hud::flat_hud`]
/// maps it into canvas space with [`at`] before handing it to the pointer host,
/// so the drawn and clickable rects are the same rect.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadoutButtonSpec {
    pub button: ReadoutButton,
    pub rect: Rect,
    pub texture: Option<&'static str>,
    pub text: Option<String>,
}

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
    /// The wielded gun's current fire-mode header ("NORM"/"BURST"/"AUTO") -
    /// the SETTING button's label. Absent when the gun names no header.
    pub gun_setting_header: Option<String>,
    /// The weapon offers a different projectile type to switch to.
    pub can_cycle_ammo: bool,
    /// The weapon takes clips (an energy weapon recharges instead).
    pub can_reload: bool,
    /// MISC.STR `Reload` - the reload button's label.
    pub reload_label: String,
    /// Whether the interactive controls are drawn and hit-tested. Flat sets
    /// this only in use mode, where the pointer can actually click them; VR
    /// leaves it off (the forearm panel has no pointer affordance - that is an
    /// interaction-design decision, not a layout one).
    pub show_buttons: bool,
}

impl AmmoReadout {
    /// Read the wielded weapon's readout from the world. `show_buttons` is the
    /// caller's (presentation's) call.
    pub(crate) fn from_world(world: &World, show_buttons: bool) -> Self {
        let weapon = crate::wielded_weapon::wielded_weapon(world);
        Self {
            psi_power: super::get_wielded_psi_power(world),
            ammo: super::get_wielded_ammo(world),
            ammo_icon: super::get_wielded_ammo_icon(world),
            ammo_type: super::get_wielded_ammo_type(world),
            gun_setting_header: super::get_wielded_gun_setting(world)
                .and_then(|(_, header)| header),
            can_cycle_ammo: weapon
                .is_some_and(|w| crate::scripts::script_util::can_cycle_ammo(world, w)),
            // An energy weapon has no magazine to swap - it recharges - so it
            // shows no reload control, exactly as the original.
            can_reload: weapon.is_some_and(|w| !crate::wielded_weapon::is_energy_weapon(world, w)),
            reload_label: super::hud_strings(world).reload_label,
            show_buttons,
        }
    }

    /// Nothing to say - no weapon with a clip and no psi power wielded. The
    /// gauge backdrop is not drawn at all in flat when this is true.
    pub(crate) fn is_empty(&self) -> bool {
        self.psi_power.is_none() && self.ammo.is_none()
    }
}

/// The readout's clickable controls this frame, in panel-local pixels.
///
/// The single source of truth for the button set: [`emit`] draws exactly these
/// and the pointer host hit-tests exactly these, so a drawn button is always
/// clickable and vice versa.
pub(crate) fn buttons(readout: &AmmoReadout) -> Vec<ReadoutButtonSpec> {
    let spec = |button, rect, texture, text: Option<&str>| ReadoutButtonSpec {
        button,
        rect,
        texture,
        text: text.map(str::to_owned),
    };
    if !readout.show_buttons {
        return Vec::new();
    }
    // The amp replaces the gun controls with its power selector: tier stepping
    // on the left, power stepping flanking the discipline name.
    if readout.psi_power.is_some() {
        return vec![
            spec(
                ReadoutButton::PsiTierPrev,
                PSI_TIER_PREV,
                Some("LEFT0.PCX"),
                None,
            ),
            spec(
                ReadoutButton::PsiTierNext,
                PSI_TIER_NEXT,
                Some("RIGHT0.PCX"),
                None,
            ),
            spec(
                ReadoutButton::PsiPowerPrev,
                PSI_POWER_PREV,
                Some("PLEFT0.PCX"),
                None,
            ),
            spec(
                ReadoutButton::PsiPowerNext,
                PSI_POWER_NEXT,
                Some("PRIGHT0.PCX"),
                None,
            ),
        ];
    }
    if readout.ammo.is_none() {
        // Nothing with a magazine wielded: no gun controls at all.
        return Vec::new();
    }
    let mut out = Vec::new();
    // The fire-mode button is shown for every gun with a magazine, energy
    // weapons included - only a gun whose data names no mode has none.
    if let Some(header) = &readout.gun_setting_header {
        out.push(spec(
            ReadoutButton::GunSetting,
            SETTING_BUTTON,
            None,
            Some(header),
        ));
    }
    if readout.can_reload {
        out.push(spec(
            ReadoutButton::Reload,
            RELOAD_BUTTON,
            None,
            Some(&readout.reload_label),
        ));
    }
    if readout.can_cycle_ammo {
        out.push(spec(
            ReadoutButton::CycleAmmo,
            CYCLE_BUTTON,
            Some("ammoarw0.pcx"),
            None,
        ));
    }
    out
}

/// Emit the readout's elements into `canvas`, with the AMMOFULL panel's
/// upper-left corner at `origin` in that canvas's pixel space. The backdrop
/// art is the caller's (flat swaps AMMOBACK/AMMOFULL with use mode); every
/// *placement* lives here.
pub(crate) fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, readout: &AmmoReadout) {
    // The psi amp shows its selected discipline where a gun shows its clip.
    if let Some((power_name, tier)) = &readout.psi_power {
        canvas
            .image(
                at(origin, PSI_TIER_BADGE),
                &format!("AmPsi{}1.PCX", (*tier).clamp(1, 5)),
            )
            .text_native_fit(
                at(origin, PSI_POWER_NAME),
                &power_name.to_ascii_uppercase(),
                FONT,
                HAlign::Center,
                VAlign::Middle,
            );
    } else if let Some(rounds) = readout.ammo {
        canvas.text_native(
            at(origin, COUNT),
            &format!("{rounds}"),
            FONT,
            HAlign::Center,
            VAlign::Middle,
        );
        if let Some(icon) = &readout.ammo_icon {
            canvas.image(at(origin, ICON), icon);
        }
        if let Some(ammo_type) = &readout.ammo_type {
            // Ellipsized: the class tag is data ("std", but also "lasershot"),
            // and an over-wide label would spill out of the gauge well.
            canvas.text_native_fit(
                at(origin, TYPE_LABEL),
                &ammo_type.to_ascii_uppercase(),
                FONT,
                HAlign::Center,
                VAlign::Middle,
            );
        }
    } else {
        return;
    }

    for button in buttons(readout) {
        let rect = at(origin, button.rect);
        if let Some(texture) = button.texture {
            canvas.image(rect, texture);
        }
        if let Some(text) = &button.text {
            // Ellipsized like the ammo-type label: both labels are data (a
            // localized MISC.STR string, an authored `P$SHead` header), and an
            // over-wide one would spill onto the gauge.
            canvas.text_native_fit(rect, text, FONT, HAlign::Center, VAlign::Middle);
        }
    }
}

/// The readout as the VR forearm draws it: a panel-sized canvas holding just
/// the readout, which the forearm lays over its AMMOFULL backdrop quad. The
/// panel *is* the canvas here, so the elements are emitted at panel origin
/// (0,0) - flat emits the same ones at its own panel origin. Pure (no
/// asset/GL access), so it is unit-testable like `build_flat_hud_canvas`.
pub(crate) fn build_readout_canvas(readout: &AmmoReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(PANEL_SIZE);
    emit(&mut canvas, vec2(0.0, 0.0), readout);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gauge well the compact AMMOBACK crop shows, read off the art.
    const WELL: Rect = Rect::new(176.0, 13.0, 73.0, 42.0);

    fn gun(ammo: i32, icon: Option<&str>, ammo_type: Option<&str>) -> AmmoReadout {
        AmmoReadout {
            ammo: Some(ammo),
            ammo_icon: icon.map(str::to_string),
            ammo_type: ammo_type.map(str::to_string),
            reload_label: "RELOAD".to_string(),
            ..Default::default()
        }
    }

    /// A gun with everything a gun can offer, in use mode.
    fn full_gun() -> AmmoReadout {
        AmmoReadout {
            gun_setting_header: Some("NORM".to_string()),
            can_cycle_ammo: true,
            can_reload: true,
            show_buttons: true,
            ..gun(12, Some("STD_I.PCX"), Some("std"))
        }
    }

    fn kinds(readout: &AmmoReadout) -> Vec<ReadoutButton> {
        buttons(readout).into_iter().map(|b| b.button).collect()
    }

    #[test]
    fn the_empty_handed_forearm_readout_is_empty() {
        // Nothing to say: the forearm shows its bare AMMOFULL backdrop quad.
        let canvas = build_readout_canvas(&AmmoReadout::default());
        assert_eq!(canvas.element_count(), 0);
        assert_eq!(canvas.size(), PANEL_SIZE);
    }

    #[test]
    fn a_wielded_gun_adds_its_count_icon_and_type_to_the_forearm() {
        assert_eq!(
            build_readout_canvas(&gun(12, None, None)).element_count(),
            1
        );
        assert_eq!(
            build_readout_canvas(&gun(12, Some("STD_I.PCX"), Some("std"))).element_count(),
            3
        );
    }

    #[test]
    fn the_forearm_count_tracks_the_clip() {
        // What the panel actually says, so a consumed round shows up here and
        // not just in a screenshot.
        let text_of = |readout| {
            build_readout_canvas(&readout)
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
        // Tier badge + discipline name = 2, and the amp's meaningless clip is
        // suppressed exactly as in flat.
        let canvas = build_readout_canvas(&AmmoReadout {
            psi_power: Some(("Projected Cryokinesis".to_string(), 1)),
            ammo: Some(0),
            ..Default::default()
        });
        assert_eq!(canvas.element_count(), 2);
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
            SETTING_BUTTON,
            RELOAD_BUTTON,
            PSI_TIER_BADGE,
            PSI_POWER_NAME,
            PSI_TIER_PREV,
            PSI_TIER_NEXT,
            PSI_POWER_PREV,
            PSI_POWER_NEXT,
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

    /// The readout proper lives in the gauge well - the part of the panel the
    /// COMPACT AMMOBACK art also shows. Before this layout the ammo icon sat
    /// 40 px to the left of the compact backdrop entirely, so shooter mode drew
    /// it on bare 3D view.
    #[test]
    fn the_readout_sits_inside_the_compact_gauge_well() {
        for rect in [COUNT, ICON, TYPE_LABEL, PSI_TIER_BADGE, PSI_POWER_NAME] {
            assert!(
                rect.x >= WELL.x && rect.x + rect.w <= WELL.x + WELL.w,
                "{rect:?} leaves the gauge well horizontally"
            );
            assert!(
                rect.y >= WELL.y && rect.y + rect.h <= WELL.y + WELL.h,
                "{rect:?} leaves the gauge well vertically"
            );
        }
    }

    /// No control may be drawn on top of the readout it annotates - the bug
    /// this layout fixes is exactly that (the ammo icon sat inside the SETTING
    /// button's rect).
    #[test]
    fn no_button_overlaps_the_readout() {
        let overlaps = |a: Rect, b: Rect| {
            a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
        };
        let psi = AmmoReadout {
            psi_power: Some(("Cryokinesis".to_string(), 1)),
            show_buttons: true,
            ..Default::default()
        };
        for (readout, drawn) in [
            (full_gun(), vec![COUNT, ICON, TYPE_LABEL]),
            (psi, vec![PSI_TIER_BADGE, PSI_POWER_NAME]),
        ] {
            for spec in buttons(&readout) {
                for rect in &drawn {
                    assert!(
                        !overlaps(spec.rect, *rect),
                        "{:?} at {:?} covers {rect:?}",
                        spec.button,
                        spec.rect
                    );
                }
            }
        }
    }

    #[test]
    fn buttons_only_appear_where_the_pointer_can_reach_them() {
        // Shooter mode / the VR forearm: the readout draws, the controls do not.
        assert!(
            kinds(&AmmoReadout {
                show_buttons: false,
                ..full_gun()
            })
            .is_empty()
        );
        assert_eq!(
            kinds(&full_gun()),
            vec![
                ReadoutButton::GunSetting,
                ReadoutButton::Reload,
                ReadoutButton::CycleAmmo
            ]
        );
    }

    #[test]
    fn an_energy_weapon_keeps_its_setting_button_but_loses_cycle_and_reload() {
        // The laser recharges and offers one projectile, but it still switches
        // between its normal and overcharged modes.
        let laser = AmmoReadout {
            can_cycle_ammo: false,
            can_reload: false,
            ..full_gun()
        };
        assert_eq!(kinds(&laser), vec![ReadoutButton::GunSetting]);
    }

    #[test]
    fn a_gun_with_no_named_mode_has_no_setting_button() {
        let plain = AmmoReadout {
            gun_setting_header: None,
            ..full_gun()
        };
        assert_eq!(
            kinds(&plain),
            vec![ReadoutButton::Reload, ReadoutButton::CycleAmmo]
        );
    }

    #[test]
    fn the_setting_button_is_labeled_with_the_current_mode() {
        let spec = buttons(&AmmoReadout {
            gun_setting_header: Some("BURST".to_string()),
            ..full_gun()
        })
        .into_iter()
        .find(|b| b.button == ReadoutButton::GunSetting)
        .expect("the setting button is shown");
        assert_eq!(spec.text.as_deref(), Some("BURST"));
        assert_eq!(spec.rect, SETTING_BUTTON);
    }

    #[test]
    fn the_psi_amp_shows_four_selector_arrows_and_no_gun_controls() {
        let amp = AmmoReadout {
            psi_power: Some(("Projected Cryokinesis".to_string(), 1)),
            // The amp carries a meaningless clip; it must not produce gun
            // controls.
            ammo: Some(0),
            gun_setting_header: Some("NORM".to_string()),
            can_reload: true,
            show_buttons: true,
            ..Default::default()
        };
        assert_eq!(
            kinds(&amp),
            vec![
                ReadoutButton::PsiTierPrev,
                ReadoutButton::PsiTierNext,
                ReadoutButton::PsiPowerPrev,
                ReadoutButton::PsiPowerNext
            ]
        );
    }

    #[test]
    fn nothing_wielded_has_no_buttons() {
        assert!(
            kinds(&AmmoReadout {
                show_buttons: true,
                gun_setting_header: Some("NORM".to_string()),
                can_reload: true,
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn drawn_buttons_match_the_hit_tested_set() {
        // Every button `buttons()` reports adds exactly one canvas element at
        // the same rect, so `mission_core` can hit-test that list verbatim.
        let readout = full_gun();
        let specs = buttons(&readout);
        let canvas = build_readout_canvas(&readout);
        for spec in &specs {
            assert!(
                canvas.elements().iter().any(|element| match element {
                    crate::ui::UiElement::Text { position, .. }
                    | crate::ui::UiElement::Image { position, .. } =>
                        *position == vec2(spec.rect.x, spec.rect.y),
                    _ => false,
                }),
                "{:?} is hit-tested but not drawn",
                spec.button
            );
        }
        // count + icon + type + 3 buttons
        assert_eq!(canvas.element_count(), 3 + specs.len());
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
