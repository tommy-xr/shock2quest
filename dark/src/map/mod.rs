use std::io::{self, Read};
use std::path::Path;

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
    /// Load map chunk data for a mission from interface directory
    pub fn load_from_mission<P: AsRef<Path>>(data_path: P, mission_name: &str) -> io::Result<Self> {
        let interface_path = data_path
            .as_ref()
            .join("res")
            .join("intrface")
            .join(mission_name.to_uppercase())
            .join("english");

        let revealed_path = interface_path.join("P001RA.BIN");
        let explored_path = interface_path.join("P001XA.BIN");

        let revealed_rects = Self::load_rect_file(&revealed_path)?;
        let explored_rects = Self::load_rect_file(&explored_path)?;

        Ok(MapChunkData {
            mission_name: mission_name.to_string(),
            revealed_rects,
            explored_rects,
        })
    }

    /// Load rectangle data from a BIN file
    fn load_rect_file<P: AsRef<Path>>(path: P) -> io::Result<Vec<MapRect>> {
        let mut file = std::fs::File::open(&path)?;
        let mut rects = Vec::new();

        loop {
            let mut buffer = [0u8; 8]; // 4 x 2-byte coordinates
            match file.read_exact(&mut buffer) {
                Ok(_) => {
                    // Parse as little-endian 16-bit integers
                    let ul_x = i16::from_le_bytes([buffer[0], buffer[1]]);
                    let ul_y = i16::from_le_bytes([buffer[2], buffer[3]]);
                    let lr_x = i16::from_le_bytes([buffer[4], buffer[5]]);
                    let lr_y = i16::from_le_bytes([buffer[6], buffer[7]]);

                    rects.push(MapRect::new(ul_x, ul_y, lr_x, lr_y));
                }
                Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }

        Ok(rects)
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
