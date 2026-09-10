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
use dark::properties::ObjectState;
use shipyard::{EntityId, World};

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// The AMMOFULL panel's authored pixel size.
pub(crate) const PANEL_W: f32 = 260.0;
pub(crate) const PANEL_H: f32 = 64.0;
#[cfg(test)]
const PANEL_SIZE: Vector2<f32> = vec2(PANEL_W, PANEL_H);

/// Selected ammo type's object icon (P$ObjIcon), at the gauge well's left -
/// right of the cycle arrow, which claims the well's leading 12 px.
pub(crate) const ICON: Rect = Rect::new(200.0, 18.0, 24.0, 24.0);
/// Round count, right of the ammo icon inside the gauge well - the middle of
/// the well's three stacked bands (badge, count, ammo type), so the condition
/// badge above it has the corner the original draws it in to itself.
pub(crate) const COUNT: Rect = Rect::new(224.0, 28.0, 25.0, 14.0);
/// The wielded gun's condition badge (`WSTATE*.PCX`, 14x14), in the gauge
/// well's upper-right corner - where the original draws it.
pub(crate) const CONDITION: Rect = Rect::new(235.0, 14.0, 14.0, 14.0);
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
/// The readout itself - badge and discipline name - opens the selection MFD,
/// where a power is *chosen* from a described grid rather than stepped past.
/// It is the gauge well minus the power arrows that flank it (x 177..237), so
/// pressing an arrow still steps rather than opening the panel; it draws
/// nothing of its own, over art the backdrop already paints.
pub(crate) const PSI_SELECT: Rect = Rect::new(
    PSI_TIER_BADGE.x,
    PSI_TIER_PREV.y,
    PSI_POWER_NEXT.x - PSI_TIER_BADGE.x,
    PSI_POWER_NEXT.h,
);

/// The font every readout label uses (the original's HUD font).
const FONT: &str = "mainfont.fon";

/// The condition badges, best first: a green "10" down through a red "1", then
/// a crossed-out badge for a gun that is not in working order. The shipped art
/// authors all eleven; the tenth is what a gun on its last legs shows and the
/// eleventh is what a broken one shows.
const CONDITION_ART: [&str; 11] = [
    "wstate1.pcx",
    "wstate2.pcx",
    "wstate3.pcx",
    "wstate4.pcx",
    "wstate5.pcx",
    "wstate6.pcx",
    "wstate7.pcx",
    "wstate8.pcx",
    "wstate9.pcx",
    "wstate10.pcx",
    "wstate11.pcx",
];

/// Which badge a gun in `state` at `condition` (0..100) shows.
///
/// A gun that still works shows the badge for its condition tenth - the same
/// tenth its name's condition word comes from, counted the other way round,
/// since the art runs best-first and the words run worst-first. A gun that is
/// broken or destroyed shows the crossed-out badge instead, so "worn out but
/// firing" and "will not fire" are never the same picture. Only those two
/// states change the badge: an unresearched weapon still fires here, so it
/// still reads as its condition.
pub(crate) fn condition_badge(state: ObjectState, condition: f32) -> &'static str {
    if matches!(state, ObjectState::Broken | ObjectState::Destroyed) {
        return CONDITION_ART[10];
    }
    CONDITION_ART[(10 - super::gun_condition_bucket(condition)) as usize]
}

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
    /// Open the psi power selection MFD.
    PsiSelect,
    /// Open the pause / system menu. Not part of the ammo gauge - it belongs
    /// to the interface canvas as a whole (see [`super::readouts`]) - but it
    /// is a control on that canvas, so it travels with the rest through the
    /// one list the host draws and hit-tests.
    SystemMenu,
    Logs,
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
            ReadoutButton::PsiSelect => "psi_select",
            ReadoutButton::SystemMenu => "system_menu",
            ReadoutButton::Logs => "logs",
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

