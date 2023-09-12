pub trait FileSystem {
    fn open_dir(&self, path: &String) -> Vec<String>;
    fn open_file(&self, path: &String) -> Vec<u8>;
}
