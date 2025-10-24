use std::collections::{HashMap, HashSet};

use engine::render_log;

use cgmath::{point2, vec3, Matrix4, Point3, SquareMatrix, Vector3};
use collision::{Aabb2, Contains, Frustum, Relation, Union};
use dark::{
    mission::{Cell, SystemShock2Level},
    properties::{PropPosition, PropPhysDimensions},
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::util::has_refs;

use super::{CullingInfo, VisibilityEngine};

pub struct PortalDebugInfo {
    #[allow(dead_code)]
    from_cell: u32,
    #[allow(dead_code)]
    to_cell: u32,
    #[allow(dead_code)]
    screen_bbox: Aabb2<f32>,
    #[allow(dead_code)]
    vertices: Vec<Point3<f32>>,
    #[allow(dead_code)]
    is_visible: bool,
}

pub struct PortalVisibilityEngine {
    ///
    /// Cache of the cell that each entity is in
    ///
    /// If the position hasn't changed, no need to recompute the cell position
    #[allow(dead_code)]
    entity_cell_cache: HashMap<EntityId, (Vector3<f32>, Option<u32>)>,

    is_visible: HashMap<EntityId, bool>,

    debug_portals: Vec<PortalDebugInfo>,
    #[allow(dead_code)]
    is_debug: bool,
}

///
/// Calculates the intersection of two axis-aligned bounding boxes, or returns None if no intersection
///
pub fn intersects(current_viewport: &Aabb2<f32>, portal: &Aabb2<f32>) -> Option<Aabb2<f32>> {
    let intersect_min_x = current_viewport.min.x.max(portal.min.x);
    let intersect_max_x = current_viewport.max.x.min(portal.max.x);

    let intersect_min_y = current_viewport.min.y.max(portal.min.y);
    let intersect_max_y = current_viewport.max.y.min(portal.max.y);

    if intersect_min_x < intersect_max_x && intersect_min_y < intersect_max_y {
        Some(Aabb2 {
            min: point2(intersect_min_x, intersect_min_y),
            max: point2(intersect_max_x, intersect_max_y),
        })
    } else {
        None
    }
}

const MAX_DEPTH: u32 = 128;

impl PortalVisibilityEngine {
    pub fn new() -> Self {
        PortalVisibilityEngine {
            entity_cell_cache: HashMap::new(),
            is_visible: HashMap::new(),
            debug_portals: Vec::new(),
            is_debug: false,
        }
    }
    fn check_cell_recursive(
        current_screen_portal_candidate: Aabb2<f32>,
        screen_width: f32,
        screen_height: f32,
        projection_view: Matrix4<f32>,
        frustum: &Frustum<f32>,
        level: &SystemShock2Level,
        visible_cells: &mut HashSet<u32>,
        visited_cells: &mut HashMap<u32, Aabb2<f32>>,
        debug_cells: &mut Vec<PortalDebugInfo>,
        current_cell: &Cell,
        depth: u32,
    ) {
        if depth >= MAX_DEPTH {
            return;
        }

        let mut current_screen_portal = current_screen_portal_candidate;

        if visible_cells.contains(&current_cell.idx) {
            // We've visited this cell - but did it check a sufficient portal?
            let previous_portal = visited_cells.get(&current_cell.idx).unwrap();

            // If the current portal is smaller than the previous portal, we can ignore it.
            if previous_portal.contains(&current_screen_portal) {
                return;
            }

            // Otherwise, we need to check the union of the current portal and previous portal
            current_screen_portal = previous_portal.union(&current_screen_portal);
        }

        visible_cells.insert(current_cell.idx);
        visited_cells.insert(current_cell.idx, current_screen_portal);

        for portal in &current_cell.portals {
            let target_cell = &level.cells[portal.target_cell_idx as usize];

            // Do quick frustum check. If the bounding sphere for the portal is not in the frustum,
            // we can ignore.
            let frustum_check = frustum.contains(&portal.bounding_sphere);
            if frustum_check == Relation::Out {
                continue;
            }

            // HACK: Sometimes, if the cell the player is in is really skinny, there is a flicker.
            // Workaround for now is to start considering visibility in adjoining cells.
            let new_intersection = if depth > 1 {
                // Now do a more expensive check to see if the portal is actually visible on the screen.
                let portal_screen_space_quad =
                    portal.screen_space_squad(projection_view, screen_width, screen_height);

                let maybe_intersects =
                    intersects(&current_screen_portal, &portal_screen_space_quad);

                if maybe_intersects.is_none() {
                    //println!("rejecting {} -> {} - current_screen_portal: {:?}, portal_screen_space_quad: {:?}", current_cell.idx, target_cell.idx, current_screen_portal, portal_screen_space_quad);
                    // debug_cells.push(PortalDebugInfo {
                    //     from_cell: current_cell.idx,
                    //     to_cell: portal.target_cell_idx as u32,
                    //     screen_bbox: portal_screen_space_quad,
                    //     vertices: portal.all_vertices.clone(),
                    //     is_visible: false,
                    // });

                    continue;
                }

                // debug_cells.push(PortalDebugInfo {
                //     from_cell: current_cell.idx,
                //     to_cell: portal.target_cell_idx as u32,
                //     screen_bbox: portal_screen_space_quad,
                //     vertices: portal.all_vertices.clone(),
                //     is_visible: true,
                // });
                maybe_intersects.unwrap()
                // } else {
                //     current_screen_portal
            } else {
                current_screen_portal
            };

            // println!(
            //     "allowing {} -> {} - current_screen_portal: {:?}, portal_screen_space_quad: {:?}",
            //     current_cell.idx, target_cell.idx, current_screen_portal, new_intersection,
            // );

            // let new_intersection = maybe_intersects.unwrap();

            //let new_intersection = current_screen_portal;

            // TOO: Check visibility of portal
            Self::check_cell_recursive(
                new_intersection,
                screen_width,
                screen_height,
                projection_view,
                frustum,
                level,
                visible_cells,
                visited_cells,
                debug_cells,
                target_cell,
                depth + 1,
            )
        }
    }

    #[allow(dead_code)]
    fn get_cell_from_position(
        &mut self,
        level: &SystemShock2Level,
        entity_id: &EntityId,
        position: Vector3<f32>,
    ) -> Option<u32> {
        let cached_info = self.entity_cell_cache.get(entity_id);

        if cached_info.is_some() {
            let (cached_position, cached_cell) = cached_info.unwrap();
            if cached_position == &position {
                // Position hasn't changed, so we can use the cached cell
                return *cached_cell;
            }
        }

        // Calculate new position for entity, since it moved...
        let maybe_cell = level.get_cell_from_position(position);
        let maybe_cell_idx = maybe_cell.map(|c| c.idx);
        self.entity_cell_cache
            .insert(*entity_id, (position, maybe_cell_idx));
        maybe_cell_idx
    }
}
fn camera_position_from_view_matrix(view_matrix: Matrix4<f32>) -> Vector3<f32> {
    let inverse_view = view_matrix.invert().unwrap();

    // The translation part of the matrix holds the camera position
    vec3(inverse_view.w.x, inverse_view.w.y, inverse_view.w.z)
}

impl VisibilityEngine for PortalVisibilityEngine {
    fn prepare(&mut self, level: &SystemShock2Level, world: &World, culling_info: &CullingInfo) {
        self.debug_portals.clear();
        let camera_position = camera_position_from_view_matrix(culling_info.view);
        let maybe_camera_cell_idx = level.get_cell_idx_from_position(camera_position);
        let maybe_camera_cell = level.get_cell_from_position(camera_position);

        render_log!(
            DEBUG,
            "visibility engine - starting from cell: {:?}",
            maybe_camera_cell_idx
        );

        if maybe_camera_cell.is_none() {
            return;
        }

        let projection_view = culling_info.projection * culling_info.view;
        let maybe_frustum = Frustum::from_matrix4(projection_view);

        if maybe_frustum.is_none() {
            return;
        }

        let frustum = maybe_frustum.unwrap();

        let mut visible_cells = HashSet::new();
        let mut visited_cells = HashMap::new();

        let camera_cell = maybe_camera_cell.unwrap();
        let screen_portal = Aabb2::new(
            point2(0.0, 0.0),
            point2(culling_info.screen_size.x, culling_info.screen_size.y),
        );
        Self::check_cell_recursive(
            screen_portal,
            culling_info.screen_size.x,
            culling_info.screen_size.y,
            projection_view,
            &frustum,
            level,
            &mut visible_cells,
            &mut visited_cells,
            &mut self.debug_portals,
            camera_cell,
            0,
        );

        render_log!(
            DEBUG,
            "total cells: {} | visible cells: {}",
            level.cells.len(),
            visible_cells.len()
        );

        let v_prop_position = world.borrow::<View<PropPosition>>().unwrap();
        let v_prop_phys_dimensions = world.borrow::<View<PropPhysDimensions>>().unwrap();

        for (id, pos) in v_prop_position.iter().with_id() {
            if !has_refs(world, id) {
                self.is_visible.insert(id, false);
                continue;
            }

            // Check if ANY corner of the entity's bounding box is in a visible cell
            let is_entity_visible = if let Ok(dimensions) = v_prop_phys_dimensions.get(id) {
                let half_size = dimensions.size * 0.5;
                let corners = [
                    pos.position + vec3(-half_size.x, -half_size.y, -half_size.z),
                    pos.position + vec3( half_size.x, -half_size.y, -half_size.z),
                    pos.position + vec3(-half_size.x,  half_size.y, -half_size.z),
                    pos.position + vec3( half_size.x,  half_size.y, -half_size.z),
                    pos.position + vec3(-half_size.x, -half_size.y,  half_size.z),
                    pos.position + vec3( half_size.x, -half_size.y,  half_size.z),
                    pos.position + vec3(-half_size.x,  half_size.y,  half_size.z),
                    pos.position + vec3( half_size.x,  half_size.y,  half_size.z),
                ];

                corners.iter().any(|&corner| {
                    if let Some(cell_idx) = level.get_cell_idx_from_position(corner) {
                        visible_cells.contains(&cell_idx)
                    } else {
                        false
                    }
                })
            } else {
                // Fallback to position-based check for entities without physics dimensions
                if let Some(cell_idx) = level.get_cell_idx_from_position(pos.position) {
                    visible_cells.contains(&cell_idx)
                } else {
                    false
                }
            };

            self.is_visible.insert(id, is_entity_visible);
        }
    }

    fn is_visible(&mut self, entity_id: EntityId) -> bool {
        *self.is_visible.get(&entity_id).unwrap_or(&false)
    }

    fn debug_render(&self, _asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        vec![]
        // let mut debug_objs = self
        //     .debug_portals
        //     .iter()
        //     .map(|portal_debug_info| {
        //         let aabb = portal_debug_info.screen_bbox;

        //         let mut lines: Vec<Point3<f32>> = Vec::new();

        //         let mut inner_idx = 1;
        //         let vertices = &portal_debug_info.vertices;
        //         let len = portal_debug_info.vertices.len();
        //         for inner_idx in 1..(len - 1) {
        //             if inner_idx + 1 >= len {
        //                 break;
        //             }
        //             let v0 = vertices[0];
        //             let v1 = vertices[inner_idx];
        //             let v2 = vertices[inner_idx + 1];
        //             lines.push(v0);
        //             lines.push(v1);

        //             lines.push(v0);
        //             lines.push(v2);

        //             lines.push(v1);
        //             lines.push(v2);
        //         }

        //         //panic!();

        //         let line_vertices = lines
        //             .iter()
        //             .map(|v| VertexPosition {
        //                 position: vec3(v.x, v.y, v.z),
        //             })
        //             .collect();

        //         let color = if portal_debug_info.is_visible {
        //             vec3(0.0, 1.0, 0.0)
        //         } else {
        //             vec3(1.0, 0.0, 0.0)
        //         };

        //         let lines_mat = engine::scene::color_material::create(color);
        //         let debug = SceneObject::new(
        //             lines_mat,
        //             Box::new(engine::scene::lines_mesh::create(line_vertices)),
        //         );

        //         let mut debug_objs = vec![debug];

        //         if portal_debug_info.to_cell == 82 && portal_debug_info.from_cell == 84 {
        //             let options = TextureOptions { wrap: false };
        //             let highlight = asset_cache.get_ext(&TEXTURE_IMPORTER, "TURQ.GIF", &options);

        //             let position = vec2(aabb.min.x, aabb.min.y);
        //             let size = vec2(aabb.max.x - aabb.min.x, aabb.max.y - aabb.min.y);
        //             let so = SceneObject::screen_space_quad2(highlight, position, size, 0.25);
        //             debug_objs.push(so);
        //         };

        //         debug_objs
        //     })
        //     .flatten()
        //     .collect();

        // debug_objs
    }
}
