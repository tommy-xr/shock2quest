//! Screen-space bounds of a sphere, for skipping and narrowing an offscreen
//! pass that only a sphere-shaped surface samples.
use cgmath::{Matrix4, Point3, Vector2, Vector3, vec2, vec4};
use collision::{Frustum, Relation, Sphere};

/// A rect of a view's normalized device coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NdcRect {
    pub min: Vector2<f32>,
    pub max: Vector2<f32>,
}

impl NdcRect {
    pub const FULL: Self = Self {
        min: vec2(-1.0, -1.0),
        max: vec2(1.0, 1.0),
    };

    /// Stretches this rect over the clip square: `crop() * projection` renders
    /// only the rect, with the full target resolution.
    pub fn crop(&self) -> Matrix4<f32> {
        let scale = vec2(
            2.0 / (self.max.x - self.min.x),
            2.0 / (self.max.y - self.min.y),
        );
        let mid = (self.min + self.max) * 0.5;
        Matrix4::new(
            scale.x,
            0.0,
            0.0,
            0.0, //
            0.0,
            scale.y,
            0.0,
            0.0, //
            0.0,
            0.0,
            1.0,
            0.0, //
            -scale.x * mid.x,
            -scale.y * mid.y,
            0.0,
            1.0,
        )
    }

    /// Texture coordinates (0..1 across the rect) of a point in the view's NDC.
    pub fn uv(&self, ndc: Vector2<f32>) -> Vector2<f32> {
        vec2(
            (ndc.x - self.min.x) / (self.max.x - self.min.x),
            (ndc.y - self.min.y) / (self.max.y - self.min.y),
        )
    }
}

/// The sphere an offscreen pass is drawn for: the pass covers only the part of
/// the view the sphere covers, plus `margin` NDC units for samples a material
/// bends past its silhouette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassFocus {
    pub center: Vector3<f32>,
    pub radius: f32,
    pub margin: f32,
}

impl PassFocus {
    /// `bounds` clipped to the view: what a pass rendered for this sphere
    /// covers.
    pub fn rect(&self, view: Matrix4<f32>, projection: Matrix4<f32>) -> Option<NdcRect> {
        let bounds = self.bounds(view, projection)?;
        let rect = NdcRect {
            min: bounds.min.map(|v| v.max(-1.0)),
            max: bounds.max.map(|v| v.min(1.0)),
        };
        (rect.min.x < rect.max.x && rect.min.y < rect.max.y).then_some(rect)
    }

    /// The rect of `projection`'s NDC the sphere covers from `view`, grown by
    /// `margin`, which may reach past the view; `None` while the sphere is
    /// off-screen. A sphere reaching behind the eye plane gets the whole view.
    pub fn bounds(&self, view: Matrix4<f32>, projection: Matrix4<f32>) -> Option<NdcRect> {
        if !sphere_in_frustum(projection * view, self.center, self.radius) {
            return None;
        }
        let center = (view * self.center.extend(1.0)).truncate();
        let depth = -center.z;
        let r = self.radius;
        let bounds = if depth > r * 1.001 {
            // Slopes (offset / depth) of the two tangent planes per axis.
            let tangents = |offset: f32| {
                let d = depth * depth - r * r;
                let spread = r * (offset * offset + d).sqrt();
                ((offset * depth - spread) / d, (offset * depth + spread) / d)
            };
            let (x0, x1) = tangents(center.x);
            let (y0, y1) = tangents(center.y);
            let corners = [(x0, y0), (x0, y1), (x1, y0), (x1, y1)].map(|(x, y)| {
                let clip = projection * vec4(x, y, -1.0, 1.0);
                vec2(clip.x / clip.w, clip.y / clip.w)
            });
            let mut rect = NdcRect {
                min: corners[0],
                max: corners[0],
            };
            for corner in &corners[1..] {
                rect.min = vec2(rect.min.x.min(corner.x), rect.min.y.min(corner.y));
                rect.max = vec2(rect.max.x.max(corner.x), rect.max.y.max(corner.y));
            }
            rect
        } else {
            NdcRect::FULL
        };
        let margin = vec2(self.margin, self.margin);
        Some(NdcRect {
            min: bounds.min - margin,
            max: bounds.max + margin,
        })
    }
}

