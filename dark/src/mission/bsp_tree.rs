use std::{collections::HashMap, io, sync::Arc};

use cgmath::Vector3;

use super::{Cell, Plane};
use crate::ss2_common::*;

pub type BspNodeId = u32;

#[derive(Debug, Clone)]
pub struct BspTree {
    root_node: Arc<BspNode>,
}

// Background level loading (projects/loading-screen.md, PR S1) parses the level on a
// worker thread, so the parse output must be `Send + Sync`. This compile-time assertion
// guards that `BspTree` stays thread-safe.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BspTree>();
};

#[derive(Debug, Clone)]
pub enum BspNode {
    Split {
        cell_idx: i32,
        plane: Plane,
        front: Option<Arc<BspNode>>,
        back: Option<Arc<BspNode>>,
    },
    Leaf {
        cell_idx: i32,
    },
}

#[derive(Clone, Debug)]
pub enum RawBspNode {
    Split {
        cell_idx: i32,
        plane_idx: u32,
        front: BspNodeId,
        back: BspNodeId,
    },
    Leaf {
        cell_idx: i32,
    },
}

use bitflags::bitflags;

bitflags! {
    pub struct BspFlags: u32 {
        const LEAF = 1 << 0;
        const UNK = 1 << 1;
        const REVERSE = 1 << 2;
    }
}

impl BspTree {
    pub fn cell_from_position(&self, position: Vector3<f32>) -> Option<u32> {
        Self::cell_from_position_recursive(&self.root_node, position)
    }

    /// Conservative cell coverage for an axis-aligned box. Testing only corners
    /// misses cells crossed by the middle of a long object. Plane intervals can
    /// admit extra leaves, but never discard a leaf overlapped by the box.
    pub fn cells_intersecting_box(
        &self,
        center: Vector3<f32>,
        half_size: Vector3<f32>,
        cells: &mut Vec<u32>,
    ) {
        cells.clear();
        Self::box_cells(&self.root_node, center, half_size.map(f32::abs), cells);
    }

    fn box_cells(node: &BspNode, center: Vector3<f32>, half: Vector3<f32>, cells: &mut Vec<u32>) {
        match node {
            BspNode::Leaf { cell_idx } => {
                if *cell_idx >= 0 {
                    cells.push(*cell_idx as u32);
                }
            }
            BspNode::Split {
                plane, front, back, ..
            } => {
                let n = plane.normal;
                let distance = n.x * center.x + n.y * center.y + n.z * center.z + plane.w;
                let radius = n.x.abs() * half.x + n.y.abs() * half.y + n.z.abs() * half.z;
                // Touching / roundoff at a plane must stay conservative. NaN or
                // infinity also visits both sides rather than hiding geometry.
                let tolerance = 8.0
                    * f32::EPSILON
                    * ((n.x * center.x).abs()
                        + (n.y * center.y).abs()
                        + (n.z * center.z).abs()
                        + plane.w.abs()
                        + radius
                        + 1.0);
                if !(distance + radius < -tolerance) {
                    if let Some(front) = front {
                        Self::box_cells(front, center, half, cells);
                    }
                }
                if !(distance - radius > tolerance) {
                    if let Some(back) = back {
                        Self::box_cells(back, center, half, cells);
                    }
                }
            }
        }
    }

    fn cell_from_position_recursive(node: &BspNode, position: Vector3<f32>) -> Option<u32> {
        match node {
            BspNode::Leaf { cell_idx } => Some(*cell_idx as u32),
            BspNode::Split {
                cell_idx: _,
                plane,
                front,
                back,
            } => {
                // let plane_position = plane.normal * plane.w;
                // let diff = position - plane_position;

                let is_in_front = plane.normal.x * position.x
                    + plane.normal.y * position.y
                    + plane.normal.z * position.z
                    + plane.w
                    >= 0.0;

                // Borrow rather than clone the Arc: this descends once per lit
                // object per frame, and an atomic refcount round-trip per node
                // is pure overhead for a read.
                if is_in_front && let Some(front) = front {
                    return Self::cell_from_position_recursive(front, position);
                }

                if !is_in_front && let Some(back) = back {
                    return Self::cell_from_position_recursive(back, position);
                }

                None
            }
        }
    }