/// Draw one control at `rect`: its art, then its label centred on it.
///
/// The single drawing routine for every [`ReadoutButtonSpec`] the interface
/// canvas carries - the gauge's own controls here and the system button in
/// [`super::readouts`] - so a control cannot be drawn one way in one place and
/// hit-tested from another.
pub(crate) fn draw_button(canvas: &mut UiCanvas, button: &ReadoutButtonSpec, rect: Rect) {
    if let Some(texture) = button.texture {
        canvas.image(rect, texture);
    }
    if let Some(text) = &button.text {
        // Ellipsized: labels are data (a localized MISC.STR string, an authored
        // `P$SHead` header), and an over-wide one would spill onto the gauge.
        canvas.text_native_fit(rect, text, FONT, HAlign::Center, VAlign::Middle);
    }
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
    /// The wielded gun's condition badge art. Absent for a weapon that has no
    /// condition to wear down (the psi amp, a melee weapon).
    pub gun_condition: Option<&'static str>,
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

/// `weapon`'s condition badge, if it is the kind of weapon that has one.
fn wielded_gun_condition(world: &World, weapon: EntityId) -> Option<&'static str> {
    let condition = crate::scripts::script_util::gun_condition(world, weapon)?;
    Some(condition_badge(
        crate::scripts::gui::object_state(world, weapon),
        condition,
    ))
}

impl AmmoReadout {
    /// Read the wielded weapon's readout from the world. `show_buttons` is the
    /// caller's (presentation's) call.
    pub(crate) fn from_world(world: &World, show_buttons: bool) -> Self {
        Self::for_weapon(
            world,
            crate::wielded_weapon::wielded_weapon(world),
            show_buttons,
        )
    }

