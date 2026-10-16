//! Render-only presentation of the Quantum Relocation anchor: a sphere whose
//! surface shows a live view from a remote camera, bent like a glass ball.
//! The sphere hangs at the anchor, so with porthole parallax that camera is
//! the player's own eye and the sphere reads as a refracting rift; the remote
//! view only differs once a sphere is placed away from its anchor.
//! Teleport gameplay stays in `psi_teleport`. Gated by the `psi_portal`
//! experimental flag until its Quest cost is measured.
use std::{cell::RefCell, collections::HashSet, rc::Rc};

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Rad, Rotation, Vector3, vec3};
use engine::scene::{
    Material, OffscreenPass, RenderLayer, SceneObject,
    material::eye_position,
    pass_focus::{PassFocus, cull_camera},
    portal_material,
    render_target::RenderTarget,
    sphere::Sphere,
};
use shipyard::EntityId;

use crate::mission::visibility_engine::VisibilityEngine;

pub const EXPERIMENTAL_FLAG: &str = "psi_portal";
/// World units: reads from across a room without swallowing a corridor wall.
pub const RADIUS: f32 = 1.1;
/// Extra clearance between the sphere and the player's eye, which is only
/// approximated by the default head height (a tracked head moves).
const EYE_CLEARANCE: f32 = 0.5;
/// Screen-UV push from the surface normal: the glass-ball refraction.
const DISTORTION: f32 = 0.12;
/// Fresnel exponent for the rim glow.
const RIM_POWER: f32 = 2.5;
const RIM_COLOR: Vector3<f32> = vec3(0.35, 0.75, 1.0);
/// Angular slack for the on-screen test and the pass cull, which see last
/// frame's culling view (on Quest the right eye's, narrower than the left
/// eye's and the head-centre pass on their outer side).
const VIEW_SLACK: Deg<f32> = Deg(10.0);
pub const RENDER_SOURCE: &str = "psi_portal";

pub struct PsiPortal {
    target: Rc<RenderTarget>,
    material: Rc<RefCell<Box<dyn Material>>>,
}

impl Default for PsiPortal {
    fn default() -> Self {
        let target = Rc::new(RenderTarget::default());
        let material = portal_material::create(target.clone(), RIM_COLOR, DISTORTION, RIM_POWER);
        Self {
            target,
            material: Rc::new(RefCell::new(material)),
        }
    }
}

/// The eye point of a pawn origin under the runtimes' default head offset.
pub fn eye_point(pawn: Vector3<f32>) -> Vector3<f32> {
    pawn + vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0)
}

/// `point` moved `offset` units along `rotation`'s horizontal facing (+Z at
/// yaw 0, and when the rotation has no horizontal facing).
pub fn offset_along_yaw(
    point: Vector3<f32>,
    rotation: Quaternion<f32>,
    offset: f32,
) -> Vector3<f32> {
    let forward = rotation.rotate_vector(vec3(0.0, 0.0, 1.0));
    let flat = vec3(forward.x, 0.0, forward.z);
    let direction = if flat.magnitude2() > 1.0e-6 {
        flat.normalize()
    } else {
        vec3(0.0, 0.0, 1.0)
    };
    point + direction * offset
}

/// Porthole parallax: the remote camera moves relative to the anchor exactly
/// as the player's eye moves relative to the sphere. Head rotation is shared
/// separately, by keeping the main camera's rotation.
pub fn remote_eye(
    anchor_eye: Vector3<f32>,
    sphere_center: Vector3<f32>,
    player_eye: Vector3<f32>,
) -> Vector3<f32> {
    anchor_eye + (player_eye - sphere_center)
}

/// Where the open portal's sphere and remote camera are this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalView {
    pub anchor_eye: Vector3<f32>,
    pub sphere_center: Vector3<f32>,
}

impl PortalView {
    /// The sphere hangs at the anchor's eye point: where the player's eye was
    /// when the marker was cast. `remote_offset` (the `psi_portal_remote_offset`
    /// dev param, 0 = none) moves the remote camera off the anchor to preview a
    /// genuine other-place view.
    pub fn at_marker(marker: Vector3<f32>, rotation: Quaternion<f32>, remote_offset: f32) -> Self {
        let eye = eye_point(marker);
        Self {
            anchor_eye: offset_along_yaw(eye, rotation, remote_offset),
            sphere_center: eye,
        }
    }

