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

#[cfg(test)]
mod tests {
    use super::*;

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
}
