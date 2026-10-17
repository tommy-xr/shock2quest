use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=DARKVR_BUNDLE_DIR");
    let mut source = "pub const FILES: &[(&str, &[u8])] = &[\n".to_string();
    if env::var_os("CARGO_FEATURE_INSTALLER").is_some()
        && let Some(dir) = env::var_os("DARKVR_BUNDLE_DIR")
    {
        let dir = fs::canonicalize(dir).expect("DARKVR_BUNDLE_DIR does not exist");
        let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
        let files = if windows {
            vec![
                "adb.exe",
                "AdbWinApi.dll",
                "AdbWinUsbApi.dll",
                "shock2quest.apk",
                "NOTICE.txt",
            ]
        } else {
            vec!["adb", "shock2quest.apk", "NOTICE.txt"]
        };
        for name in files {
            let path = dir.join(name);
            assert!(path.is_file(), "missing bundled file: {}", path.display());
            println!("cargo:rerun-if-changed={}", path.display());
            source.push_str(&format!(
                "({name:?}, include_bytes!({:?})),\n",
                path.to_str().unwrap()
            ));
        }
    }
    source.push_str("];");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("installer_payload.rs"),
        source,
    )
    .unwrap();
}