/// How a material finds a world point in a pass's texture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampling {
    /// World to the clip space of the camera the pass was focused from,
    /// without the pass's translation.
    pub view_projection: Matrix4<f32>,
    /// The part of that view the texture covers.
    pub rect: NdcRect,
}

impl Sampling {
    /// Texture coordinates of `world`, before any material distortion.
    pub fn uv(&self, world: Vector3<f32>) -> Vector2<f32> {
        let clip = self.view_projection * world.extend(1.0);
        self.rect.uv(vec2(clip.x / clip.w, clip.y / clip.w))
    }
}

/// How a pass renders and is sampled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassCamera {
    pub view: Matrix4<f32>,
    pub projection: Matrix4<f32>,
    pub sampling: Sampling,
}

/// The pass for `focus` seen by a camera `view`/`projection`, its eye moved
/// by `translation`: where the camera sees world point `p`, the pass shows
/// `p + translation`, narrowed to the focus rect. `None` while off-screen.
pub fn pass_camera(
    view: Matrix4<f32>,
    projection: Matrix4<f32>,
    translation: Vector3<f32>,
    focus: PassFocus,
) -> Option<PassCamera> {
    let rect = focus.rect(view, projection)?;
    Some(PassCamera {
        view: view * Matrix4::from_translation(-translation),
        projection: rect.crop() * projection,
        sampling: Sampling {
            view_projection: projection * view,
            rect,
        },
    })
}

/// The pass's view, and a projection narrowed to `focus` without clipping to
/// the view. For culling what a pass may draw from a camera close to the one
/// it renders with (another eye, last frame), with `focus` grown to cover the
/// difference.
pub fn cull_camera(
    view: Matrix4<f32>,
    projection: Matrix4<f32>,
    translation: Vector3<f32>,
    focus: PassFocus,
) -> Option<(Matrix4<f32>, Matrix4<f32>)> {
    let bounds = focus.bounds(view, projection)?;
    Some((
        view * Matrix4::from_translation(-translation),
        bounds.crop() * projection,
    ))
}

