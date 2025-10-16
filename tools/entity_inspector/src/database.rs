use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use dark::ss2_entity_info::{merge_with_gamesys, SystemShock2EntityInfo};

use crate::error::{EntityInspectorError, Result};

pub struct EntityDatabase {
    pub entity_info: SystemShock2EntityInfo,
    pub file_type: FileType,
    pub file_path: String,
}

#[derive(Debug, Clone)]
pub enum FileType {
    Gamesys,
    Mission,
}

impl EntityDatabase {
    /// Load from a gamesys file (.gam)
    pub fn from_gamesys<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(EntityInspectorError::FileNotFound {
                path: path.to_path_buf(),
            });
        }

        let file = File::open(path).map_err(EntityInspectorError::IoError)?;
        let mut reader = BufReader::new(file);

        let (properties, links, links_with_data) = dark::properties::get();

        let gamesys = dark::gamesys::read(&mut reader, &links, &links_with_data, &properties);

        Ok(EntityDatabase {
            entity_info: gamesys.entity_info,
            file_type: FileType::Gamesys,
            file_path: path.display().to_string(),
        })
    }

    /// Load from a mission file (.mis) merged with gamesys
    pub fn from_mission_with_gamesys<P: AsRef<Path>>(
        mission_path: P,
        gamesys_path: P,
    ) -> Result<Self> {
        let mission_path = mission_path.as_ref();
        let gamesys_path = gamesys_path.as_ref();

        if !mission_path.exists() {
            return Err(EntityInspectorError::FileNotFound {
                path: mission_path.to_path_buf(),
            });
        }

        if !gamesys_path.exists() {
            return Err(EntityInspectorError::FileNotFound {
                path: gamesys_path.to_path_buf(),
            });
        }

        // Load gamesys first
        let gamesys_file = File::open(gamesys_path).map_err(EntityInspectorError::IoError)?;
        let mut gamesys_reader = BufReader::new(gamesys_file);

        let (properties, links, links_with_data) = dark::properties::get();

        let gamesys = dark::gamesys::read(&mut gamesys_reader, &links, &links_with_data, &properties);

        // For now, just use a minimal asset cache - this is a basic implementation
        // In a full implementation we would need proper asset cache setup
        let mut dummy_asset_cache = engine::assets::asset_cache::AssetCache::new(
            "/tmp".to_string(),
            engine::assets::asset_paths::AssetPath::folder("/tmp".to_string())
        );

        // Load mission file
        let mission_file = File::open(mission_path).map_err(EntityInspectorError::IoError)?;
        let mut mission_reader = BufReader::new(mission_file);

        let mission_data = dark::mission::read(
            &mut dummy_asset_cache,
            &mut mission_reader,
            &gamesys,
            &links,
            &links_with_data,
            &properties,
        );

        // Merge mission with gamesys
        let merged_entity_info = merge_with_gamesys(&mission_data.entity_info, &gamesys);

        Ok(EntityDatabase {
            entity_info: merged_entity_info,
            file_type: FileType::Mission,
            file_path: mission_path.display().to_string(),
        })
    }

    /// Auto-detect file type and load appropriately
    /// For mission files, this requires gamesys to be in the same directory or parent directory
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        match extension.to_lowercase().as_str() {
            "gam" => Self::from_gamesys(path),
            "mis" => {
                // Try to find gamesys in same directory or parent
                let gamesys_path = Self::find_gamesys_for_mission(path)?;
                Self::from_mission_with_gamesys(path, &gamesys_path)
            }
            _ => Err(EntityInspectorError::ParseError {
                message: format!("Unsupported file extension: {}", extension),
            }),
        }
    }

    /// Find gamesys file for a mission file
    fn find_gamesys_for_mission<P: AsRef<Path>>(mission_path: P) -> Result<std::path::PathBuf> {
        let mission_path = mission_path.as_ref();
        let parent = mission_path.parent().unwrap_or(mission_path);

        // Common gamesys names
        let gamesys_names = ["shock2.gam", "gamesys.gam", "SHOCK2.GAM", "GAMESYS.GAM"];

        for name in &gamesys_names {
            let gamesys_path = parent.join(name);
            if gamesys_path.exists() {
                return Ok(gamesys_path);
            }

            // Try parent directory
            if let Some(grandparent) = parent.parent() {
                let gamesys_path = grandparent.join(name);
                if gamesys_path.exists() {
                    return Ok(gamesys_path);
                }
            }
        }

        Err(EntityInspectorError::FileNotFound {
            path: parent.join("shock2.gam"),
        })
    }

    /// Get entity by template ID
    pub fn get_entity_by_id(&self, template_id: i32) -> Option<&Vec<std::rc::Rc<Box<dyn dark::properties::Property>>>> {
        self.entity_info.entity_to_properties.get(&template_id)
    }

    /// Find entities by name (requires PropSymName property)
    pub fn find_entities_by_name(&self, name: &str) -> Vec<i32> {
        // This is a simplified implementation
        // In a real implementation, we'd need to examine PropSymName properties
        // For now, we'll return an empty vec as this requires more complex property inspection
        vec![]
    }

    /// Get all template IDs
    pub fn get_all_template_ids(&self) -> Vec<i32> {
        self.entity_info.entity_to_properties.keys().copied().collect()
    }

    /// Get entity count
    pub fn entity_count(&self) -> usize {
        self.entity_info.entity_to_properties.len()
    }
}