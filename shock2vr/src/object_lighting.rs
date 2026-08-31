//! Per-object lighting: which of a mission's lights shade a given object.
//!
//! World geometry is lit by baked lightmaps, but objects - props, creatures,
//! held items - are not in those lightmaps. The original engine lights them
//! from a second database shipped alongside: a table of lights, and per cell
//! the list of lights that reach it. Lighting an object is therefore "find its
//! cell, evaluate that cell's lights", which is what this module does.
//!
//! The renderer has fewer light slots than a room can have lights, so the set
//! is ranked by how much each light contributes at the object's position and
//! the strongest are kept.

use cgmath::{InnerSpace, Vector3, Vector4, vec3};
use dark::mission::{LightTable, WorldLight};
use engine::scene::light::{LightArray, PointLight, SpotLight};

use crate::dev_params;
use crate::mission::spatial_query::SpatialQueryEngine;

/// How many lights the renderer can apply to one object.
const MAX_OBJECT_LIGHTS: usize = 6;

/// Stop once the kept lights account for this much of the total contribution -
/// the remainder cannot meaningfully change the shading.
const ENERGY_CUTOFF: f32 = 0.95;

/// Rank lights with the same source radius the shader shades them with. A
/// light inside its own lamp model would otherwise score in the hundreds and
/// take the entire energy budget, ejecting every light that actually matters.
const MIN_DISTANCE: f32 = engine::scene::light::LIGHT_SOURCE_RADIUS;

/// Brightness is authored against the mission's own units, but our world is
/// `SCALE_FACTOR` smaller - so every distance is smaller by that factor and the
/// inverse-distance falloff would come out that much brighter. Scale the
/// brightness back down by the same factor to land where the original did.
const BRIGHTNESS_SCALE: f32 = 1.0 / dark::SCALE_FACTOR;

/// Perceptual weighting used to rank lights against each other - green carries
/// most of the apparent brightness.
pub fn perceived_brightness(brightness: Vector3<f32>) -> f32 {
    0.25 * brightness.x + 0.5 * brightness.y + 0.25 * brightness.z
}

/// How much of a spotlight's cone covers `position`: 1 inside the inner cone,
/// falling linearly to 0 at the outer one. Omni lights are always 1.
///
/// This ranks lights; it does NOT scale the light we hand the renderer. The
/// shader evaluates the cone per fragment, which is finer than the whole-object
/// answer here - pre-scaling as well would apply the cone twice.
fn cone_coverage(light: &WorldLight, position: Vector3<f32>) -> f32 {
    if !light.is_spotlight() {
        return 1.0;
    }

    let to_position = position - light.position;
    if to_position.magnitude2() < MIN_DISTANCE * MIN_DISTANCE {
        return 1.0;
    }

    let alignment = to_position.normalize().dot(light.direction.normalize());
    if alignment <= light.outer {
        return 0.0;
    }
    if alignment >= light.inner {
        return 1.0;
    }

    (alignment - light.outer) / (light.inner - light.outer)
}

/// How much light an object at `position` receives from `lights`, ignoring
/// which way its surfaces face. This is the scalar the original engine used to
/// answer "how lit is this object" - including, notably, for whether AI can see
/// the player - so it is the right summary number for tooling to report.
pub fn received_light(lights: &LightArray, position: Vector3<f32>) -> f32 {
    lights
        .iter_active()
        .map(|(_, light)| {
            let color = light.color_intensity();
            let brightness = vec3(color.x, color.y, color.z) * color.w;
            let distance = (light.position() - position).magnitude().max(MIN_DISTANCE);
            perceived_brightness(brightness) / distance
        })
        .sum()
}

/// The lights that shade an object at `position`, strongest first.
pub fn lights_for_position(
    spatial: &dyn SpatialQueryEngine,
    table: &LightTable,
    position: Vector3<f32>,
) -> LightArray {
    let ambient_boost = dev_params::get(dev_params::OBJECT_LIGHT_AMBIENT_BOOST);
    let ambient = spatial.get_ambient_light() + vec3(ambient_boost, ambient_boost, ambient_boost);
    let mut lights = LightArray::new()
        .with_object_lighting(ambient, dev_params::get(dev_params::OBJECT_LIGHT_WRAP));

    let Some(cell) = spatial.get_cell_from_position(position) else {
        // Outside the world rep (or in a scene with none) - no cell, no lights.
        return lights;
    };

    let mut candidates: Vec<(f32, &WorldLight)> = Vec::new();
    let mut total_contribution = 0.0;

    for index in &cell.light_indices {
        let Some(light) = table.get(*index) else {
            continue;
        };

        let to_light = light.position - position;
        let distance2 = to_light.magnitude2();
        if light.radius > 0.0 && distance2 > light.radius * light.radius {
            continue;
        }

        let coverage = cone_coverage(light, position);
        if coverage <= 0.0 {
            continue;
        }

        let distance = distance2.sqrt().max(MIN_DISTANCE);
        let contribution = perceived_brightness(light.brightness) * coverage / distance;
        if contribution <= 0.0 {
            continue;
        }

        total_contribution += contribution;
        candidates.push((contribution, light));
    }

    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));

    let enough = total_contribution * ENERGY_CUTOFF;
    let mut kept = 0.0;
    for (contribution, light) in candidates.into_iter().take(MAX_OBJECT_LIGHTS) {
        lights.add_light(to_scene_light(light));
        kept += contribution;
        if kept >= enough {
            break;
        }
    }

    lights
}

