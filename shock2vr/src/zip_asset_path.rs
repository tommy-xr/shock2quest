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
        Self::build(zip_path, true, None)
    }

    pub fn new2(zip_path: String, collapse_paths: bool) -> Box<ZipAssetPath> {
        Self::build(zip_path, collapse_paths, None)
    }

    /// Mount like [`ZipAssetPath::new`], but *additionally* register every entry
    /// under an archive-qualified key `"<namespace>/<name>"`. The plain keys are
    /// unchanged, so existing lookups are untouched; the qualified keys let a
    /// caller pin an asset to this archive when several mounted archives share a
    /// basename (e.g. `"iface/log.pcx"` selects iface.crf's 188x296 MFD frame
    /// rather than obj.crf's 64x64 model texture, both named `LOG.PCX`).
    pub fn with_namespace(zip_path: String, namespace: &str) -> Box<ZipAssetPath> {
        Self::build(zip_path, true, Some(namespace))
    }

    /// Mount only the entries under `prefix`, keyed by the path *relative to*
    /// it (plus the bare basename).
    ///
    /// A `.crf` holds one resource family at its root, so `obj.crf` yields keys
    /// like `txt16/foo.pcx`. The 25th Anniversary Edition packs every family into
    /// one KPF instead (`data/res/obj/txt16/foo.pcx` in the base archive,
    /// `obj/txt16/foo.dds` in a mod layer), so a whole-archive mount would never
    /// produce those keys. Mounting per family with the prefix stripped makes a
    /// KPF behave exactly like the `.crf` it replaces - which is what lets the
    /// same family-ordered mount list serve both installs.
    pub fn with_prefix(zip_path: String, prefix: &str) -> Box<ZipAssetPath> {
        let file = File::open(&zip_path)
            .unwrap_or_else(|e| panic!("failed to open archive {zip_path}: {e}"));
        let mut archive = zip::ZipArchive::new(BufReader::new(file)).unwrap();
        let prefix = prefix.to_ascii_lowercase();
        let mut asset_to_path = HashMap::new();
        for i in 0..archive.len() {
            let file = archive.by_index(i).unwrap();
            if file.name().ends_with('/') {
                continue;
            }
            let Some(outpath) = file.enclosed_name() else {
                continue;
            };
            let full = outpath.to_str().unwrap_or_default().to_string();
            let lower = full.to_ascii_lowercase();
            // Archives are inconsistent about case (patch_ext uses `OBJ/`), so
            // match the prefix case-insensitively.
            let Some(relative) = lower.strip_prefix(&prefix) else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            asset_to_path.insert(relative.to_owned(), full.clone());
            if let Some(base) = relative.rsplit('/').next() {
                asset_to_path.entry(base.to_owned()).or_insert(full);
            }
        }
        Box::new(ZipAssetPath {
            archive: Mutex::new(archive),
            asset_to_path,
        })
    }

    fn build(zip_path: String, collapse_paths: bool, namespace: Option<&str>) -> Box<ZipAssetPath> {
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
                    asset_to_path.insert(
                        just_file_name.clone(),
                        outpath.to_str().unwrap().to_string(),
                    );
                }
                if let Some(namespace) = namespace {
                    asset_to_path.insert(
                        format!("{}/{}", namespace, just_file_name),
                        outpath.to_str().unwrap().to_string(),
                    );
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
