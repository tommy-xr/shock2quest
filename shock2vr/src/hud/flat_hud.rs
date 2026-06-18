//! Flatscreen (non-VR) screen-space HUD.
//!
//! Where `virtual_arms` renders the HUD as world-space panels on the VR hands,
//! this draws a classic 2D overlay: a centered crosshair plus health/psi bars,
//! laid out on the original game's 640x480 virtual canvas and scaled to the
//! actual screen. Built only in `PresentationMode::Flat`. See
//! `projects/flatscreen-and-vr-architecture.md` (Slice 1).

use std::rc::Rc;

use cgmath::{Matrix4, Vector2, vec2, vec3};
use dark::importers::{FONT_IMPORTER, TEXTURE_IMPORTER};
use engine::{
    assets::asset_cache::AssetCache,
    scene::SceneObject,
    texture::{TextureOptions, TextureTrait},
};
use shipyard::World;

use super::{get_health_percentage, get_psi_percentage};

/// The original SS2 HUD is authored against a 640x480 display; we lay out in
/// those virtual pixels and scale to the real screen.
const VIRTUAL_W: f32 = 640.0;
const VIRTUAL_H: f32 = 480.0;

const CROSSHAIR_SIZE: f32 = 32.0;

const BAR_W: f32 = 120.0;
const BAR_H: f32 = 12.0;
const HEALTH_BAR_X: f32 = 20.0;
const HEALTH_BAR_Y: f32 = 448.0; // near the bottom edge
const PSI_BAR_X: f32 = 20.0;
const PSI_BAR_Y: f32 = 430.0; // stacked just above health

const HEALTH_TEXT_X: f32 = 148.0; // right of the bars
const HEALTH_TEXT_Y: f32 = 444.0;
const HEALTH_TEXT_SIZE: f32 = 16.0;

/// A single laid-out HUD element in screen pixels (origin top-left). Kept as a
/// pure data description so the layout is unit-testable without a GL context.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FlatHudElement {
    Image {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
    },
    /// A horizontally-filling bar; `fill` (0..1) clips the texture from the left.
    Bar {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        fill: f32,
    },
    Text {
        position: Vector2<f32>,
        size: f32, // font height in screen pixels
        text: String,
    },
}

/// Compute the flat HUD layout in screen pixels for a given screen size and
/// player stat fractions. Pure: no asset/GL access, so it can be tested.
pub(crate) fn flat_hud_layout(
    screen_size: Vector2<f32>,
    health_fraction: f32,
    psi_fraction: f32,
) -> Vec<FlatHudElement> {
    let scale = vec2(screen_size.x / VIRTUAL_W, screen_size.y / VIRTUAL_H);
    // Scale a virtual-canvas point/size into actual screen pixels.
    let s = |x: f32, y: f32| vec2(x * scale.x, y * scale.y);

    let health = health_fraction.clamp(0.0, 1.0);
    let psi = psi_fraction.clamp(0.0, 1.0);

    let crosshair = s(CROSSHAIR_SIZE, CROSSHAIR_SIZE);

    vec![
        // Crosshair, centered on screen.
        FlatHudElement::Image {
            position: vec2(
                (screen_size.x - crosshair.x) / 2.0,
                (screen_size.y - crosshair.y) / 2.0,
            ),
            size: crosshair,
            texture: "CROSSHAI.PCX".to_owned(),
        },
        // Health bar (bottom-left), filled to the health fraction.
        FlatHudElement::Bar {
            position: s(HEALTH_BAR_X, HEALTH_BAR_Y),
            size: s(BAR_W, BAR_H),
            texture: "HPBAR.PCX".to_owned(),
            fill: health,
        },
        // Psi bar, stacked above health.
        FlatHudElement::Bar {
            position: s(PSI_BAR_X, PSI_BAR_Y),
            size: s(BAR_W, BAR_H),
            texture: "PSIBAR.PCX".to_owned(),
            fill: psi,
        },
        // Health percentage readout.
        FlatHudElement::Text {
            position: s(HEALTH_TEXT_X, HEALTH_TEXT_Y),
            size: HEALTH_TEXT_SIZE * scale.y,
            text: format!("{}", (health * 100.0).round() as i32),
        },
    ]
}

