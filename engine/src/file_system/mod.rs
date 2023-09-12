pub mod file_system;
pub use file_system::FileSystem;

pub mod storage;
pub use storage::Storage;

pub mod default_file_system;
pub use default_file_system::DefaultFileSystem;

#[cfg(target_os = "android")]
pub mod android_file_system;
#[cfg(target_os = "android")]
pub use android_file_system::AndroidFileSystem;
