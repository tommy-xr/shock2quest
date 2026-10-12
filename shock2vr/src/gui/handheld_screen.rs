//! Map the shared GamePig content window onto its model's physical aperture.
use crate::{
    runtime_props::RuntimePropTransform,
    ui::{Rect, canvas_viewport::CanvasViewport},
};
use cgmath::{InnerSpace, Matrix3, Matrix4, Quaternion, Vector2, Vector3, vec2, vec3};
use shipyard::{Component, EntityId, Get, View, World};

#[derive(Clone, Copy, Component)]
pub(crate) struct HandheldScreen {
    pub center: Vector3<f32>,
    pub size: Vector2<f32>,
}

impl HandheldScreen {
    /// Both shipped model variants have a broad, flat screen at this aperture.
    /// Read its actual plane: the remastered glass is recessed more deeply
    /// than the original, so a fixed Z coordinate would bury one variant.
    pub fn from_triangles(triangles: &[[Vector3<f32>; 3]]) -> Option<Self> {
        let triangle = triangles.iter().find(|triangle| {
            let min_x = triangle.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
            let max_x = triangle
                .iter()
                .map(|p| p.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let min_y = triangle.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
            let max_y = triangle
                .iter()
                .map(|p| p.y)
                .fold(f32::NEG_INFINITY, f32::max);
            min_x < -0.10
                && max_x > 0.10
                && min_y < -0.10
                && max_y > 0.20
                && triangle.iter().all(|p| {
                    p.x.abs() < 0.12
                        && p.y > -0.12
                        && p.y < 0.23
                        && p.z < 0.0
                        && (p.z - triangle[0].z).abs() < 0.00001
                })
        })?;
        let z = triangle[0].z;
        let points: Vec<_> = triangles
            .iter()
            .filter(|tri| {
                tri.iter().all(|p| {
                    (p.z - z).abs() < 0.00001 && p.x.abs() < 0.12 && p.y > -0.12 && p.y < 0.23
                })
            })
            .flatten()
            .collect();
        // Inscribe a rectangle inside the slight trapezoid; preserve pixel
        // aspect with horizontal letterboxing rather than stretching sprites.
        let left = points
            .iter()
            .filter(|p| p.x < 0.0)
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let right = points
            .iter()
            .filter(|p| p.x > 0.0)
            .map(|p| p.x)
            .fold(f32::INFINITY, f32::min);
        let bottom = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let top = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        Some(Self {
            center: vec3((left + right) * 0.5, (bottom + top) * 0.5, z - 0.001),
            size: vec2(right - left, top - bottom),
        })
    }

    pub fn for_entity(world: &World, entity: EntityId) -> Option<Self> {
        world.borrow::<View<Self>>().ok()?.get(entity).ok().copied()
    }

    pub fn viewport(self) -> CanvasViewport {
        CanvasViewport::fit(Rect::new(13.0, 11.0, 140.0, 276.0), self.surface())
    }

    pub fn surface(self) -> Rect {
        Rect::new(0.0, 0.0, self.size.x * 1000.0, self.size.y * 1000.0)
    }

    pub fn transform(self, world: &World, entity: EntityId) -> Option<Matrix4<f32>> {
        let parent = world
            .borrow::<View<RuntimePropTransform>>()
            .ok()?
            .get(entity)
            .ok()?
            .0;
        Some(parent * Matrix4::from_translation(self.center))
    }

    pub fn proxy_pose(
        self,
        transform: Matrix4<f32>,
    ) -> (Vector3<f32>, Quaternion<f32>, Vector2<f32>) {
        let x = transform.x.truncate();
        let y = transform.y.truncate();
        let z = transform.z.truncate();
        (
            transform.w.truncate(),
            Quaternion::from(Matrix3::from_cols(
                x.normalize(),
                y.normalize(),
                z.normalize(),
            )),
            vec2(self.size.x * x.magnitude(), self.size.y * y.magnitude()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_both_screen_planes_and_uniformly_fits_the_same_canvas() {
        for (top, bottom, top_x, bottom_x, z) in [
            (0.2093348, -0.11308365, 0.10856037, 0.11087646, -0.012942314),
            (0.215906, -0.112886, 0.109015, 0.109015, -0.03008),
        ] {
            let tl = vec3(top_x, top, z);
            let tr = vec3(-top_x, top, z);
            let bl = vec3(bottom_x, bottom, z);
            let br = vec3(-bottom_x, bottom, z);
            let screen = HandheldScreen::from_triangles(&[
                [
                    vec3(-0.14, -0.28, -0.03),
                    vec3(0.14, -0.28, -0.03),
                    vec3(0.14, 0.28, -0.03),
                ],
                [
                    vec3(-0.14, -0.28, 0.03),
                    vec3(0.14, -0.28, 0.03),
                    vec3(0.14, 0.28, 0.03),
                ],
                [tl, tr, br],
                [tl, br, bl],
            ])
            .unwrap();
            assert!((screen.center.z - (z - 0.001)).abs() < 0.000001);
            assert!((screen.size.x - top_x * 2.0).abs() < 0.000001);
            let viewport = screen.viewport();
            assert!(
                (viewport.dst.w / viewport.src.w - viewport.dst.h / viewport.src.h).abs()
                    < 0.000001
            );
            let point = viewport.to_src_point(viewport.dst.center()).unwrap();
            assert!((point - vec2(83.0, 149.0)).magnitude() < 0.00001);
        }
        assert!(HandheldScreen::from_triangles(&[]).is_none());
    }
}