/// Replicate `SceneObject::screen_space_quad`'s transform so a manually-built
/// screen-space SceneObject (e.g. one using the clipped bar material) maps to
/// the same pixel rectangle.
fn screen_space_quad_transform(position: Vector2<f32>, size: Vector2<f32>) -> Matrix4<f32> {
    Matrix4::from_translation(vec3(position.x, position.y, 0.0))
        * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
        * Matrix4::from_translation(vec3(0.5, 0.5, 0.0))
}

/// Build the flat HUD as screen-space scene objects for the current player state.
pub(crate) fn create_flat_hud(
    asset_cache: &mut AssetCache,
    world: &World,
    screen_size: Vector2<f32>,
) -> Vec<SceneObject> {
    let layout = flat_hud_layout(
        screen_size,
        get_health_percentage(world),
        get_psi_percentage(world),
    );

    let texture_options = TextureOptions { wrap: false };
    let mut objs = Vec::with_capacity(layout.len());

    for element in layout {
        match element {
            FlatHudElement::Image {
                position,
                size,
                texture,
            } => {
                let tex = asset_cache.get_ext(&TEXTURE_IMPORTER, &texture, &texture_options);
                objs.push(SceneObject::screen_space_quad(
                    tex.clone() as Rc<dyn TextureTrait>,
                    position,
                    size,
                ));
            }
            FlatHudElement::Bar {
                position,
                size,
                texture,
                fill,
            } => {
                let tex = asset_cache.get_ext(&TEXTURE_IMPORTER, &texture, &texture_options);
                let material = engine::scene::clipped_screen_material::create_screen_space(
                    tex.clone() as Rc<dyn TextureTrait>,
                    fill,
                );
                let mut obj = SceneObject::new(material, Box::new(engine::scene::quad::create()));
                obj.set_local_transform(screen_space_quad_transform(position, size));
                objs.push(obj);
            }
            FlatHudElement::Text {
                position,
                size,
                text,
            } => {
                let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon").clone();
                objs.push(SceneObject::screen_space_text(
                    &text, font, size, 0.0, position.x, position.y,
                ));
            }
        }
    }

    objs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_has_crosshair_and_bars() {
        let layout = flat_hud_layout(vec2(640.0, 480.0), 1.0, 1.0);

        // Crosshair, two bars, one text readout.
        assert_eq!(layout.len(), 4);

        // Crosshair is centered on the 640x480 screen (32px wide -> 304,224).
        match &layout[0] {
            FlatHudElement::Image {
                position, texture, ..
            } => {
                assert_eq!(texture, "CROSSHAI.PCX");
                assert_eq!(*position, vec2(304.0, 224.0));
            }
            other => panic!("expected crosshair image, got {other:?}"),
        }
    }

    #[test]
    fn bar_fill_tracks_health_and_psi() {
        let layout = flat_hud_layout(vec2(640.0, 480.0), 0.25, 0.5);

        let fills: Vec<f32> = layout
            .iter()
            .filter_map(|e| match e {
                FlatHudElement::Bar { fill, texture, .. } if texture == "HPBAR.PCX" => Some(*fill),
                FlatHudElement::Bar { fill, texture, .. } if texture == "PSIBAR.PCX" => Some(*fill),
                _ => None,
            })
            .collect();
        assert_eq!(fills, vec![0.25, 0.5]);
    }

    #[test]
    fn fractions_are_clamped() {
        let layout = flat_hud_layout(vec2(640.0, 480.0), 2.0, -1.0);
        for e in &layout {
            if let FlatHudElement::Bar { fill, .. } = e {
                assert!((0.0..=1.0).contains(fill), "fill {fill} not clamped");
            }
        }
    }

    #[test]
    fn layout_scales_with_screen_size() {
        // At 2x the virtual canvas, the crosshair doubles in size.
        let layout = flat_hud_layout(vec2(1280.0, 960.0), 1.0, 1.0);
        match &layout[0] {
            FlatHudElement::Image { size, .. } => assert_eq!(*size, vec2(64.0, 64.0)),
            other => panic!("expected crosshair image, got {other:?}"),
        }
    }
}
