use std::{collections::HashMap, io, rc::Rc};

use cgmath::Vector3;

use super::{Cell, Plane};
use crate::ss2_common::*;

pub type BspNodeId = u32;

#[derive(Debug)]
pub struct BspTree {
    root_node: Rc<BspNode>,
}

#[derive(Debug)]
pub enum BspNode {
    Split {
        cell_idx: i32,
        plane: Plane,
        front: Option<Rc<BspNode>>,
        back: Option<Rc<BspNode>>,
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
        Self::cell_from_position_recursive(self.root_node.clone(), position)
    }

    fn cell_from_position_recursive(node: Rc<BspNode>, position: Vector3<f32>) -> Option<u32> {
        match node.as_ref() {
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

                if is_in_front && front.is_some() {
                    return Self::cell_from_position_recursive(front.clone().unwrap(), position);
                }

                if !is_in_front && back.is_some() {
                    return Self::cell_from_position_recursive(back.clone().unwrap(), position);
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
            root_node: Rc::new(root_node),
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
                    Some(Rc::new(Self::create_node_recursive(
                        cells,
                        raw_node_map,
                        raw_node_map.get(front).unwrap(),
                        extra_planes,
                    )))
                };

                let back_node = if *back == 0xFFFFFF {
                    None
                } else {
                    Some(Rc::new(Self::create_node_recursive(
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
