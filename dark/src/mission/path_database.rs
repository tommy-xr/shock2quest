use crate::SCALE_FACTOR;
use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;
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

/// On-disk layout of the AIPATH chunk, keyed by the chunk version.
///
/// Version 2.9 (every shipped SS2 mission except shodan.mis) stores cell and
/// link IDs as packed u16s (sAIPathCell = 32 bytes, sAIPathCellLink = 8 bytes).
/// Version 3.4 (shodan.mis) widens those IDs to u32 (44-byte cells, 16-byte
/// links); planes, vertices, and cell-vertex links are unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AiPathLayout {
    PackedIds, // version 2.9
    WideIds,   // version 3.4
}

fn read_u8_opt<T: io::Read>(reader: &mut T) -> Option<u8> {
    reader.read_u8().ok()
}

fn read_u16_opt<T: io::Read>(reader: &mut T) -> Option<u16> {
    reader.read_u16::<byteorder::LittleEndian>().ok()
}

fn read_u32_opt<T: io::Read>(reader: &mut T) -> Option<u32> {
    reader.read_u32::<byteorder::LittleEndian>().ok()
}

/// Fallible version of `ss2_common::read_vec3`: same axis remap, NaNs zeroed.
fn read_vec3_opt<T: io::Read>(reader: &mut T) -> Option<Vector3<f32>> {
    let mut components = [0.0f32; 3];
    for component in &mut components {
        let v = reader.read_f32::<byteorder::LittleEndian>().ok()?;
        *component = if v.is_nan() { 0.0 } else { v };
    }
    let [neg_x, z, y] = components;
    Some(Vector3::new(-neg_x, y, z))
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
        let version = (aipath_chunk.version_major, aipath_chunk.version_minor);
        let layout = match version {
            (2, 9) => AiPathLayout::PackedIds,
            (3, 4) => AiPathLayout::WideIds,
            _ => {
                warn!(
                    "Unsupported AIPATH chunk version {}.{}; loading mission without pathfinding data",
                    version.0, version.1
                );
                return None;
            }
        };

        // Slurp the chunk into memory so a malformed file can never read past
        // the chunk boundary into unrelated data - every read below fails
        // cleanly at the chunk end instead.
        if reader.seek(SeekFrom::Start(aipath_chunk.offset)).is_err() {
            warn!("Failed to seek to AIPATH chunk");
            return None;
        }
        let mut chunk_data = vec![0u8; aipath_chunk.length as usize];
        if reader.read_exact(&mut chunk_data).is_err() {
            warn!("AIPATH chunk is truncated; loading mission without pathfinding data");
            return None;
        }
        let mut cursor = io::Cursor::new(chunk_data.as_slice());

        // Read pathfinding initialization flag
        let pathfind_inited = read_u32_opt(&mut cursor)?;
        debug!("Pathfinding initialized: {}", pathfind_inited);

        if pathfind_inited == 0 {
            debug!("Pathfinding not initialized");
            return None;
        }

        let result = Self::parse(&mut cursor, layout);
        if result.is_none() {
            warn!(
                "Malformed AIPATH chunk (version {}.{}); loading mission without pathfinding data",
                version.0, version.1
            );
        }
        result
    }

    fn parse(reader: &mut io::Cursor<&[u8]>, layout: AiPathLayout) -> Option<PathDatabase> {
        // Cell/link IDs are packed u16s in version 2.9, u32s in version 3.4
        let read_id = |reader: &mut io::Cursor<&[u8]>| -> Option<u32> {
            match layout {
                AiPathLayout::PackedIds => read_u16_opt(reader).map(u32::from),
                AiPathLayout::WideIds => read_u32_opt(reader),
            }
        };

        // Skip unknown data - based on hex analysis, second value varies
        // (the original engine reads and discards an `obsolete` int here)
        let unknown = read_u32_opt(reader)?;
        debug!("Second value (unknown): {}", unknown);

        // According to Dark Engine source: reads m_nCells + 1
        let m_n_cells_raw = read_u32_opt(reader)?;
        let num_cells = m_n_cells_raw + 1;
        debug!(
            "Raw m_nCells: {}, actual cells: {} (m_nCells + 1)",
            m_n_cells_raw, num_cells
        );

        if num_cells > 50000 {
            warn!("Cell count {} seems unreasonably large", num_cells);
            return None;
        }

        // Read cell data (sAIPathCell: 32 bytes in v2.9, 44 bytes in v3.4)
        let mut cells = Vec::new();
        let mut cell_link_info = Vec::new(); // Store (first_cell, cell_count) for each cell
        let mut cell_vertex_info = Vec::new(); // Store (first_vertex, vertex_count) for each cell

        for i in 0..num_cells {
            let first_vertex = read_id(reader)?; // index into cell-vertex link array
            let first_cell = read_id(reader)?; // index into links array
            let _plane = read_id(reader)?;
            let _next = read_id(reader)?; // A* scratch, garbage on disk
            let _best_neighbor = read_id(reader)?; // A* scratch, garbage on disk
            let _link_from_neighbor = read_id(reader)?; // A* scratch, garbage on disk

            let (vertex_count, path_flags, cell_count) = match layout {
                AiPathLayout::PackedIds => {
                    let vertex_count = read_u8_opt(reader)?;
                    let path_flags = read_u8_opt(reader)?;
                    let cell_count = read_u8_opt(reader)?;
                    let _wrap_flags = read_u8_opt(reader)?;
                    (vertex_count, path_flags, cell_count)
                }
                AiPathLayout::WideIds => {
                    // v3.4 packs the counts as (vertexCount: u16, pathFlags: u8,
                    // cellCount: u8); wrapFlags is gone
                    let vertex_count = read_u16_opt(reader)? as u8;
                    let path_flags = read_u8_opt(reader)?;
                    let cell_count = read_u8_opt(reader)?;
                    (vertex_count, path_flags, cell_count)
                }
            };

            // Read center point (cMxsVector - 12 bytes: 3 floats)
            let center = read_vec3_opt(reader)? / SCALE_FACTOR;

            // Read cell-info bitfields (4 bytes: light level, ramp/stair, etc.)
            let _bitfield_data = read_u32_opt(reader)?;
            let flags = PathCellFlags::from_bits_truncate(path_flags as u32);

            // Store the link and vertex range information for this cell
            cell_link_info.push((first_cell, cell_count as u32));
            cell_vertex_info.push((first_vertex, vertex_count as u32));

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
                    center.x,
                    center.y,
                    center.z,
                    first_vertex,
                    vertex_count,
                    first_cell,
                    cell_count
                );
            }
        }

        debug!(
            "Cursor position after reading {} cells: {}",
            num_cells,
            reader.position()
        );

        // According to Dark Engine source: reads m_nPlanes + 1 (similar to cells)
        let m_n_planes_raw = read_u32_opt(reader)?;
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
        let planes_end = reader.position() + (num_planes as u64) * 16;
        if planes_end > reader.get_ref().len() as u64 {
            warn!("Plane data extends past end of AIPATH chunk");
            return None;
        }
        reader.set_position(planes_end);

        // Read number of vertices
        // According to Dark Engine source: likely also reads m_nVertices + 1
        let m_n_vertices_raw = read_u32_opt(reader)?;
        let num_vertices = m_n_vertices_raw + 1;
        debug!(
            "Raw m_nVertices: {}, actual vertices: {} (m_nVertices + 1)",
            m_n_vertices_raw, num_vertices
        );

        if num_vertices > 100000 {
            warn!("Vertex count {} seems unreasonably large", num_vertices);
            return None;
        }

        // Read vertex data (sAIPathVertex - 16 bytes: 3 floats + 1 u32)
        let mut vertices = Vec::new();
        for i in 0..num_vertices {
            let vertex_point = read_vec3_opt(reader)? / SCALE_FACTOR;
            let _pt_info = read_u32_opt(reader)?;

            vertices.push(vertex_point);

            if i < 10 {
                debug!(
                    "Vertex {}: ({:.2}, {:.2}, {:.2})",
                    i, vertex_point.x, vertex_point.y, vertex_point.z
                );
            }
        }

        // Read Links array (sAIPathCellLink: 8 bytes in v2.9, 16 bytes in v3.4)
        let m_n_links_raw = read_u32_opt(reader)?;
        let num_links = m_n_links_raw + 1;
        debug!(
            "Raw m_nLinks: {}, actual links: {} (m_nLinks + 1)",
            m_n_links_raw, num_links
        );

        let mut links = Vec::new();
        for i in 0..num_links {
            let dest = read_id(reader)?; // destination cell
            let vertex_1 = read_id(reader)?; // first vertex of shared edge
            let vertex_2 = read_id(reader)?; // second vertex of shared edge
            let (ok_bits_raw, cost) = match layout {
                AiPathLayout::PackedIds => {
                    let ok_bits_raw = read_u8_opt(reader)?;
                    let cost = read_u8_opt(reader)?;
                    (ok_bits_raw, cost)
                }
                AiPathLayout::WideIds => {
                    // v3.4 widens okBits and cost to u16 each
                    let ok_bits_raw = read_u16_opt(reader)? as u8;
                    let cost = read_u16_opt(reader)? as u8;
                    (ok_bits_raw, cost)
                }
            };

            links.push(PathCellLink {
                from_cell: 0, // Will populate after reading all data
                to_cell: dest,
                edge_vertex_a: vertex_1,
                edge_vertex_b: vertex_2,
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
        let m_n_cell_vertices_raw = read_u32_opt(reader)?;
        let num_cell_vertices = m_n_cell_vertices_raw + 1;
        debug!(
            "Raw m_nCellVertices: {}, actual cell-vertex links: {} (m_nCellVertices + 1)",
            m_n_cell_vertices_raw, num_cell_vertices
        );

        // Read cell-vertex links (sAIPathCell2VertexLink - 4 bytes each)
        let mut cell_vertex_links = Vec::new();
        for i in 0..num_cell_vertices {
            let vertex_id = read_u32_opt(reader)?;
            cell_vertex_links.push(vertex_id);
            if i < 5 {
                debug!("Cell-vertex link {}: vertex_id={}", i, vertex_id);
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
