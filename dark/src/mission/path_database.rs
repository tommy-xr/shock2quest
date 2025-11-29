use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;
use crate::ss2_common::{read_single, read_u8};
use byteorder::ReadBytesExt;
use cgmath::Vector3;
use std::io;
use std::io::SeekFrom;
use tracing::{debug, warn};

bitflags::bitflags! {
    /// Flags for path cells indicating traversal properties
    pub struct PathCellFlags: u32 {
        const UNPATHABLE = 0x01;
        const BELOW_DOOR = 0x02;
        const BLOCKING_OBB = 0x04;
        const MOVING_TERRAIN = 0x08;
    }
}

bitflags::bitflags! {
    /// Movement type bits indicating who can traverse a link
    pub struct MovementBits: u32 {
        const WALK = 0x01;
        const FLY = 0x02;
        const SWIM = 0x04;
        const SMALL_CREATURE = 0x08;
    }
}

/// A convex floor polygon for AI navigation
#[derive(Debug, Clone)]
pub struct PathCell {
    pub id: u32,
    pub center: Vector3<f32>,     // Cached center point
    pub vertex_indices: Vec<u32>, // Indices into vertices array
    pub flags: PathCellFlags,     // Unpathable, below-door, etc.
}

/// Link between two path cells
#[derive(Debug, Clone)]
pub struct PathCellLink {
    pub from_cell: u32,
    pub to_cell: u32,
    pub edge_vertex_a: u32,    // Shared edge start
    pub edge_vertex_b: u32,    // Shared edge end
    pub ok_bits: MovementBits, // Who can traverse
    pub cost: u8,              // Traversal cost
}

/// Complete path database loaded from AIPATH chunk
#[derive(Debug, Clone)]
pub struct PathDatabase {
    pub cells: Vec<PathCell>,
    pub vertices: Vec<Vector3<f32>>,
    pub links: Vec<PathCellLink>,
}

