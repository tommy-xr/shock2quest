use cgmath::{Matrix4, Vector3, vec3};
use engine::scene::{SceneObject, VertexPosition};

use super::ArcTrajectory;

#[derive(Clone, Copy)]
pub struct ArcRenderConfig {
    pub landing_scale: Vector3<f32>,
    pub landing_height_offset: f32,
}

impl Default for ArcRenderConfig {
    fn default() -> Self {
        Self {
            landing_scale: vec3(0.3, 0.02, 0.3),
            landing_height_offset: 0.02,
        }
    }
}

pub struct ArcRenderer;

impl ArcRenderer {
    /// Create a line mesh matching the arc trajectory for quick visualization.
    pub fn create_arc_lines(
        trajectory: &ArcTrajectory,
        color: Vector3<f32>,
    ) -> Option<SceneObject> {
        if trajectory.points.len() < 2 {
            return None;
        }

        let mut vertices = Vec::with_capacity(trajectory.points.len().saturating_sub(1) * 2);

        for pair in trajectory.points.windows(2) {
            vertices.push(VertexPosition { position: pair[0] });
            vertices.push(VertexPosition { position: pair[1] });
        }

        if vertices.len() < 2 {
            return None;
        }

        let material = engine::scene::color_material::create(color);
        let mesh = engine::scene::lines_mesh::create(vertices);
        let mut arc = SceneObject::new(material, Box::new(mesh));
        arc.set_depth_write(false);
        Some(arc)
    }

    /// Create a small landing indicator so players can see the destination.
    pub fn create_target_indicator(
        position: Vector3<f32>,
        color: Vector3<f32>,
        config: ArcRenderConfig,
    ) -> SceneObject {
        let mut target = SceneObject::new(
            engine::scene::color_material::create(color),
            Box::new(engine::scene::cube::create()),
        );

        let translation =
            Matrix4::from_translation(position + vec3(0.0, config.landing_height_offset, 0.0));
        let scale = Matrix4::from_nonuniform_scale(
            config.landing_scale.x,
            config.landing_scale.y,
            config.landing_scale.z,
        );
        target.set_transform(translation * scale);
        target.set_depth_write(false);

        target
    }
}
