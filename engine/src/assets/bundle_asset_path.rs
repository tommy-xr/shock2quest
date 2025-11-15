use std::{cell::RefCell, io::Cursor, sync::Arc};

use tracing::trace;

use super::asset_paths::{AbstractAssetPath, ReadableAndSeekable};
use crate::file_system::Storage;

pub struct BundleAssetPath {
    folder_name: String,
    storage: Arc<dyn Storage>,
}

impl AbstractAssetPath for BundleAssetPath {
    fn exists(&self, _base_path: String, asset_name: String) -> bool {
        let path = self.build_relative_path(asset_name);
        let bundle_fs = self.storage.bundle_filesystem();
        let exists = bundle_fs.file_exists(&path);
        trace!("Bundle asset checking exists [{}]: {}", path, exists);
        exists
    }

    fn get_reader(
        &self,
        _base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
        let path = self.build_relative_path(asset_name);
        trace!("Bundle asset reading from path: {}", path);

        let bundle_fs = self.storage.bundle_filesystem();
        if !bundle_fs.file_exists(&path) {
            trace!("Bundle asset missing from bundle: {}", path);
            return None;
        }

        let file_data = bundle_fs.open_file(&path);
        trace!("Bundle asset successfully read {} bytes", file_data.len());
        let cursor = Cursor::new(file_data);
        Some(RefCell::new(Box::new(cursor)))
    }
}

impl BundleAssetPath {
    pub fn new(folder_name: String, storage: Arc<dyn Storage>) -> Box<dyn AbstractAssetPath> {
        Box::new(BundleAssetPath {
            folder_name,
            storage,
        })
    }

    fn build_relative_path(&self, asset_name: String) -> String {
        if self.folder_name.is_empty() {
            asset_name
        } else {
            format!("{}/{}", self.folder_name, asset_name)
        }
    }
}
