pub trait FileSystem {
    fn open_dir(&self, path: &str) -> Vec<String>;
    fn open_file(&self, path: &str) -> Vec<u8>;
}
