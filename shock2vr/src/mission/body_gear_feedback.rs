//! Read-only body gear feedback. Inventory ownership stays in the pouch/holsters.
use cgmath::{Deg, Matrix4, vec3};
use engine::scene::{self, SceneObject};
use shipyard::{EntityId, World};

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum PouchState {
    Inactive,
    Ready,
    Empty,
    Refused,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct PouchReadout {
    pub weapon: Option<i32>,
    pub icon: Option<String>,
    pub count: Option<u32>,
    pub state: PouchState,
    pub near: bool,
}

impl PouchReadout {
    pub fn resolve(
        world: &World,
        held: [Option<EntityId>; 2],
        available: [bool; 2],
        near: [bool; 2],
        refused: bool,
        enabled: bool,
    ) -> Self {
        let guns = held.map(|item| {
            item.filter(|gun| {
                crate::scripts::script_util::active_gun_setting(world, *gun).is_some()
            })
        });
        // One shared pouch has no unambiguous draw while both hands hold guns.
        let (weapon, taking) = match guns {
            [Some(gun), None] => (Some(gun), 1),
            [None, Some(gun)] => (Some(gun), 0),
            _ => (None, 0),
        };
        let offer = weapon.and_then(|gun| super::reload::reserve_clip_for_pouch(world, gun));
        let state = if !enabled {
            PouchState::Inactive
        } else if refused {
            PouchState::Refused
        } else if weapon.is_none() || !available[taking] {
            PouchState::Inactive
        } else if offer.is_some() {
            PouchState::Ready
        } else {
            PouchState::Empty
        };
        Self {
            weapon: weapon.map(|id| id.inner() as i32),
            icon: crate::hud::get_weapon_ammo_icon(world, weapon),
            count: weapon.map(|gun| super::reload::reserve_rounds(world, gun)),
            state,
            near: near[taking],
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct HolsterReadout {
    pub weapon: Option<i32>,
    pub ammo: Option<i32>,
    pub capacity: Option<i32>,
    pub segments: usize,
}

impl HolsterReadout {
    pub fn resolve(world: &World, item: Option<EntityId>) -> Self {
        let ammo = crate::hud::get_weapon_ammo(world, item);
        let capacity = item
            .and_then(|id| crate::scripts::script_util::active_gun_setting(world, id))
            .map(|setting| setting.clip)
            .filter(|capacity| *capacity > 0);
        Self {
            weapon: item.map(|id| id.inner() as i32),
            ammo,
            capacity,
            segments: match (ammo, capacity) {
                (Some(ammo), Some(capacity)) => lit_segments(ammo as f32 / capacity as f32),
                _ => 0,
            },
        }
    }
}

fn lit_segments(fraction: f32) -> usize {
    if fraction.is_finite() {
        (fraction.clamp(0.0, 1.0) * 4.0).ceil() as usize
    } else {
        0
    }
}

thread_local! {
    // Reuse one small annulus geometry; changing state changes only its color.
    static RING: std::rc::Rc<Box<dyn scene::Geometry>> = {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for i in 0..=48 {
            let angle = i as f32 * std::f32::consts::TAU / 48.0;
            for radius in [0.43, 0.5] {
                vertices.push(scene::VertexPosition { position: vec3(angle.cos()*radius, angle.sin()*radius, 0.0) });
            }
            if i < 48 { let a = i*2; indices.extend([a, a+2, a+1, a+1, a+2, a+3]); }
        }
        std::rc::Rc::new(Box::new(scene::indexed_mesh::create(vertices, indices)))
    };
}

/// Shared by chest storage and the ammo pouch; readiness does not color idle rings.
pub(super) fn badge_color(
    targeted: bool,
    incompatible: bool,
    refusal_flash: bool,
) -> cgmath::Vector3<f32> {
    if incompatible || refusal_flash {
        vec3(0.7, 0.08, 0.02)
    } else if targeted {
        vec3(0.85, 0.85, 0.85)
    } else {
        vec3(0.3, 0.3, 0.3)
    }
}

/// Body-fixed upward tilt, shared by chest and ammo. No view/camera rotation.
pub(super) fn badge_frame(
    center: cgmath::Vector3<f32>,
    body_rotation: Matrix4<f32>,
    offset_m: cgmath::Vector3<f32>,
) -> Matrix4<f32> {
    Matrix4::from_translation(center)
        * body_rotation
        * Matrix4::from_scale(1.0 / crate::METERS_PER_WORLD_UNIT)
        * Matrix4::from_translation(offset_m)
        * Matrix4::from_angle_x(Deg(-60.0))
}

/// `plate` maps metre-local badge coordinates into world space. Its orientation
/// is supplied by the body mount, never by the viewing camera.
pub(super) fn item_badge(
    icon_name: Option<&str>,
    count: Option<u32>,
    color: cgmath::Vector3<f32>,
    plate: Matrix4<f32>,
    assets: &mut engine::assets::asset_cache::AssetCache,
) -> Vec<SceneObject> {
    let mut ring = RING.with(|geometry| {
        SceneObject::create(
            std::cell::RefCell::new(scene::color_material::create(color)),
            geometry.clone(),
        )
    });
    ring.set_transform(plate * Matrix4::from_scale(0.09));
    let mut objects = vec![ring];
    if let Some(name) = icon_name {
        if let Some(texture) = assets.get_ext_opt(
            &dark::importers::TEXTURE_IMPORTER,
            name,
            &engine::texture::TextureOptions {
                wrap: false,
                transparent_index_0: true,
                ..Default::default()
            },
        ) {
            // Keep the complete icon inside the circular rim without cropping.
            let width = texture.width() as f32;
            let height = texture.height() as f32;
            let longest = width.max(height).max(1.0);
            let texture: std::rc::Rc<dyn engine::texture::TextureTrait> = texture;
            let mut icon = SceneObject::new(
                scene::basic_material::create_with_fixed_ambient(texture, 1.0, 0.0),
                // PCX rows start at the top; retain the outward geometry winding
                // and reverse V so the complete authored icon reads upright.
                Box::new(scene::quad::create_with_uv(
                    cgmath::vec2(0.0, 1.0),
                    cgmath::vec2(1.0, 0.0),
                )),
            );
            icon.set_transform(
                plate
                    * Matrix4::from_translation(vec3(0.0, 0.0, 0.001))
                    * Matrix4::from_nonuniform_scale(
                        0.055 * width / longest,
                        0.055 * height / longest,
                        1.0,
                    ),
            );
            objects.push(icon);
        }
    }
    if let Some(count) = count {
        use crate::ui::{HAlign, Rect, UiCanvas, VAlign};
        // Author the number once in canvas pixels. The shared presenter resolves
        // glyph placement, including its top/centre anchoring, for both views.
        // The lower-right corner keeps the complete item icon unobscured.
        let mut canvas = UiCanvas::new(cgmath::vec2(100.0, 100.0));
        canvas.text_fit(
            Rect::new(55.0, 78.0, 43.0, 22.0),
            &count.to_string(),
            "mainfont.fon",
            22.0,
            HAlign::Right,
            VAlign::Top,
        );
        objects.extend(canvas.render_world_space(
            assets,
            plate * Matrix4::from_translation(vec3(0.0, 0.0, 0.002)) * Matrix4::from_scale(0.09),
            None,
            None,
            0.0,
        ));
    }
    objects
}

pub(super) fn holster(readout: &HolsterReadout, root: Matrix4<f32>) -> Vec<SceneObject> {
    if readout.weapon.is_none() {
        return vec![];
    }
    let color = match readout.ammo {
        Some(ammo) if ammo <= 0 => vec3(0.75, 0.035, 0.015),
        Some(_) if readout.segments <= 1 => vec3(0.65, 0.25, 0.02),
        Some(_) => vec3(0.06, 0.55, 0.22),
        None => vec3(0.025, 0.15, 0.2),
    };
    (0..4)
        .map(|i| {
            // Empty has no filled bars; a red, short marker replaces the first
            // bar so its silhouette differs from a full four-bar readout.
            let empty_marker = readout.ammo.is_some_and(|ammo| ammo <= 0) && i == 0;
            let lit = i < readout.segments || empty_marker || readout.ammo.is_none();
            let mut object = SceneObject::new(
                scene::color_material::create(if lit { color } else { vec3(0.012, 0.018, 0.02) }),
                Box::new(scene::cube::create()),
            );
            object.set_transform(
                root * Matrix4::from_translation(vec3(
                    0.055,
                    0.085 - i as f32 * 0.012,
                    0.019 - i as f32 * 0.012,
                )) * Matrix4::from_angle_x(Deg(-45.0))
                    * Matrix4::from_nonuniform_scale(
                        0.008,
                        0.003,
                        if empty_marker { 0.004 } else { 0.009 },
                    ),
            );
            object
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn badge_refusal_overrides_target_brightness_without_tinting_idle() {
        assert_eq!(badge_color(false, false, false), vec3(0.3, 0.3, 0.3));
        assert_eq!(badge_color(true, false, false), vec3(0.85, 0.85, 0.85));
        for targeted in [false, true] {
            assert_eq!(badge_color(targeted, true, false), vec3(0.7, 0.08, 0.02));
            assert_eq!(badge_color(targeted, false, true), vec3(0.7, 0.08, 0.02));
        }
    }

    #[test]
    fn each_holster_reads_its_own_gun_and_dual_wield_has_no_pouch_selection() {
        use dark::properties::{PropBaseGunDesc, PropGunState};
        let mut world = World::new();
        let guns = [(12, 12), (3, 6)].map(|(ammo, clip)| {
            let mut description = PropBaseGunDesc {
                settings: Default::default(),
            };
            for setting in &mut description.settings {
                setting.clip = clip;
            }
            world.add_entity((
                description,
                PropGunState {
                    ammo,
                    condition: 100.0,
                    setting: 0,
                    modification: 0,
                    silence_value: 0.0,
                },
            ))
        });
        let pistol = HolsterReadout::resolve(&world, Some(guns[0]));
        let shotgun = HolsterReadout::resolve(&world, Some(guns[1]));
        assert_eq!(
            (pistol.ammo, pistol.capacity, pistol.segments),
            (Some(12), Some(12), 4)
        );
        assert_eq!(
            (shotgun.ammo, shotgun.capacity, shotgun.segments),
            (Some(3), Some(6), 2)
        );
        let pouch =
            PouchReadout::resolve(&world, guns.map(Some), [false; 2], [true; 2], false, true);
        assert_eq!(pouch.weapon, None);
        assert_eq!(pouch.icon, None);
        assert_eq!(pouch.state, PouchState::Inactive);
        let disabled = PouchReadout::resolve(&world, [None; 2], [true; 2], [true; 2], true, false);
        assert_eq!(disabled.state, PouchState::Inactive);
    }

    #[test]
    fn segments_distinguish_empty_partial_and_full_and_reject_bad_values() {
        assert_eq!(lit_segments(0.0), 0);
        assert_eq!(lit_segments(0.01), 1);
        assert_eq!(lit_segments(0.5), 2);
        assert_eq!(lit_segments(1.0), 4);
        assert_eq!(lit_segments(2.0), 4);
        assert_eq!(lit_segments(f32::NAN), 0);
    }
}