    /// Resolve every field from the same weapon, including a gun beside a psi amp.
    pub(crate) fn for_weapon(
        world: &World,
        weapon: Option<shipyard::EntityId>,
        show_buttons: bool,
    ) -> Self {
        Self {
            gun_condition: weapon.and_then(|w| wielded_gun_condition(world, w)),
            psi_power: super::get_weapon_psi_power(world, weapon),
            ammo: super::get_weapon_ammo(world, weapon),
            ammo_icon: super::get_weapon_ammo_icon(world, weapon),
            ammo_type: super::get_weapon_ammo_type(world, weapon),
            gun_setting_header: super::get_weapon_gun_setting(world, weapon)
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
            // Last: the host hit-tests in order and takes the first match, so
            // the four arrows keep their pixels where they meet the readout.
            spec(ReadoutButton::PsiSelect, PSI_SELECT, None, None),
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
        if let Some(badge) = readout.gun_condition {
            canvas.image(at(origin, CONDITION), badge);
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
        draw_button(canvas, &button, at(origin, button.rect));
    }
}

/// The readout as the VR forearm draws it: a panel-sized canvas holding just
/// the readout, which the forearm lays over its AMMOFULL backdrop quad. The
/// panel *is* the canvas here, so the elements are emitted at panel origin
/// (0,0) - flat emits the same ones at its own panel origin. Pure (no
/// asset/GL access), so it is unit-testable like `build_flat_hud_canvas`.
#[cfg(test)]
fn build_readout_canvas(readout: &AmmoReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(PANEL_SIZE);
    emit(&mut canvas, vec2(0.0, 0.0), readout);
    canvas
}

/// The complete compact ammo UI, the rightmost 94x64 pixels of AMMOFULL.
/// Preserve AMMOBACK's authored frame rather than cutting through its bezel.
pub(crate) const WRIST_CROP: Rect = Rect::new(166.0, 0.0, 94.0, 64.0);
pub(crate) const WRIST_CROP_SIZE: Vector2<f32> = vec2(WRIST_CROP.w, WRIST_CROP.h);

/// The shipped compact ammo panel with the same overlay origin as the flat HUD.
/// Empty hands have no panel; the wrist has no clickable controls.
pub(crate) fn build_wrist_canvas(readout: &AmmoReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(WRIST_CROP_SIZE);
    if readout.is_empty() {
        return canvas;
    }
    canvas.image(
        Rect::new(0.0, 0.0, WRIST_CROP.w, WRIST_CROP.h),
        "AMMOBACK.PCX",
    );
    let passive = AmmoReadout {
        show_buttons: false,
        ..readout.clone()
    };
    emit(&mut canvas, vec2(-WRIST_CROP.x, -WRIST_CROP.y), &passive);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrist_preserves_the_complete_compact_panel_and_shared_overlay_layout() {
        let readout = full_gun();
        let wrist = build_wrist_canvas(&readout);
        assert_eq!(wrist.size(), vec2(94.0, 64.0));
        assert!(
            matches!(&wrist.elements()[0], crate::ui::UiElement::Image { texture, kind: crate::ui::ImageKind::Ui, .. } if texture == "AMMOBACK.PCX")
        );
        let passive = AmmoReadout {
            show_buttons: false,
            ..readout
        };
        let panel = build_readout_canvas(&passive);
        assert_eq!(wrist.element_count(), panel.element_count() + 1);
        for (cropped, full) in wrist.elements()[1..].iter().zip(panel.elements()) {
            let (a, b) = (cropped.rect(), full.rect());
            assert_eq!(
                (a.x + WRIST_CROP.x, a.y + WRIST_CROP.y, a.w, a.h),
                (b.x, b.y, b.w, b.h)
            );
        }
        assert_eq!(
            build_wrist_canvas(&AmmoReadout::default()).element_count(),
            0
        );
    }

    #[test]
    fn each_wrist_reads_its_own_clip_and_empty_support_hand_has_no_counter() {
        use crate::{mission::PlayerInfo, vr_config::Handedness, wielded_weapon::weapon_in_hand};
        use dark::properties::PropGunState;
        let mut world = World::new();
        let player = world.add_entity(());
        let left = world.add_entity((PropGunState {
            ammo: 6,
            condition: 100.0,
            setting: 0,
            modification: 0,
            silence_value: 0.0,
        },));
        let right = world.add_entity((PropGunState {
            ammo: 12,
            condition: 100.0,
            setting: 1,
            modification: 0,
            silence_value: 0.0,
        },));
        let mut info = PlayerInfo {
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: Some(left),
            right_hand_entity_id: Some(right),
        };
        world.add_unique(info.clone());
        let read = |world: &World, hand| {
            AmmoReadout::for_weapon(world, weapon_in_hand(world, hand), false)
        };
        assert_eq!(read(&world, Handedness::Left).ammo, Some(6));
        assert_eq!(read(&world, Handedness::Right).ammo, Some(12));
        info.left_hand_entity_id = Some(right);
        info.right_hand_entity_id = None;
        *world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap() = info;
        assert_eq!(read(&world, Handedness::Left).ammo, Some(12));
        assert!(read(&world, Handedness::Right).is_empty());
    }

    #[test]
    fn psi_amp_beside_a_gun_does_not_steal_its_ammo_readout() {
        use crate::{
            mission::mission_core::GlobalTemplateClassTags,
            psi::{GlobalPsiPowers, PsiPowerInfo, PsiPowerSelection},
        };
        use dark::properties::{PropGunState, PropPsiPower, PropTemplateId};
        use std::collections::HashMap;
        let mut world = World::new();
        let gun = world.add_entity((PropGunState {
            ammo: 12,
            condition: 100.0,
            setting: 0,
            modification: 0,
            silence_value: 0.0,
        },));
        let amp = world.add_entity((PropTemplateId { template_id: -247 },));
        world.add_unique(GlobalTemplateClassTags(HashMap::from([(
            -247,
            HashMap::from([("weapontype".to_owned(), "psiamp".to_owned())]),
        )])));
        world.add_unique(GlobalPsiPowers(vec![PsiPowerInfo {
            template_id: -100,
            name: "Cryokinesis".to_owned(),
            display_name: Some("Projected Cryokinesis".to_owned()),
            power: PropPsiPower {
                power_id: 1,
                activation_type: 0,
                psi_cost: 99,
                data: [0.0; 4],
            },
            projectiles: vec![],
            overloadable: false,
            duration: None,
        }]));
        world.add_unique(PsiPowerSelection { index: 0 });
        let gun_readout = AmmoReadout::for_weapon(&world, Some(gun), false);
        let amp_readout = AmmoReadout::for_weapon(&world, Some(amp), false);
        assert_eq!(gun_readout.ammo, Some(12));
        assert_eq!(gun_readout.psi_power, None);
        assert_eq!(amp_readout.ammo, None);
        assert_eq!(
            amp_readout.psi_power,
            Some(("Projected Cryokinesis".to_owned(), 1))
        );
    }

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

    /// The badge grades a working gun and calls out one that will not fire.
    #[test]
    fn the_condition_badge_counts_down_with_the_gun() {
        let working = |condition| condition_badge(ObjectState::Normal, condition);
        assert_eq!(working(100.0), "wstate1.pcx", "pristine");
        assert_eq!(working(91.0), "wstate1.pcx", "barely used");
        assert_eq!(working(45.0), "wstate6.pcx", "half worn");
        assert_eq!(working(5.0), "wstate10.pcx", "about to give out");
        assert_eq!(working(0.0), "wstate10.pcx", "worn out");
        // A gun that will not fire reads differently from one that barely
        // will - that is the whole point of the eleventh badge.
        assert_eq!(condition_badge(ObjectState::Broken, 0.0), "wstate11.pcx");
        assert_eq!(
            condition_badge(ObjectState::Destroyed, 90.0),
            "wstate11.pcx"
        );
        // ...but a state that does not stop the gun leaves the grade alone.
        assert_eq!(
            condition_badge(ObjectState::Unresearched, 100.0),
            working(100.0)
        );
    }

    /// The badge is emitted with the gun's readout, in its own corner: both
    /// presentations build this canvas, so neither can place it differently.
    #[test]
    fn the_condition_badge_takes_the_wells_corner_clear_of_the_count() {
        let canvas = build_readout_canvas(&AmmoReadout {
            gun_condition: Some("wstate11.pcx"),
            ..gun(12, None, None)
        });
        let badge = canvas
            .elements()
            .iter()
            .find_map(|element| match element {
                crate::ui::UiElement::Image {
                    texture, position, ..
                } if texture == "wstate11.pcx" => Some(*position),
                _ => None,
            })
            .expect("the readout draws the badge");
        assert_eq!(badge, vec2(CONDITION.x, CONDITION.y));
        // The well stacks three bands, and none of them may sit on another.
        assert!(
            CONDITION.y + CONDITION.h <= COUNT.y && COUNT.y + COUNT.h <= TYPE_LABEL.y,
            "badge/count/type must stack: {CONDITION:?} {COUNT:?} {TYPE_LABEL:?}"
        );
        assert!(
            CONDITION.x + CONDITION.w <= WELL.x + WELL.w,
            "the badge must stay inside the gauge well"
        );
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
            CONDITION,
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
        for rect in [
            COUNT,
            ICON,
            TYPE_LABEL,
            CONDITION,
            PSI_TIER_BADGE,
            PSI_POWER_NAME,
        ] {
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

    /// No control may be *drawn* on top of the readout it annotates - the bug
    /// this layout fixes is exactly that (the ammo icon sat inside the SETTING
    /// button's rect). `PsiSelect` is the exception that proves it: it draws
    /// nothing at all and its whole point is to make the badge and discipline
    /// name themselves clickable, so it is asserted separately below.
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
                if spec.button == ReadoutButton::PsiSelect {
                    continue;
                }
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

    /// The badge-and-name area opens the selection MFD, and it draws nothing
    /// of its own: it covers exactly the readout the backdrop already paints,
    /// and stops short of the power arrows that flank it, so a click on an
    /// arrow still steps.
    #[test]
    fn the_psi_readout_itself_opens_the_selection_panel() {
        let amp = AmmoReadout {
            psi_power: Some(("Projected Cryokinesis".to_string(), 1)),
            show_buttons: true,
            ..Default::default()
        };
        let spec = buttons(&amp)
            .into_iter()
            .find(|spec| spec.button == ReadoutButton::PsiSelect)
            .expect("the psi readout is clickable");
        assert!(spec.texture.is_none() && spec.text.is_none(), "{spec:?}");
        for covered in [PSI_TIER_BADGE, PSI_POWER_NAME] {
            assert!(
                spec.rect.x <= covered.x
                    && spec.rect.y <= covered.y
                    && spec.rect.x + spec.rect.w >= covered.x + covered.w
                    && spec.rect.y + spec.rect.h >= covered.y + covered.h,
                "{covered:?} must be inside {:?}",
                spec.rect
            );
        }
        for arrow in [PSI_TIER_PREV, PSI_TIER_NEXT, PSI_POWER_PREV, PSI_POWER_NEXT] {
            let overlaps = spec.rect.x < arrow.x + arrow.w
                && arrow.x < spec.rect.x + spec.rect.w
                && spec.rect.y < arrow.y + arrow.h
                && arrow.y < spec.rect.y + spec.rect.h;
            assert!(!overlaps, "{arrow:?} must stay outside {:?}", spec.rect);
        }
        // ...and it is hit-tested last, so where the rects touch the arrows win.
        assert_eq!(
            buttons(&amp).last().map(|s| s.button),
            Some(ReadoutButton::PsiSelect)
        );
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
                ReadoutButton::PsiPowerNext,
                ReadoutButton::PsiSelect
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
