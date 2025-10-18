use std::{
    cell::RefCell,
    fs::File,
    io::{self, BufReader},
    path::Path,
};

use tracing::{debug, trace};

// Asset Paths

pub trait ReadableAndSeekable: io::Read + io::Seek + Send + Sync + 'static {}

impl<T> ReadableAndSeekable for T where T: io::Read + io::Seek + Send + Sync + 'static {}

pub trait AbstractAssetPath: Sync + Send {
    fn exists(&self, base_path: String, asset_name: String) -> bool;

    fn get_reader(
        &self,
        base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>>;
}

struct MultipleAssetPaths {
    asset_paths: Vec<Box<dyn AbstractAssetPath>>,
}

impl AbstractAssetPath for MultipleAssetPaths {
    fn exists(&self, base_path: String, asset_name: String) -> bool {
        for asset_path in &self.asset_paths {
            if asset_path.exists(base_path.to_owned(), asset_name.to_owned()) {
                return true;
            }
        }

        false
    }

    fn get_reader(
        &self,
        base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
        debug!("Trying to get reader for {}", asset_name);
        for asset_path in &self.asset_paths {
            if !asset_path.exists(base_path.to_owned(), asset_name.to_owned()) {
                //println!("Not found for asset_path: {:?}", asset_path);
                continue;
            }

            let reader = asset_path.get_reader(base_path.to_owned(), asset_name.to_owned());
            if reader.is_some() {
                return reader;
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct AssetPath {
    folder_name: String,
}

impl AbstractAssetPath for AssetPath {
    fn exists(&self, base_path: String, asset_name: String) -> bool {
        let path = base_path.to_owned()
            + "/"
            + &self.folder_name.to_owned()
            + "/"
            + &asset_name.to_string();
        let exists = Path::new(&path).exists();
        trace!("Checking exists [{}]:{}", path, exists);
        exists
    }

    fn get_reader(
        &self,
        base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
        let path = base_path.to_owned()
            + "/"
            + &self.folder_name.to_owned()
            + "/"
            + &asset_name.to_string();
        trace!(" -- reading from path: {}", path);

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        Some(RefCell::new(Box::new(reader)))
    }
}

impl AssetPath {
    pub fn combine(asset_paths: Vec<Box<dyn AbstractAssetPath>>) -> Box<dyn AbstractAssetPath> {
        Box::new(MultipleAssetPaths { asset_paths })
    }

    pub fn folder(folder_name: String) -> Box<dyn AbstractAssetPath> {
        Box::new(AssetPath { folder_name })
    }
}