impl PathDatabase {
    /// Read the AIPATH chunk to load pathfinding data
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> Option<PathDatabase> {
        let aipath_chunk = table_of_contents.get_chunk("AIPATH".to_string());

        if aipath_chunk.is_none() {
            debug!("No AIPATH chunk found in mission file");
            return None;
        }

        let aipath_chunk = aipath_chunk.unwrap();
        reader.seek(SeekFrom::Start(aipath_chunk.offset)).unwrap();

        // Read pathfinding initialization flag
        let pathfind_inited = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        debug!("Pathfinding initialized: {}", pathfind_inited);

        if pathfind_inited == 0 {
            debug!("Pathfinding not initialized");
            return None;
        }

        // Skip unknown data - based on hex analysis, second value varies
        let unknown = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        debug!("Second value (unknown): {}", unknown);

        // Read number of cells (this should be at offset 8, value 4694 from hex dump)
        // According to Dark Engine source: reads m_nCells + 1
        let m_n_cells_raw = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let num_cells = m_n_cells_raw + 1;
        debug!(
            "Raw m_nCells: {}, actual cells: {} (m_nCells + 1)",
            m_n_cells_raw, num_cells
        );

        if num_cells > 50000 {
            warn!("Cell count {} seems unreasonably large", num_cells);
            return None;
        }

        // Read cell data (32 bytes per cell according to sAIPathCell structure)
        let mut cells = Vec::new();
        let mut cell_link_info = Vec::new(); // Store (first_cell, cell_count) for each cell
        let mut cell_vertex_info = Vec::new(); // Store (first_vertex, vertex_count) for each cell

        for i in 0..num_cells {
            // Parse sAIPathCell structure (32 bytes total)
            let first_vertex = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 0
            let first_cell = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 2
            let _plane = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 4
            let _next = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 6
            let _best_neighbor = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 8
            let _link_from_neighbor = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 10

            let vertex_count = read_u8(reader); // offset 12
            let path_flags = read_u8(reader); // offset 13
            let cell_count = read_u8(reader); // offset 14
            let _wrap_flags = read_u8(reader); // offset 15

            // Read center point (cMxsVector - 12 bytes: 3 floats)
            let center_x = read_single(reader); // offset 16
            let center_y = read_single(reader); // offset 20
            let center_z = read_single(reader); // offset 24

            // Read bitfields (4 bytes total, offsets 28-31)
            let _bitfield_data = reader.read_u32::<byteorder::LittleEndian>().unwrap();

            let center = Vector3::new(center_x, center_y, center_z);
            let flags = PathCellFlags::from_bits_truncate(path_flags as u32);

            // Store the link and vertex range information for this cell
            cell_link_info.push((first_cell as u32, cell_count as u32));
            cell_vertex_info.push((first_vertex as u32, vertex_count as u32));

            cells.push(PathCell {
                id: i,
                center,
                vertex_indices: Vec::new(), // Will populate using first_vertex and vertex_count
                flags,
            });

            if i < 5 {
                debug!(
                    "Cell {}: center=({:.2}, {:.2}, {:.2}) firstVertex={} vertexCount={} firstCell={} cellCount={}",
                    i,
                    center_x,
                    center_y,
                    center_z,
                    first_vertex,
                    vertex_count,
                    first_cell,
                    cell_count
                );
            }
        }

        // Read number of planes and skip plane data
        let current_pos = reader.seek(SeekFrom::Current(0)).unwrap();
        debug!(
            "File position after reading {} cells: {}",
            num_cells, current_pos
        );

        // According to Dark Engine source: reads m_nPlanes + 1 (similar to cells)
        let m_n_planes_raw = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let num_planes = m_n_planes_raw + 1;
        debug!(
            "Raw m_nPlanes: {}, actual planes: {} (m_nPlanes + 1)",
            m_n_planes_raw, num_planes
        );

        if num_planes > 50000 {
            warn!("Plane count {} seems unreasonably large", num_planes);
            return None;
        }

        // Skip plane data (16 bytes per plane, according to sAIPathCellPlane)
        reader
            .seek(SeekFrom::Current((num_planes * 16) as i64))
            .unwrap();

        // Read number of vertices
        // According to Dark Engine source: likely also reads m_nVertices + 1
        let m_n_vertices_raw = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let num_vertices = m_n_vertices_raw + 1;
        debug!(
            "Raw m_nVertices: {}, actual vertices: {} (m_nVertices + 1)",
            m_n_vertices_raw, num_vertices
        );

        if num_vertices > 100000 {
            warn!("Vertex count {} seems unreasonably large", num_vertices);
            return None;
        }

        // Read vertex data (16 bytes per vertex: 3 floats + 1 u32)
        let mut vertices = Vec::new();
        for i in 0..num_vertices {
            let x = read_single(reader);
            let y = read_single(reader);
            let z = read_single(reader);
            let _pt_info = reader.read_u32::<byteorder::LittleEndian>().unwrap();

            vertices.push(Vector3::new(x, y, z));

            if i < 10 {
                debug!("Vertex {}: ({:.2}, {:.2}, {:.2})", i, x, y, z);
            }
        }

        // Read Links array (sAIPathCellLink)
        let m_n_links_raw = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let num_links = m_n_links_raw + 1;
        debug!(
            "Raw m_nLinks: {}, actual links: {} (m_nLinks + 1)",
            m_n_links_raw, num_links
        );

        let mut links = Vec::new();
        for i in 0..num_links {
            // Parse sAIPathCellLink structure (8 bytes total)
            let dest = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 0: destination cell
            let vertex_1 = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 2: first vertex
            let vertex_2 = reader.read_u16::<byteorder::LittleEndian>().unwrap(); // offset 4: second vertex
            let ok_bits_raw = read_u8(reader); // offset 6: movement bits
            let cost = read_u8(reader); // offset 7: traversal cost

            links.push(PathCellLink {
                from_cell: 0, // Will populate after reading all data
                to_cell: dest as u32,
                edge_vertex_a: vertex_1 as u32,
                edge_vertex_b: vertex_2 as u32,
                ok_bits: MovementBits::from_bits_truncate(ok_bits_raw as u32),
                cost,
            });

            if i < 5 {
                debug!(
                    "Link {}: -> cell {}, vertices {}:{}, cost={}, bits=0x{:02x}",
                    i, dest, vertex_1, vertex_2, cost, ok_bits_raw
                );
            }
        }

        // Read CellVertices array (sAIPathCell2VertexLink)
        let m_n_cell_vertices_raw = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let num_cell_vertices = m_n_cell_vertices_raw + 1;
        debug!(
            "Raw m_nCellVertices: {}, actual cell-vertex links: {} (m_nCellVertices + 1)",
            m_n_cell_vertices_raw, num_cell_vertices
        );

        // Read cell-vertex links (sAIPathCell2VertexLink - 4 bytes each)
        let pos_before_cell_vertices = reader.seek(SeekFrom::Current(0)).unwrap();
        let chunk_end = aipath_chunk.offset + aipath_chunk.length as u64;
        let remaining_bytes = chunk_end.saturating_sub(pos_before_cell_vertices);
        debug!(
            "Before cell-vertex links: {} bytes remaining for {} links",
            remaining_bytes, num_cell_vertices
        );

        let mut cell_vertex_links = Vec::new();
        if remaining_bytes < (num_cell_vertices * 4) as u64 {
            warn!(
                "Not enough bytes remaining for cell-vertex links: need {} but only have {}",
                num_cell_vertices * 4,
                remaining_bytes
            );
        } else {
            for i in 0..num_cell_vertices {
                let vertex_id = reader.read_u32::<byteorder::LittleEndian>().unwrap();
                cell_vertex_links.push(vertex_id);
                if i < 5 {
                    debug!("Cell-vertex link {}: vertex_id={}", i, vertex_id);
                }
            }
        }

        // Now populate the from_cell information using the cell data
        // Each cell has firstCell (index into links array) and cellCount (number of outgoing links)
        for (cell_index, (first_cell, cell_count)) in cell_link_info.iter().enumerate() {
            let start_link = *first_cell as usize;
            let end_link = start_link + (*cell_count as usize);

            // Populate from_cell for this cell's outgoing links
            for link_index in start_link..end_link.min(links.len()) {
                if link_index < links.len() {
                    links[link_index].from_cell = cell_index as u32;
                }
            }

            if cell_index < 5 && *cell_count > 0 {
                debug!(
                    "Cell {} has {} outgoing links (indices {}-{})",
                    cell_index,
                    cell_count,
                    start_link,
                    end_link.saturating_sub(1)
                );
            }
        }

        // Populate vertex_indices for each cell using first_vertex and vertex_count
        for (cell_index, (first_vertex, vertex_count)) in cell_vertex_info.iter().enumerate() {
            let start_vertex = *first_vertex as usize;
            let end_vertex = start_vertex + (*vertex_count as usize);

            // Populate vertex_indices for this cell
            for vertex_index in start_vertex..end_vertex.min(cell_vertex_links.len()) {
                if vertex_index < cell_vertex_links.len() {
                    let vertex_id = cell_vertex_links[vertex_index];
                    // Validate vertex ID is within bounds
                    if vertex_id < vertices.len() as u32 {
                        cells[cell_index].vertex_indices.push(vertex_id);
                    }
                }
            }

            if cell_index < 5 && *vertex_count > 0 {
                debug!(
                    "Cell {} has {} vertices (indices {}-{}): {:?}",
                    cell_index,
                    vertex_count,
                    start_vertex,
                    end_vertex.saturating_sub(1),
                    &cells[cell_index].vertex_indices
                );
            }
        }

        debug!(
            "AIPATH loaded: {} cells, {} vertices, {} links, {} cell-vertex links",
            cells.len(),
            vertices.len(),
            links.len(),
            num_cell_vertices
        );

        Some(PathDatabase {
            cells,
            vertices,
            links,
        })
    }

    /// Calculate the center point of a cell from its vertices
    #[allow(dead_code)] // Will be used in future phases
    fn calculate_center(vertex_indices: &[u32], vertices: &[Vector3<f32>]) -> Vector3<f32> {
        if vertex_indices.is_empty() {
            return Vector3::new(0.0, 0.0, 0.0);
        }

        let mut sum = Vector3::new(0.0, 0.0, 0.0);
        for &idx in vertex_indices {
            if let Some(vertex) = vertices.get(idx as usize) {
                sum += *vertex;
            }
        }

        sum / vertex_indices.len() as f32
    }
}
