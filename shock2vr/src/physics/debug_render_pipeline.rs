use std::collections::HashMap;

use cgmath::{Vector3, vec3};
use engine::scene::{SceneObject, VertexPosition};
use ordered_float::*;
use rapier3d::prelude::*;

use super::{InternalCollisionGroups, util::*};

pub struct DebugRenderer {
    lines: HashMap<Vector3<OrderedFloat<f32>>, Vec<VertexPosition>>,
}

impl DebugRenderer {
    pub fn new() -> DebugRenderer {
        DebugRenderer {
            lines: HashMap::new(),
        }
    }

    /// Add a colored world-space line segment (for custom debug overlays such as
    /// joint anchors).
    pub fn add_line(&mut self, a: Vector3<f32>, b: Vector3<f32>, color: Vector3<f32>) {
        let key = vec3(
            OrderedFloat(color.x),
            OrderedFloat(color.y),
            OrderedFloat(color.z),
        );
        self.lines
            .entry(key)
            .or_default()
            .extend([VertexPosition { position: a }, VertexPosition { position: b }]);
    }

    /// Small 3-axis cross marker at `p`.
    pub fn add_cross(&mut self, p: Vector3<f32>, size: f32, color: Vector3<f32>) {
        self.add_line(p - vec3(size, 0.0, 0.0), p + vec3(size, 0.0, 0.0), color);
        self.add_line(p - vec3(0.0, size, 0.0), p + vec3(0.0, size, 0.0), color);
        self.add_line(p - vec3(0.0, 0.0, size), p + vec3(0.0, 0.0, size), color);
    }
    pub fn render(self) -> Vec<SceneObject> {
        let mut ret = Vec::new();
        for (color, lines) in self.lines {
            let lines_mat = engine::scene::color_material::create(Vector3::new(
                color.x.into_inner(),
                color.y.into_inner(),
                color.z.into_inner(),
            ));
            let debug = SceneObject::new(
                lines_mat,
                Box::new(engine::scene::lines_mesh::create(lines)),
            );
            ret.push(debug);
        }
        ret
    }
}

impl DebugRenderBackend for DebugRenderer {
    fn draw_line(
        &mut self,
        object: DebugRenderObject,
        a: Point<Real>,
        b: Point<Real>,
        c: [f32; 4],
    ) {
        let mut vcolor = vec3(
            OrderedFloat::from(c[0]),
            OrderedFloat::from(c[1]),
            OrderedFloat::from(c[2]),
        );
        match object {
            DebugRenderObject::Collider(_, c) => {
                // Filter out world geometry from debug renderer
                if c.parent().is_none() {
                    return;
                }

                if c.is_sensor() {
                    vcolor = vec3(
                        OrderedFloat::from(0.0),
                        OrderedFloat::from(0.0),
                        OrderedFloat::from(1.0),
                    );
                } else if c
                    .collision_groups()
                    .memberships
                    .contains(InternalCollisionGroups::SELECTABLE.bits.into())
                {
                    vcolor = vec3(
                        OrderedFloat::from(0.0),
                        OrderedFloat::from(1.0),
                        OrderedFloat::from(0.0),
                    );
                } else if c
                    .collision_groups()
                    .memberships
                    .contains(InternalCollisionGroups::ENTITY.bits.into())
                {
                    vcolor = vec3(
                        OrderedFloat::from(1.0),
                        OrderedFloat::from(0.0),
                        OrderedFloat::from(0.0),
                    );
                }
            }
            DebugRenderObject::ImpulseJoint(_, _) => {
                vcolor = vec3(
                    OrderedFloat::from(1.0),
                    OrderedFloat::from(0.75),
                    OrderedFloat::from(0.0),
                );
            }
            _ => return,
        }

        self.lines.entry(vcolor).or_default().append(&mut vec![
            VertexPosition {
                position: npoint_to_cgvec(a),
            },
            VertexPosition {
                position: npoint_to_cgvec(b),
            },
        ])
    }
}