    /// `remote_eye(..) - player_eye`: constant, so it is `remote_eye` of an eye
    /// at the origin. Applied to the rendering camera, it keeps the tracked
    /// head pose.
    pub fn camera_translation(&self) -> Vector3<f32> {
        remote_eye(self.anchor_eye, self.sphere_center, vec3(0.0, 0.0, 0.0))
    }

    /// Whether the player's eye is clearly outside the sphere; inside, the
    /// opaque surface would fill the whole view.
    pub fn clear_of(&self, player_eye: Vector3<f32>) -> bool {
        (player_eye - self.sphere_center).magnitude() > RADIUS + EYE_CLEARANCE
    }

    /// The pass renders, and is culled, only where the sphere is on screen.
    pub fn focus(&self) -> PassFocus {
        PassFocus {
            center: self.sphere_center,
            radius: RADIUS,
            margin: portal_material::sample_margin(DISTORTION),
        }
    }

    /// The pass's culling view and projection from a camera `view`/
    /// `projection`, narrowed to the sphere grown by `VIEW_SLACK` so a head
    /// turn or the other eye never misses what the pass draws; `None` while
    /// even that is off-screen.
    pub fn cull(
        &self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
    ) -> Option<(Matrix4<f32>, Matrix4<f32>)> {
        let distance = (eye_position(&view) - self.sphere_center).magnitude();
        let focus = PassFocus {
            radius: RADIUS + distance * Rad::from(VIEW_SLACK).0.tan(),
            ..self.focus()
        };
        cull_camera(view, projection, self.camera_translation(), focus)
    }
}

/// One entity build feeds both views: entities only the remote camera sees go
/// to `pass_only`; entities only the player sees are kept out of the pass.
#[derive(Default)]
pub struct PortalCull {
    active: bool,
    hidden_from_pass: HashSet<u64>,
    pub pass_only: Vec<SceneObject>,
}

impl PortalCull {
    pub fn new(active: bool) -> Self {
        Self {
            active,
            ..Default::default()
        }
    }

    /// Whether the portal's remote view sees `entity`. Always false with no
    /// portal open, so the main scene's culling is untouched.
    pub fn sees(
        &mut self,
        visibility: &mut dyn VisibilityEngine,
        entity: EntityId,
        main_visible: bool,
    ) -> bool {
        if !self.active {
            return false;
        }
        let remote = visibility.is_visible_secondary(entity);
        if main_visible && !remote {
            self.hidden_from_pass.insert(entity.inner());
        }
        remote
    }
}

