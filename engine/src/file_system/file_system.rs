pub trait FileSystem: Send + Sync {
    fn open_dir(&self, path: &str) -> Vec<String>;
    fn open_file(&self, path: &str) -> Vec<u8>;
    fn file_exists(&self, path: &str) -> bool;
}
