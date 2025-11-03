use cgmath::Vector3;
use dark::mission::{BspTree, Cell, SystemShock2Level};

/// Spatial query interface for level data
/// Provides position-based lookups without requiring the full SystemShock2Level
pub trait SpatialQueryEngine {
    /// Get the cell index from a world position using BSP tree traversal
    fn get_cell_idx_from_position(&self, position: Vector3<f32>) -> Option<u32>;

    /// Get a cell reference from a world position
    fn get_cell_from_position(&self, position: Vector3<f32>) -> Option<&Cell>;

    /// Get the total number of cells
    fn get_cell_count(&self) -> usize;

    /// Get a cell by its index
    fn get_cell_by_index(&self, index: usize) -> Option<&Cell>;
}

/// Lightweight spatial data structure extracted from SystemShock2Level
/// Contains only the data needed for spatial queries and visibility calculations
pub struct LevelSpatialData {
    pub cells: Vec<Cell>,
    pub bsp_tree: BspTree,
}

impl SpatialQueryEngine for LevelSpatialData {
    fn get_cell_idx_from_position(&self, position: Vector3<f32>) -> Option<u32> {
        self.bsp_tree.cell_from_position(position)
    }

    fn get_cell_from_position(&self, position: Vector3<f32>) -> Option<&Cell> {
        let idx = self.get_cell_idx_from_position(position)?;
        self.cells.get(idx as usize)
    }

    fn get_cell_count(&self) -> usize {
        self.cells.len()
    }

    fn get_cell_by_index(&self, index: usize) -> Option<&Cell> {
        self.cells.get(index)
    }
}

impl LevelSpatialData {
    /// Create spatial data from a SystemShock2Level by extracting only the required components
    pub fn from_level(level: &SystemShock2Level) -> Self {
        Self {
            cells: level.cells.clone(),
            bsp_tree: level.bsp_tree.clone(),
        }
    }
}

/// Implementation of SpatialQueryEngine for SystemShock2Level
/// This allows existing code to continue working during the transition
impl SpatialQueryEngine for SystemShock2Level {
    fn get_cell_idx_from_position(&self, position: Vector3<f32>) -> Option<u32> {
        self.get_cell_idx_from_position(position)
    }

    fn get_cell_from_position(&self, position: Vector3<f32>) -> Option<&Cell> {
        self.get_cell_from_position(position)
    }

    fn get_cell_count(&self) -> usize {
        self.cells.len()
    }

    fn get_cell_by_index(&self, index: usize) -> Option<&Cell> {
        self.cells.get(index)
    }
}