/// Whether a sphere touches the frustum of `view_projection` (OpenGL clip
/// space). Conservative: tested against each plane alone, so a sphere just
/// outside a frustum corner still counts as inside.
pub fn sphere_in_frustum(view_projection: Matrix4<f32>, center: Vector3<f32>, radius: f32) -> bool {
    let sphere = Sphere {
        center: Point3::new(center.x, center.y, center.z),
        radius,
    };
    Frustum::from_matrix4(view_projection)
        .is_none_or(|frustum| frustum.contains(&sphere) != Relation::Out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{
        Deg, ElementWise, InnerSpace, Quaternion, Rotation, Rotation3, frustum, perspective, vec3,
    };

    fn camera() -> Matrix4<f32> {
        // Eye at the origin looking down -Z, 90 degrees wide and tall.
        perspective(Deg(90.0), 1.0, 0.1, 100.0)
    }

    #[test]
    fn sphere_ahead_is_in_view_and_behind_is_not() {
        assert!(sphere_in_frustum(camera(), vec3(0.0, 0.0, -6.0), 1.0));
        assert!(!sphere_in_frustum(camera(), vec3(0.0, 0.0, 6.0), 1.0));
        // Wholly beyond the right plane (x = -z) at depth 6.
        assert!(!sphere_in_frustum(camera(), vec3(8.0, 0.0, -6.0), 1.0));
    }

    #[test]
    fn sphere_overlapping_a_plane_counts() {
        // Centre 0.5 past the right plane, radius reaches back across it.
        let center = vec3(6.0 + 0.5 * 2f32.sqrt(), 0.0, -6.0);
        assert!(sphere_in_frustum(camera(), center, 1.0));
        assert!(!sphere_in_frustum(camera(), center, 0.4));
        // Straddling the eye still draws.
        assert!(sphere_in_frustum(camera(), vec3(0.0, 0.0, 0.0), 1.0));
    }

    #[test]
    fn rect_hugs_a_sphere_ahead() {
        let focus = PassFocus {
            center: vec3(0.0, 0.0, -6.0),
            radius: 1.0,
            margin: 0.0,
        };
        let rect = focus
            .rect(Matrix4::from_scale(1.0), camera())
            .expect("in view");
        // tan(asin(1/6)) at 90 degrees FOV.
        let half = 1.0 / 35f32.sqrt();
        assert!((rect.max.x - half).abs() < 1e-5 && (rect.min.x + half).abs() < 1e-5);
        assert!((rect.max.y - half).abs() < 1e-5 && (rect.min.y + half).abs() < 1e-5);
        // Margin grows it, clipped to the view.
        let wide = PassFocus {
            margin: 0.9,
            ..focus
        };
        let rect = wide.rect(Matrix4::from_scale(1.0), camera()).unwrap();
        assert_eq!(rect, NdcRect::FULL);
        let bounds = wide.bounds(Matrix4::from_scale(1.0), camera()).unwrap();
        assert!((bounds.max.x - (half + 0.9)).abs() < 1e-5);
        // Off-screen gives nothing; around the eye gives the whole view.
        let behind = PassFocus {
            center: vec3(0.0, 0.0, 6.0),
            ..focus
        };
        assert!(behind.rect(Matrix4::from_scale(1.0), camera()).is_none());
        let around = PassFocus {
            center: vec3(0.0, 0.0, -0.5),
            ..focus
        };
        assert_eq!(
            around.rect(Matrix4::from_scale(1.0), camera()),
            Some(NdcRect::FULL)
        );
    }

    #[test]
    fn sphere_point_maps_to_the_same_texel_for_pass_and_material() {
        // A turned eye with an asymmetric (VR-like) projection, the pass eye
        // moved by a remote offset.
        let eye = vec3(2.0, 1.0, 3.0);
        let rotation = Quaternion::from_angle_y(Deg(25.0)) * Quaternion::from_angle_x(Deg(-10.0));
        let forward = rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
        let view = Matrix4::look_to_rh(
            Point3::new(eye.x, eye.y, eye.z),
            forward,
            vec3(0.0, 1.0, 0.0),
        );
        let projection = frustum(-0.12, 0.08, -0.1, 0.09, 0.1, 100.0);
        let focus = PassFocus {
            center: eye + forward * 7.0 + vec3(0.6, -0.3, 0.0),
            radius: 1.1,
            margin: 0.3,
        };
        let translation = vec3(-4.0, 0.5, 6.0);
        let camera = pass_camera(view, projection, translation, focus).expect("in view");
        let size = 256.0;
        let texel = |uv: Vector2<f32>| (uv * size).map(|v| v.floor());
        for i in 0..12 {
            for j in 1..6 {
                let (yaw, pitch) = (i as f32 * 30.0, j as f32 * 30.0 - 90.0);
                let normal =
                    Quaternion::from_angle_y(Deg(yaw)) * Quaternion::from_angle_x(Deg(pitch));
                let point = focus.center + normal.rotate_vector(vec3(0.0, 0.0, focus.radius));
                // The material's lookup.
                let sampled = camera.sampling.uv(point);
                // Where the pass drew what lies behind that point.
                let clip = camera.projection * camera.view * (point + translation).extend(1.0);
                let drawn = vec2(clip.x / clip.w, clip.y / clip.w) * 0.5 + vec2(0.5, 0.5);
                assert!(
                    (sampled - drawn).magnitude() < 1e-4,
                    "{sampled:?} vs {drawn:?}"
                );
                assert_eq!(texel(sampled), texel(drawn));
                // Samples bent by up to the margin still land in the texture.
                let ndc = camera.sampling.rect.min
                    + (camera.sampling.rect.max - camera.sampling.rect.min)
                        .mul_element_wise(sampled);
                for bend in [vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, -1.0)] {
                    let uv = camera.sampling.rect.uv(ndc + bend * focus.margin * 0.999);
                    assert!(
                        (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y),
                        "{uv:?}"
                    );
                }
            }
        }
    }
}
