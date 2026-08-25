//! The on-amp psi meter's layout - decided ONCE, in panel pixels, the same
//! way [`super::ammo_panel`] lays out the on-weapon ammo tag. A psi pool bar
//! (PSIBAR.PCX, the same fill art the flat bio-monitor uses) plus the
//! hold-to-overload meter (LOADBACK/LOADMETR/LOADGOOD/LOADBURN) - the latter
//! was flat-only before this (`flat_hud.rs`'s `OVERLOAD_METER`, no VR
//! consumer); this closes that parity gap by drawing it here too, on the amp
//! itself. Both presentations emit this readout from [`emit`], so it lands in
//! the same place relative to the amp in the headset as the fraction would on
//! screen (AGENTS.md section 3).
//!
//! Rects are panel-local: (0,0) is the panel's upper-left corner.

use cgmath::{Vector2, vec2};
use shipyard::World;

use crate::runtime_props::{PsiChargePhase, RuntimePropPsiCharge};
use crate::ui::{Rect, UiCanvas};

/// The panel's authored pixel size: two stacked bars, each the same 80x14 the
/// flat bio-monitor's HPBAR/PSIBAR fills use.
pub(crate) const PANEL_W: f32 = 80.0;
pub(crate) const PANEL_H: f32 = 32.0;
pub(crate) const PANEL_SIZE: Vector2<f32> = vec2(PANEL_W, PANEL_H);

/// Psi pool level - always shown while the amp is wielded.
pub(crate) const PSI_BAR: Rect = Rect::new(0.0, 0.0, 80.0, 14.0);
/// Hold-to-overload progress - shown only while a charge is in progress or
/// its result is still flashing (`readout.charge.is_some()`).
pub(crate) const OVERLOAD_BAR: Rect = Rect::new(0.0, 18.0, 80.0, 14.0);

/// What the on-amp psi meter says this frame. Presentation-agnostic: both the
/// flat viewmodel and the VR amp-mounted panel build one of these from the
/// world.
#[derive(Debug, Clone, Default)]
pub(crate) struct PsiAmpReadout {
    /// Whether the psi amp is the wielded weapon - the panel draws nothing at
    /// all otherwise.
    pub wielded: bool,
    /// Psi pool fraction (0..1).
    pub psi_fraction: f32,
    /// The hold-to-overload meter's state, or `None` when no charge is in
    /// progress (or nothing is wielded).
    pub charge: Option<RuntimePropPsiCharge>,
}

impl PsiAmpReadout {
    /// Read the wielded amp's readout from the world.
    pub(crate) fn from_world(world: &World) -> Self {
        let wielded = crate::wielded_weapon::wielded_weapon(world)
            .is_some_and(|weapon| crate::wielded_weapon::is_psi_amp(world, weapon));
        Self {
            wielded,
            psi_fraction: super::get_psi_percentage(world),
            charge: super::get_wielded_psi_charge(world),
        }
    }

    /// Nothing to say - the amp is not wielded. Neither presentation draws
    /// the panel at all when this is true.
    pub(crate) fn is_empty(&self) -> bool {
        !self.wielded
    }
}

/// Emit the readout's elements into `canvas`, with the panel's upper-left
/// corner at `origin`. Every placement decision lives here.
pub(crate) fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, readout: &PsiAmpReadout) {
    if !readout.wielded {
        return;
    }
    let at = |rect: Rect| Rect::new(origin.x + rect.x, origin.y + rect.y, rect.w, rect.h);
    canvas.bar(at(PSI_BAR), "PSIBAR.PCX", readout.psi_fraction);
    if let Some(charge) = &readout.charge {
        match charge.phase {
            PsiChargePhase::Charging => {
                canvas.image(at(OVERLOAD_BAR), "LOADBACK.PCX").bar(
                    at(OVERLOAD_BAR),
                    "LOADMETR.PCX",
                    charge.fraction,
                );
            }
            PsiChargePhase::Overloaded => {
                canvas.image(at(OVERLOAD_BAR), "LOADGOOD.PCX");
            }
            PsiChargePhase::Burnout => {
                canvas.image(at(OVERLOAD_BAR), "LOADBURN.PCX");
            }
        }
    }
}

/// The readout as a panel-sized canvas, the shape the amp-mounted panel lays
/// over its own root transform. Pure (no asset/GL access), so it is
/// unit-testable like `ammo_panel::build_readout_canvas`.
pub(crate) fn build_readout_canvas(readout: &PsiAmpReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(PANEL_SIZE);
    emit(&mut canvas, vec2(0.0, 0.0), readout);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unwielded_amp_draws_nothing() {
        // Negative case: no amp wielded, the panel is bare.
        let readout = PsiAmpReadout::default();
        assert!(readout.is_empty());
        assert_eq!(build_readout_canvas(&readout).element_count(), 0);
    }

    #[test]
    fn a_wielded_amp_shows_its_psi_pool() {
        let readout = PsiAmpReadout {
            wielded: true,
            psi_fraction: 0.5,
            charge: None,
        };
        assert!(!readout.is_empty());
        assert_eq!(build_readout_canvas(&readout).element_count(), 1);
    }

    #[test]
    fn no_charge_means_no_overload_meter() {
        let readout = PsiAmpReadout {
            wielded: true,
            psi_fraction: 1.0,
            charge: None,
        };
        let canvas = build_readout_canvas(&readout);
        assert_eq!(canvas.element_count(), 1, "only the psi bar should draw");
    }

    #[test]
    fn charging_adds_the_track_and_fill() {
        let readout = PsiAmpReadout {
            wielded: true,
            psi_fraction: 0.75,
            charge: Some(RuntimePropPsiCharge {
                fraction: 0.4,
                phase: PsiChargePhase::Charging,
            }),
        };
        // Psi bar + overload track + overload fill = 3.
        assert_eq!(build_readout_canvas(&readout).element_count(), 3);
    }

    #[test]
    fn a_flashed_result_shows_a_single_image_not_a_bar() {
        for phase in [PsiChargePhase::Overloaded, PsiChargePhase::Burnout] {
            let readout = PsiAmpReadout {
                wielded: true,
                psi_fraction: 0.2,
                charge: Some(RuntimePropPsiCharge {
                    fraction: 1.0,
                    phase,
                }),
            };
            // Psi bar + one result image = 2.
            assert_eq!(build_readout_canvas(&readout).element_count(), 2);
        }
    }

    #[test]
    fn placing_the_panel_offsets_position_but_not_size() {
        let mut canvas = UiCanvas::new(PANEL_SIZE);
        emit(
            &mut canvas,
            vec2(10.0, 20.0),
            &PsiAmpReadout {
                wielded: true,
                psi_fraction: 0.5,
                charge: None,
            },
        );
        match &canvas.elements()[0] {
            crate::ui::UiElement::Bar { position, size, .. } => {
                assert_eq!(*position, vec2(10.0 + PSI_BAR.x, 20.0 + PSI_BAR.y));
                assert_eq!(*size, vec2(PSI_BAR.w, PSI_BAR.h));
            }
            other => panic!("expected a Bar element, got {other:?}"),
        }
    }
}