/// Convert a parsed light into the renderer's representation. Cone angles are
/// stored as cosines and the renderer wants radians; a light with no radius
/// reaches everywhere, which the renderer spells as an enormous range.
fn to_scene_light(light: &WorldLight) -> engine::scene::light::SceneLight {
    let scale = BRIGHTNESS_SCALE * dev_params::get(dev_params::OBJECT_LIGHT_BRIGHTNESS);
    let color_intensity = Vector4::new(
        light.brightness.x * scale,
        light.brightness.y * scale,
        light.brightness.z * scale,
        1.0,
    );
    let range = if light.radius > 0.0 {
        light.radius
    } else {
        f32::MAX
    };

    if light.is_spotlight() {
        SpotLight {
            position: light.position,
            direction: normalize_or_down(light.direction),
            color_intensity,
            inner_cone_angle: light.inner.clamp(-1.0, 1.0).acos(),
            outer_cone_angle: light.outer.clamp(-1.0, 1.0).acos(),
            range,
        }
        .into()
    } else {
        PointLight {
            position: light.position,
            color_intensity,
            range,
        }
        .into()
    }
}

fn normalize_or_down(direction: Vector3<f32>) -> Vector3<f32> {
    if direction.magnitude2() > 0.0 {
        direction.normalize()
    } else {
        vec3(0.0, -1.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn omni(position: Vector3<f32>, brightness: f32, radius: f32) -> WorldLight {
        WorldLight {
            position,
            direction: vec3(0.0, 0.0, 0.0),
            brightness: vec3(brightness, brightness, brightness),
            inner: -1.0,
            outer: 0.0,
            radius,
        }
    }

    fn spot(position: Vector3<f32>, direction: Vector3<f32>) -> WorldLight {
        WorldLight {
            position,
            direction,
            brightness: vec3(1.0, 1.0, 1.0),
            // 60 degrees to full-off at 90.
            inner: 0.5,
            outer: 0.0,
            radius: 0.0,
        }
    }

    #[test]
    fn a_light_outside_its_radius_does_not_reach() {
        let light = omni(vec3(0.0, 0.0, 0.0), 1.0, 5.0);
        assert!(light.radius > 0.0);

        // The radius test lives in lights_for_position; assert the geometry it
        // relies on rather than duplicating the loop.
        let inside = vec3(4.0, 0.0, 0.0);
        let outside = vec3(6.0, 0.0, 0.0);
        assert!((inside - light.position).magnitude2() <= light.radius * light.radius);
        assert!((outside - light.position).magnitude2() > light.radius * light.radius);
    }

    #[test]
    fn a_cone_covers_what_it_points_at_and_nothing_behind_it() {
        let light = spot(vec3(0.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0));

        assert_eq!(cone_coverage(&light, vec3(0.0, -10.0, 0.0)), 1.0, "on axis");
        assert_eq!(cone_coverage(&light, vec3(0.0, 10.0, 0.0)), 0.0, "behind");

        // Inside the inner cone (cosine 0.5 = 60 degrees) is still full.
        assert_eq!(cone_coverage(&light, vec3(7.0, -7.0, 0.0)), 1.0, "45 deg");

        // Between the cones - alignment 0.25, i.e. ~75 degrees off axis.
        let edge = cone_coverage(&light, vec3(9.68, -2.5, 0.0));
        assert!(
            edge > 0.0 && edge < 1.0,
            "between the cones it should fall off, got {edge}"
        );
    }

    #[test]
    fn an_omni_covers_every_direction() {
        let light = omni(vec3(0.0, 0.0, 0.0), 1.0, 0.0);
        for position in [
            vec3(5.0, 0.0, 0.0),
            vec3(-5.0, 0.0, 0.0),
            vec3(0.0, 5.0, 0.0),
        ] {
            assert_eq!(cone_coverage(&light, position), 1.0);
        }
    }

    #[test]
    fn brightness_ranking_weights_green_most() {
        assert!(
            perceived_brightness(vec3(0.0, 1.0, 0.0)) > perceived_brightness(vec3(1.0, 0.0, 0.0))
        );
        assert_eq!(perceived_brightness(vec3(1.0, 1.0, 1.0)), 1.0);
    }

    #[test]
    fn an_omni_becomes_a_point_light_and_a_cone_becomes_a_spotlight() {
        let point = to_scene_light(&omni(vec3(1.0, 2.0, 3.0), 0.5, 0.0));
        assert!(point.inner_cone_angle() < 0.0, "a point light has no cone");
        assert_eq!(point.range(), f32::MAX, "radius 0 reaches everywhere");

        let cone = to_scene_light(&spot(vec3(0.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0)));
        assert!(cone.inner_cone_angle() >= 0.0);
        // inner cosine 0.5 is 60 degrees.
        assert!((cone.inner_cone_angle().to_degrees() - 60.0).abs() < 0.01);
    }

    #[test]
    fn a_zero_direction_cone_does_not_produce_a_nan_direction() {
        let mut light = spot(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0));
        light.inner = 0.5;
        let converted = to_scene_light(&light);
        let direction = match converted {
            engine::scene::light::SceneLight::Spot(spot) => spot.direction,
            engine::scene::light::SceneLight::Point(_) => panic!("expected a spotlight"),
        };
        assert!(direction.magnitude2().is_finite() && direction.magnitude2() > 0.0);
    }
}
