use std::{cell::RefCell, io::Cursor};

use tracing::trace;

use super::asset_paths::{AbstractAssetPath, ReadableAndSeekable};

pub struct BundleAssetPath {
    folder_name: String,
    root_path: String,
}

impl AbstractAssetPath for BundleAssetPath {
    fn exists(&self, _base_path: String, asset_name: String) -> bool {
        let path = if self.folder_name.is_empty() {
            asset_name.clone()
        } else {
            self.folder_name.clone() + "/" + &asset_name
        };
        let full_path = std::path::Path::new(&self.root_path).join(&path);
        let exists = full_path.exists();
        trace!("Bundle asset checking exists [{}]: {}", path, exists);
        exists
    }

    fn get_reader(
        &self,
        _base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
        let path = if self.folder_name.is_empty() {
            asset_name.clone()
        } else {
            self.folder_name.clone() + "/" + &asset_name
        };
        trace!("Bundle asset reading from path: {}", path);

        let full_path = std::path::Path::new(&self.root_path).join(&path);
        trace!("Bundle asset trying to read file: {:?}", full_path);

        match std::fs::read(&full_path) {
            Ok(file_data) => {
                trace!("Bundle asset successfully read {} bytes", file_data.len());
                let cursor = Cursor::new(file_data);
                Some(RefCell::new(Box::new(cursor)))
            }
            Err(_) => {
                trace!("Bundle asset failed to read file: {:?}", full_path);
                None
            }
        }
    }
}

impl BundleAssetPath {
    pub fn new(folder_name: String, root_path: String) -> Box<dyn AbstractAssetPath> {
        Box::new(BundleAssetPath {
            folder_name,
            root_path,
        })
    }
}
