use std::sync::Arc;

use crate::file_system::FileSystem;

pub trait Storage: Send + Sync {
    fn bundle_filesystem(&self) -> &dyn FileSystem;
    fn bundle_filesystem_arc(&self) -> Arc<dyn FileSystem>;
}

pub struct StorageImpl {
    bundle_filesystem: Arc<dyn FileSystem>,
}

impl Storage for StorageImpl {
    fn bundle_filesystem(&self) -> &dyn FileSystem {
        &*self.bundle_filesystem
    }

    fn bundle_filesystem_arc(&self) -> Arc<dyn FileSystem> {
        Arc::clone(&self.bundle_filesystem)
    }
}

pub fn init(bundle: Box<dyn FileSystem>) -> Arc<dyn Storage> {
    Arc::new(StorageImpl {
        bundle_filesystem: Arc::from(bundle),
    })
}
