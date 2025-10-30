use std::io;

use crate::importers::MAP_POSITION_IMPORTER;
use engine::assets::asset_cache::AssetCache;

/// Rectangle coordinates for map chunks, matches Dark Engine's Rect struct
/// Format: upper-left corner (ul.x, ul.y) and lower-right corner (lr.x, lr.y)
#[derive(Debug, Clone, PartialEq)]
pub struct MapRect {
    pub ul_x: i16, // upper-left x
    pub ul_y: i16, // upper-left y
    pub lr_x: i16, // lower-right x
    pub lr_y: i16, // lower-right y
}

impl MapRect {
    /// Create a new MapRect from coordinates
    pub fn new(ul_x: i16, ul_y: i16, lr_x: i16, lr_y: i16) -> Self {
        Self {
            ul_x,
            ul_y,
            lr_x,
            lr_y,
        }
    }

    /// Get width of the rectangle
    pub fn width(&self) -> i16 {
        self.lr_x - self.ul_x
    }

    /// Get height of the rectangle
    pub fn height(&self) -> i16 {
        self.lr_y - self.ul_y
    }
}

/// Map chunk data containing rectangles for both revealed and explored overlays
#[derive(Debug)]
pub struct MapChunkData {
    pub mission_name: String,
    pub revealed_rects: Vec<MapRect>, // P001RA.BIN - bright overlays
    pub explored_rects: Vec<MapRect>, // P001XA.BIN - dimmed overlays
}

impl MapChunkData {
    /// Load map chunk data for a mission using asset cache
    pub fn load_from_mission(asset_cache: &mut AssetCache, mission_name: &str) -> io::Result<Self> {
        let revealed_path = format!("{}/english/P001RA.BIN", mission_name.to_uppercase());
        let explored_path = format!("{}/english/P001XA.BIN", mission_name.to_uppercase());

        // Load rectangle data using asset cache
        let revealed_rects =
            if let Some(rects) = asset_cache.get_opt(&MAP_POSITION_IMPORTER, &revealed_path) {
                (*rects).clone() // Convert Rc<Vec<MapRect>> to Vec<MapRect>
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Could not load revealed rects: {}", revealed_path),
                ));
            };

        let explored_rects =
            if let Some(rects) = asset_cache.get_opt(&MAP_POSITION_IMPORTER, &explored_path) {
                (*rects).clone() // Convert Rc<Vec<MapRect>> to Vec<MapRect>
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Could not load explored rects: {}", explored_path),
                ));
            };

        Ok(MapChunkData {
            mission_name: mission_name.to_string(),
            revealed_rects,
            explored_rects,
        })
    }

    /// Get the number of map chunks
    pub fn chunk_count(&self) -> usize {
        // Both revealed and explored should have the same count
        std::cmp::max(self.revealed_rects.len(), self.explored_rects.len())
    }

    /// Get rectangle for a specific chunk slot
    pub fn get_revealed_rect(&self, slot: usize) -> Option<&MapRect> {
        self.revealed_rects.get(slot)
    }

    /// Get explored rectangle for a specific chunk slot
    pub fn get_explored_rect(&self, slot: usize) -> Option<&MapRect> {
        self.explored_rects.get(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_rect_dimensions() {
        let rect = MapRect::new(10, 20, 100, 80);
        assert_eq!(rect.width(), 90);
        assert_eq!(rect.height(), 60);
    }
}
