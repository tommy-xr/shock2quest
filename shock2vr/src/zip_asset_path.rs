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
        Self::with_prefix_opts(zip_path, prefix, true, None)
    }

    /// [`with_prefix`](Self::with_prefix) with the same knobs the `.crf` mounts
    /// use, because a family's mount options have to match whichever archive it
    /// comes from.
    ///
    /// `collapse_paths` must be **false** for `strings`: 28 of the 80 string
    /// basenames exist more than once (English at `strings/foo.str`, German at
    /// `strings/German/foo.str`, plus an `rcs/` set), so collapsing lets a
    /// translated table win by archive order. The classic mount disables it for
    /// exactly this reason and the KPF mount has to as well.
    pub fn with_prefix_opts(
        zip_path: String,
        prefix: &str,
        collapse_paths: bool,
        namespace: Option<&str>,
    ) -> Box<ZipAssetPath> {
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
            let base = relative.rsplit('/').next().unwrap_or(relative);
            if collapse_paths {
                asset_to_path
                    .entry(base.to_owned())
                    .or_insert_with(|| full.clone());
            }
            if let Some(namespace) = namespace {
                asset_to_path
                    .entry(format!("{namespace}/{base}"))
                    .or_insert(full);
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

#[cfg(test)]
mod tests {
    use super::*;
    use engine::assets::asset_paths::AssetPath;

    /// `res/iface.crf` bundles its own `fonts/` subfolder sharing most
    /// basenames with `res/fonts.crf` (the canonical font family) - almost all
    /// byte-identical, but its `MAINFONT.FON`/`MAINAA.FON` are stripped copies
    /// missing glyph data for `%` and `&` (a 1px-wide blank cell instead of the
    /// real bitmap - shock2quest issue #778, the Research MFD's "Research: 5%"
    /// progress line rendering as "Research: 5"). `fonts.crf` must be mounted
    /// ahead of `iface.crf` (as `Game::init` does) so the bare "mainfont.fon"
    /// lookup resolves to the complete font, not the archive that merely
    /// happens to also carry a `fonts/` subfolder.
    #[test]
    fn fonts_crf_outranks_iface_crfs_bundled_fonts_folder() {
        let root = std::env::var("DARK_ASSET_PATH").unwrap_or_else(|_| "../Data".to_owned());
        let fonts_crf = format!("{root}/res/fonts.crf");
        let iface_crf = format!("{root}/res/iface.crf");
        if !std::path::Path::new(&fonts_crf).exists() || !std::path::Path::new(&iface_crf).exists()
        {
            eprintln!("skipping: res/fonts.crf or res/iface.crf not found under {root}");
            return;
        }

        // Same relative order as the `Game::init` mount list: fonts.crf first.
        let mounts = AssetPath::combine(vec![
            ZipAssetPath::new(fonts_crf.clone()),
            ZipAssetPath::with_namespace(iface_crf, "iface"),
        ]);
        let reader = mounts
            .get_reader(String::new(), "mainfont.fon".to_owned())
            .expect("mainfont.fon should resolve from either mount");
        let mut bytes = Vec::new();
        reader.borrow_mut().read_to_end(&mut bytes).unwrap();

        // fonts.crf's MAINFONT.FON is 1737 bytes; iface.crf's stripped copy is
        // 1715. Confirms the resolved bytes came from fonts.crf, not iface.crf.
        let expected = std::fs::read(&fonts_crf)
            .ok()
            .and_then(|zip_bytes| {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).ok()?;
                let mut file = archive.by_name("MAINFONT.FON").ok()?;
                let mut out = Vec::new();
                file.read_to_end(&mut out).ok()?;
                Some(out)
            })
            .expect("fonts.crf should contain MAINFONT.FON directly");
        assert_eq!(
            bytes, expected,
            "mainfont.fon must resolve to fonts.crf's complete copy, not iface.crf's stripped one"
        );
    }
}