    pub fn read<T: io::Read>(reader: &mut T, planes: &Vec<Cell>) -> BspTree {
        // Read "extra planes"
        // Most maps don't use them - but looks like at least command1.mis and command2.mis
        //
        // Necessitates special handling in the BSP tree:
        // https://github.com/volca02/openDarkEngine/blob/7a2d7baaf0fc5194a9066a635c6f44b0f7b26c56/src/services/worldrep/WorldRepService.cpp#L340
        //
        // This allows for BSP nodes that don't correspond to cells - they can just have a splitting plane.
        let num_extra_planes = read_u32(reader);
        let mut extra_planes = Vec::new();
        for _ in 0..num_extra_planes {
            let plane = Plane::read(reader);
            extra_planes.push(plane);
        }

        let num_bsp_nodes = read_u32(reader);

        // First pass: read nodes and populate dictionary -> id
        let mut raw_node_map: HashMap<u32, RawBspNode> = HashMap::new();
        let mut raw_root_node = None;

        for idx in 0..num_bsp_nodes {
            let node_header = read_u32(reader);

            // The first 4 byte are packed:
            // - 1 byte: flags
            // - 3 bytes: node_id
            let _node_id = node_header & 0x00FFFFFF;
            let flags = (node_header & 0xFF000000) >> 24;
            //let node_id = first_bits + flags;
            // let node_id = node_header & 0xFFFFFF00 >> 8;
            // let flags = (node_header & 0x000000FF);
            let normalized_flags = BspFlags::from_bits(flags).unwrap();

            let cell = read_i32(reader);
            let plane = read_u32(reader);
            let front = read_i32(reader);
            let back = read_i32(reader);

            let node = {
                if normalized_flags.contains(BspFlags::LEAF) {
                    RawBspNode::Leaf {
                        // Weird, but in the packed representaiton, the 'front'
                        // is the target cell idx for this node...
                        cell_idx: front,
                    }
                } else {
                    if normalized_flags.contains(BspFlags::REVERSE) {
                        RawBspNode::Split {
                            cell_idx: cell,
                            plane_idx: plane,
                            front: back as u32,
                            back: front as u32,
                        }
                    } else {
                        RawBspNode::Split {
                            cell_idx: cell,
                            plane_idx: plane,
                            front: front as u32,
                            back: back as u32,
                        }
                    }
                }
            };

            if raw_root_node.is_none() {
                raw_root_node = Some(node.clone());
            }
            raw_node_map.insert(idx, node.clone());
        }

        // Grab the root node
        let root_node = Self::create_node_recursive(
            planes,
            &raw_node_map,
            &raw_root_node.unwrap(),
            &extra_planes,
        );

        BspTree {
            root_node: Arc::new(root_node),
        }
    }
    fn create_node_recursive(
        cells: &Vec<Cell>,
        raw_node_map: &HashMap<u32, RawBspNode>,
        raw_node: &RawBspNode,
        extra_planes: &Vec<Plane>,
    ) -> BspNode {
        match raw_node {
            RawBspNode::Leaf { cell_idx } => BspNode::Leaf {
                cell_idx: *cell_idx,
            },
            RawBspNode::Split {
                cell_idx,
                plane_idx,
                front,
                back,
            } => {
                let front_node = if *front == 0xFFFFFF {
                    None
                } else {
                    Some(Arc::new(Self::create_node_recursive(
                        cells,
                        raw_node_map,
                        raw_node_map.get(front).unwrap(),
                        extra_planes,
                    )))
                };

                let back_node = if *back == 0xFFFFFF {
                    None
                } else {
                    Some(Arc::new(Self::create_node_recursive(
                        cells,
                        raw_node_map,
                        raw_node_map.get(back).unwrap(),
                        extra_planes,
                    )))
                };

                // Handle the extra plane - the extra plane is used if the parent node does not correspond to an extra cell.
                let plane = if *cell_idx < 0 {
                    extra_planes[*plane_idx as usize].clone()
                } else {
                    cells[*cell_idx as usize].planes[*plane_idx as usize].clone()
                };

                BspNode::Split {
                    cell_idx: *cell_idx,
                    plane,
                    front: front_node,
                    back: back_node,
                }
            }
        }
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    use cgmath::vec3;

    fn leaf(index: i32) -> Arc<BspNode> {
        Arc::new(BspNode::Leaf { cell_idx: index })
    }
    fn split(
        normal: Vector3<f32>,
        w: f32,
        front: Arc<BspNode>,
        back: Arc<BspNode>,
    ) -> Arc<BspNode> {
        Arc::new(BspNode::Split {
            cell_idx: -1,
            plane: Plane { normal, w },
            front: Some(front),
            back: Some(back),
        })
    }
    fn strip() -> BspTree {
        BspTree {
            root_node: split(
                vec3(1.0, 0.0, 0.0),
                -1.0,
                leaf(2),
                split(vec3(1.0, 0.0, 0.0), 1.0, leaf(1), leaf(0)),
            ),
        }
    }

    #[test]
    fn long_skinny_box_covers_middle_cell_even_when_corners_and_center_miss_it() {
        let tree = strip();
        let center = vec3(-8.0, 0.0, 0.0);
        let half = vec3(20.0, 0.001, 0.001);
        assert_eq!(tree.cell_from_position(center), Some(0));
        for x in [-half.x, half.x] {
            for y in [-half.y, half.y] {
                for z in [-half.z, half.z] {
                    assert_ne!(tree.cell_from_position(center + vec3(x, y, z)), Some(1));
                }
            }
        }
        let mut cells = vec![];
        tree.cells_intersecting_box(center, half, &mut cells);
        cells.sort_unstable();
        assert_eq!(cells, [0, 1, 2]);
    }

    #[test]
    fn grazing_faces_flat_bounds_and_negative_sizes_stay_conservative() {
        let tree = strip();
        for (center, half, expected) in [
            (vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), vec![1]),
            (vec3(-2.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec![0, 1]),
            (vec3(2.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0), vec![1, 2]),
            (vec3(2.0 - 1e-6, 0.0, 0.0), vec3(1.0, 1e-7, 0.0), vec![1, 2]),
            (vec3(2.01, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec![2]),
        ] {
            let mut cells = vec![999];
            tree.cells_intersecting_box(center, half, &mut cells);
            cells.sort_unstable();
            assert_eq!(cells, expected);
        }
    }

    #[test]
    fn oblique_planes_never_drop_sampled_points_inside_bounds() {
        for angle in [0.001_f32, 0.1, 0.7, 1.2, 1.5707, 2.7] {
            let n = vec3(angle.cos(), angle.sin(), 0.13);
            let tree = BspTree {
                root_node: split(
                    n,
                    -0.3,
                    leaf(2),
                    split(vec3(-0.2, 0.7, 0.9), 0.2, leaf(1), leaf(0)),
                ),
            };
            for center in [
                vec3(0.0, 0.0, 0.0),
                vec3(-1.0, 0.5, 0.2),
                vec3(100.0, -200.0, 30.0),
            ] {
                for half in [
                    vec3(20.0, 0.001, 0.001),
                    vec3(0.001, 20.0, 0.001),
                    vec3(0.2, 0.5, 0.7),
                ] {
                    let mut cells = vec![];
                    tree.cells_intersecting_box(center, half, &mut cells);
                    for x in -4..=4 {
                        for y in -4..=4 {
                            for z in -4..=4 {
                                let point = center
                                    + vec3(
                                        half.x * x as f32 / 4.0,
                                        half.y * y as f32 / 4.0,
                                        half.z * z as f32 / 4.0,
                                    );
                                if let Some(cell) = tree.cell_from_position(point) {
                                    assert!(
                                        cells.contains(&cell),
                                        "angle={angle}, point={point:?}, cells={cells:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn malformed_bounds_do_not_hide_cells_and_invalid_leaves_are_ignored() {
        let tree = strip();
        for center in [vec3(f32::NAN, 0.0, 0.0), vec3(f32::INFINITY, 0.0, 0.0)] {
            let mut cells = vec![];
            tree.cells_intersecting_box(center, vec3(1.0, 1.0, 1.0), &mut cells);
            cells.sort_unstable();
            assert_eq!(cells, [0, 1, 2]);
        }
        let tree = BspTree {
            root_node: split(vec3(1.0, 0.0, 0.0), 0.0, leaf(1), leaf(-1)),
        };
        let mut cells = vec![];
        tree.cells_intersecting_box(vec3(0.0, 0.0, 0.0), vec3(1.0, 1.0, 1.0), &mut cells);
        assert_eq!(cells, [1]);
    }
}
