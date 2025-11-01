use crate::file_system::FileSystem;

pub trait Storage {
    fn bundle_filesystem(&self) -> &dyn FileSystem;
}

pub struct StorageImpl {
    bundle_filesystem: Box<dyn FileSystem>,
}

impl Storage for StorageImpl {
    fn bundle_filesystem(&self) -> &dyn FileSystem {
        &*self.bundle_filesystem
    }
}

pub fn init(bundle: Box<dyn FileSystem>) -> Box<dyn Storage> {
    Box::new(StorageImpl {
        bundle_filesystem: bundle,
    })
}