impl PsiPortal {
    /// The sphere and the pass it samples. The pass draws the world layer of
    /// `scene` that the remote camera can see, plus `cull.pass_only`.
    pub fn render(
        &self,
        view: PortalView,
        scene: &[SceneObject],
        cull: PortalCull,
    ) -> (SceneObject, Rc<OffscreenPass>) {
        let hidden = &cull.hidden_from_pass;
        let objects = scene
            .iter()
            .filter(|object| object.render_layer() == RenderLayer::World)
            .filter(|object| {
                object
                    .debug_tag()
                    .and_then(|tag| tag.entity_id)
                    .is_none_or(|id| !hidden.contains(&id))
            })
            .cloned()
            .chain(cull.pass_only)
            .collect();
        let pass = OffscreenPass {
            target: self.target.clone(),
            camera_translation: view.camera_translation(),
            focus: view.focus(),
            objects,
        };
        let mut sphere = SceneObject::create_with_shared_material(
            self.material.clone(),
            Rc::new(Box::new(Sphere)),
        );
        sphere.set_transform(
            Matrix4::from_translation(view.sphere_center) * Matrix4::from_scale(RADIUS),
        );
        let mut objects = [sphere];
        crate::util::tag_render_source(&mut objects, RENDER_SOURCE);
        let [sphere] = objects;
        (sphere, Rc::new(pass))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{EuclideanSpace, Rotation3};

    const IDENTITY: Quaternion<f32> = Quaternion::new(1.0, 0.0, 0.0, 0.0);

    #[test]
    fn remote_offset_follows_the_marker_yaw() {
        let p = vec3(1.0, 2.0, 3.0);
        assert_eq!(offset_along_yaw(p, IDENTITY, 6.0), p + vec3(0.0, 0.0, 6.0));
        let east = offset_along_yaw(p, Quaternion::from_angle_y(Deg(90.0)), 6.0);
        assert!((east - (p + vec3(6.0, 0.0, 0.0))).magnitude() < 1e-5);
        // Facing straight up has no yaw: fall back to +Z.
        let up = Quaternion::from_angle_x(Deg(-90.0));
        assert!((offset_along_yaw(p, up, 6.0) - (p + vec3(0.0, 0.0, 6.0))).magnitude() < 1e-5);
        // Zero keeps the anchor, so the camera translation stays zero.
        let view = PortalView::at_marker(p, Quaternion::from_angle_y(Deg(30.0)), 0.0);
        assert_eq!(view.camera_translation(), vec3(0.0, 0.0, 0.0));
        let view = PortalView::at_marker(p, IDENTITY, 6.0);
        assert_eq!(view.camera_translation(), vec3(0.0, 0.0, 6.0));
        assert_eq!(view.sphere_center, eye_point(p));
    }

    #[test]
    fn remote_eye_moves_with_the_player_relative_to_the_sphere() {
        let anchor = vec3(10.0, 2.0, -4.0);
        let sphere = vec3(-3.0, 1.0, 6.0);
        let player = sphere + vec3(0.0, 0.0, 5.0);
        assert_eq!(
            remote_eye(anchor, sphere, player),
            anchor + vec3(0.0, 0.0, 5.0)
        );
        // Stepping sideways around the sphere shifts the remote view 1:1.
        let step = vec3(1.5, 0.0, -0.5);
        assert_eq!(
            remote_eye(anchor, sphere, player + step) - remote_eye(anchor, sphere, player),
            step
        );
        let view = PortalView {
            anchor_eye: anchor,
            sphere_center: sphere,
        };
        assert_eq!(
            player + view.camera_translation(),
            remote_eye(anchor, sphere, player)
        );
    }

    #[test]
    fn hidden_while_the_player_eye_is_inside() {
        let view = PortalView::at_marker(vec3(0.0, 0.0, 0.0), IDENTITY, 0.0);
        assert!(!view.clear_of(view.sphere_center));
        assert!(!view.clear_of(view.sphere_center + vec3(RADIUS, 0.0, 0.0)));
        assert!(view.clear_of(view.sphere_center + vec3(0.0, 0.0, 3.0)));
    }

    #[test]
    fn off_screen_only_when_clearly_outside_the_view() {
        let view = PortalView::at_marker(vec3(0.0, 0.0, 0.0), IDENTITY, 0.0);
        let projection = cgmath::perspective(Deg(90.0), 1.0, 0.1, 100.0);
        // Camera 6 units from the sphere: looking at it, then away from it.
        let eye = view.sphere_center + vec3(0.0, 0.0, 6.0);
        let toward = Matrix4::look_to_rh(
            cgmath::Point3::from_vec(eye),
            vec3(0.0, 0.0, -1.0),
            Vector3::unit_y(),
        );
        let away = Matrix4::look_to_rh(
            cgmath::Point3::from_vec(eye),
            vec3(0.0, 0.0, 1.0),
            Vector3::unit_y(),
        );
        assert!(view.cull(toward, projection).is_some());
        assert!(view.cull(away, projection).is_none());
        // Just past the 45 degree edge: the slack keeps it.
        let side = Matrix4::look_to_rh(
            cgmath::Point3::from_vec(eye),
            Quaternion::from_angle_y(Deg(-60.0)).rotate_vector(vec3(0.0, 0.0, -1.0)),
            Vector3::unit_y(),
        );
        assert!(view.focus().rect(side, projection).is_none());
        assert!(view.cull(side, projection).is_some());
    }

    #[test]
    fn sphere_hangs_at_the_marker_eye() {
        let marker = vec3(3.0, -2.0, 7.0);
        let view = PortalView::at_marker(marker, IDENTITY, 0.0);
        let (sphere, pass) = PsiPortal::default().render(view, &[], PortalCull::default());
        assert_eq!(sphere.get_transform().w.truncate(), eye_point(marker));
        assert!(eye_point(marker).y > marker.y);
        assert_eq!(pass.camera_translation, vec3(0.0, 0.0, 0.0));
    }
}
