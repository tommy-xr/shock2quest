use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{BufReader, Cursor, Read},
    sync::Mutex,
};

use engine::assets::asset_paths::{AbstractAssetPath, ReadableAndSeekable};
use zip::ZipArchive;

pub struct ZipAssetPath {
    archive: Mutex<ZipArchive<BufReader<File>>>,
    asset_to_path: HashMap<String, String>,
}

impl ZipAssetPath {
    pub fn new(zip_path: String) -> Box<ZipAssetPath> {
        Self::new2(zip_path, true)
    }

    pub fn new2(zip_path: String, collapse_paths: bool) -> Box<ZipAssetPath> {
        let file = File::open(zip_path).unwrap();
        let reader = BufReader::new(file);

        let mut archive = zip::ZipArchive::new(reader).unwrap();
        let mut asset_to_path = HashMap::new();
        for i in 0..archive.len() {
            let file = archive.by_index(i).unwrap();
            let outpath = match file.enclosed_name() {
                Some(path) => path,
                None => {
                    // println!("Entry {} has a suspicious path", file.name());
                    continue;
                }
            };

            if !(*file.name()).ends_with('/') {
                let just_file_name = outpath
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_ascii_lowercase();

                asset_to_path.insert(
                    outpath.to_str().unwrap().to_ascii_lowercase(),
                    outpath.to_str().unwrap().to_string(),
                );
                if collapse_paths {
                    asset_to_path.insert(just_file_name, outpath.to_str().unwrap().to_string());
                }
            }
        }
        Box::new(ZipAssetPath {
            archive: Mutex::new(archive),
            asset_to_path,
        })
    }
}

impl AbstractAssetPath for ZipAssetPath {
    fn exists(&self, _base_path: String, asset_name: String) -> bool {
        self.asset_to_path.contains_key(&asset_name)
    }

    fn get_reader(
        &self,
        _base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
        let full_name = self.asset_to_path.get(&asset_name).unwrap();
        let mut archive = self.archive.lock().unwrap();
        let mut file = archive.by_name(full_name).unwrap();

        let mut file_contents = Vec::new();
        file.read_to_end(&mut file_contents).unwrap();
        Some(RefCell::new(Box::new(Cursor::new(file_contents))))
    }
}
