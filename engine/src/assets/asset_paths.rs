use std::{
    cell::RefCell,
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
};

use tracing::{debug, trace};

// Asset Paths

pub trait ReadableAndSeekable: io::Read + io::Seek + Send + Sync + 'static {}

impl<T> ReadableAndSeekable for T where T: io::Read + io::Seek + Send + Sync + 'static {}

/// One lookup key a mount can serve, plus where the bytes really live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    /// The key `exists`/`get_reader` resolve (lowercased).
    pub key: String,
    /// The archive or folder serving it.
    pub source: String,
    /// The real entry name inside the source (original case, full path).
    pub entry_name: String,
    /// True for a secondary key (collapsed basename, namespace-qualified name)
    /// pointing at the same bytes as a primary mount-relative key. Filter these
    /// out to list each file once; keep them to see every resolvable name.
    pub is_alias: bool,
}

pub trait AbstractAssetPath: Sync + Send {
    fn exists(&self, base_path: String, asset_name: String) -> bool;

    fn get_reader(
        &self,
        base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>>;

    /// Pick the first of `candidates` that exists, resolving **mount-first**:
    /// the highest-priority mount that has *any* candidate wins, and only then
    /// does candidate order break the tie within that mount.
    ///
    /// This is what lets a mod layer's upgraded encoding of a texture win over
    /// the original. Testing each candidate independently with `exists` would
    /// instead let a low-priority mount's early-ordered candidate beat a
    /// high-priority mount's later-ordered one.
    ///
    /// The default (a leaf mount) is simply first-match; `MultipleAssetPaths`
    /// overrides it to do the per-mount pass.
    fn resolve_first(&self, base_path: String, candidates: &[String]) -> Option<String> {
        candidates
            .iter()
            .find(|candidate| self.exists(base_path.clone(), (*candidate).to_string()))
            .cloned()
    }

    /// Every lookup key this mount can serve. Combined mounts concatenate in
    /// mount-priority order (a key's first occurrence is the mount a lookup
    /// serves it from); within one mount, keys are unique and unordered.
    /// Mounts that cannot cheaply enumerate (loose folders, the app bundle)
    /// report nothing - enumeration is a tooling affordance, not a lookup path.
    fn entries(&self) -> Vec<AssetEntry> {
        Vec::new()
    }
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

    fn resolve_first(&self, base_path: String, candidates: &[String]) -> Option<String> {
        // Mounts are the outer loop: a later candidate in a higher-priority mount
        // beats an earlier candidate in a lower-priority one.
        for asset_path in &self.asset_paths {
            if let Some(found) = asset_path.resolve_first(base_path.clone(), candidates) {
                return Some(found);
            }
        }
        None
    }

    fn entries(&self) -> Vec<AssetEntry> {
        self.asset_paths
            .iter()
            .flat_map(|asset_path| asset_path.entries())
            .collect()
    }
}

#[derive(Debug)]
pub struct AssetPath {
    folder_name: String,
}

impl AbstractAssetPath for AssetPath {
    fn exists(&self, base_path: String, asset_name: String) -> bool {
        let path = self.resolve_path(&base_path, &asset_name);
        let exists = path.exists();
        trace!("Checking exists [{}]:{}", path.display(), exists);
        exists
    }

    fn get_reader(
        &self,
        base_path: String,
        asset_name: String,
    ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
        let path = self.resolve_path(&base_path, &asset_name);
        trace!(" -- reading from path: {}", path.display());

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        Some(RefCell::new(Box::new(reader)))
    }
}

impl AssetPath {
    fn resolve_path(&self, base_path: &str, asset_name: &str) -> PathBuf {
        let folder = Path::new(&self.folder_name);
        if folder.is_absolute() {
            folder.join(asset_name)
        } else {
            Path::new(base_path).join(folder).join(asset_name)
        }
    }

    pub fn combine(asset_paths: Vec<Box<dyn AbstractAssetPath>>) -> Box<dyn AbstractAssetPath> {
        Box::new(MultipleAssetPaths { asset_paths })
    }

