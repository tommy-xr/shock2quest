pub use crate::file_system::FileSystem;
use std::path::Path;

pub struct DefaultFileSystem<'a> {
    pub root_path: Box<&'a Path>,
}

impl FileSystem for DefaultFileSystem<'_> {
    fn open_dir(&self, path: &str) -> Vec<String> {
        let full_path = self.root_path.join(path);
        let read_dir_result = std::fs::read_dir(full_path).unwrap();

        read_dir_result
            .map(|entry| {
                let entry = entry.unwrap();
                let entry_path = entry.path();
                let file_name = entry_path.file_name().unwrap();
                let file_name_as_str = file_name.to_str().unwrap();

                String::from(file_name_as_str)
            })
            .collect::<Vec<String>>()
    }

    fn open_file(&self, path: &str) -> Vec<u8> {
        let full_path = self.root_path.join(path);
        println!("open_file: {full_path:?}");

        std::fs::read(full_path).unwrap()
    }

    fn file_exists(&self, path: &str) -> bool {
        let full_path = self.root_path.join(path);
        full_path.exists()
    }
}
