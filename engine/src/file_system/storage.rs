use crate::file_system::FileSystem;

pub trait Storage {
    fn external_filesystem(&self) -> &Box<dyn FileSystem>;
    fn bundle_filesystem(&self) -> &Box<dyn FileSystem>;
}

pub struct StorageImpl {
    external_filesystem: Box<dyn FileSystem>,
    bundle_filesystem: Box<dyn FileSystem>,
}

impl Storage for StorageImpl {
    fn external_filesystem(&self) -> &Box<dyn FileSystem> {
        &self.external_filesystem
    }

    fn bundle_filesystem(&self) -> &Box<dyn FileSystem> {
        &self.bundle_filesystem
    }
}

pub fn init(external: Box<dyn FileSystem>, bundle: Box<dyn FileSystem>) -> Box<dyn Storage> {
    Box::new(StorageImpl {
        external_filesystem: external,
        bundle_filesystem: bundle,
    })
}
