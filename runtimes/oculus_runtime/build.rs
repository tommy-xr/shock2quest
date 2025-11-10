use std::env;
use std::path::PathBuf;

fn main() {
    // For Android builds, just set up linking to our prebuilt ffmpeg libraries
    if env::var("TARGET").unwrap_or_default().contains("android") {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let lib_dir = PathBuf::from(&manifest_dir).join("lib/arm64-v8a");

        // Tell cargo where to find our ffmpeg libraries
        println!("cargo:rustc-link-search=native={}", lib_dir.display());

        // Link individual ffmpeg libraries (these will be dynamically loaded)
        println!("cargo:rustc-link-lib=dylib=avcodec");
        println!("cargo:rustc-link-lib=dylib=avdevice");
        println!("cargo:rustc-link-lib=dylib=avfilter");
        println!("cargo:rustc-link-lib=dylib=avformat");
        println!("cargo:rustc-link-lib=dylib=avutil");
        println!("cargo:rustc-link-lib=dylib=swresample");
        println!("cargo:rustc-link-lib=dylib=swscale");
    }
}