    pub fn folder(folder_name: String) -> Box<dyn AbstractAssetPath> {
        Box::new(AssetPath { folder_name })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Read,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TempAssetTree(PathBuf);

    impl TempAssetTree {
        fn new(name: &str) -> Self {
            let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "shock2quest-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempAssetTree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    /// A mount that simply owns a fixed set of names.
    struct FakeMount(Vec<&'static str>);

    impl AbstractAssetPath for FakeMount {
        fn exists(&self, _base_path: String, asset_name: String) -> bool {
            self.0.iter().any(|n| *n == asset_name)
        }

        fn get_reader(
            &self,
            _base_path: String,
            _asset_name: String,
        ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
            None
        }
    }

    fn candidates(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// The upgraded encoding lives in the higher-priority mount and must win,
    /// even though the original is an earlier-listed candidate.
    #[test]
    fn resolve_first_prefers_the_higher_priority_mount() {
        let paths = AssetPath::combine(vec![
            Box::new(FakeMount(vec!["txt16/foo.dds"])),
            Box::new(FakeMount(vec!["txt16/foo.pcx"])),
        ]);
        assert_eq!(
            paths.resolve_first(
                String::new(),
                &candidates(&["txt16/foo.pcx", "txt16/foo.dds"])
            ),
            Some("txt16/foo.dds".to_owned())
        );
    }

    /// Within a single mount, candidate order decides.
    #[test]
    fn resolve_first_uses_candidate_order_inside_one_mount() {
        let paths = AssetPath::combine(vec![Box::new(FakeMount(vec![
            "txt16/foo.dds",
            "txt16/foo.pcx",
        ]))]);
        assert_eq!(
            paths.resolve_first(
                String::new(),
                &candidates(&["txt16/foo.dds", "txt16/foo.pcx"])
            ),
            Some("txt16/foo.dds".to_owned())
        );
    }

    /// A lower-priority mount is still consulted when the higher one has nothing.
    #[test]
    fn resolve_first_falls_through_to_a_lower_mount() {
        let paths = AssetPath::combine(vec![
            Box::new(FakeMount(vec!["txt16/other.dds"])),
            Box::new(FakeMount(vec!["txt16/foo.pcx"])),
        ]);
        assert_eq!(
            paths.resolve_first(
                String::new(),
                &candidates(&["txt16/foo.dds", "txt16/foo.pcx"])
            ),
            Some("txt16/foo.pcx".to_owned())
        );
    }

    #[test]
    fn resolve_first_returns_none_when_nothing_matches() {
        let paths = AssetPath::combine(vec![Box::new(FakeMount(vec!["txt16/other.pcx"]))]);
        assert_eq!(
            paths.resolve_first(String::new(), &candidates(&["txt16/foo.pcx"])),
            None
        );
    }

    #[test]
    fn absolute_folder_mount_resolves_without_prefixing_the_base_path() {
        let root = TempAssetTree::new("absolute-folder-mount");
        let folder = root.0.join("res/obj");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("override.bin"), b"loose override").unwrap();

        let path = AssetPath::folder(folder.to_string_lossy().into_owned());
        let base_path = root.0.to_string_lossy().into_owned();

        assert!(path.exists(base_path.clone(), "override.bin".to_owned()));
        let reader = path
            .get_reader(base_path, "override.bin".to_owned())
            .unwrap();
        let mut contents = String::new();
        reader.borrow_mut().read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "loose override");
    }

    #[test]
    fn relative_folder_mount_resolves_under_the_base_path() {
        let root = TempAssetTree::new("relative-folder-mount");
        let folder = root.0.join("res/obj");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("override.bin"), b"relative override").unwrap();

        let path = AssetPath::folder("res/obj".to_owned());
        let base_path = root.0.to_string_lossy().into_owned();

        assert!(path.exists(base_path.clone(), "override.bin".to_owned()));
        let reader = path
            .get_reader(base_path, "override.bin".to_owned())
            .unwrap();
        let mut contents = String::new();
        reader.borrow_mut().read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "relative override");
    }
}
