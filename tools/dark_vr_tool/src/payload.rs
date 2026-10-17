use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

include!(concat!(env!("OUT_DIR"), "/installer_payload.rs"));

pub struct Payload {
    pub adb: PathBuf,
    pub apk: Option<PathBuf>,
}

pub fn prepare() -> Result<Payload> {
    if !FILES.is_empty() {
        let base = if cfg!(windows) {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?)
        } else {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".cache")
        };
        return extract(FILES, &base.join("Shock2Quest/installer"));
    }
    // macOS keeps signed helper code inside the app bundle. Development builds
    // can use PATH and an explicitly selected APK without any packaged payload.
    let exe = std::env::current_exe()?;
    let contents = exe
        .parent()
        .and_then(Path::parent)
        .context("executable has no parent directory")?;
    let helper = contents.join("Helpers/adb");
    let apk = contents.join("Resources/shock2quest.apk");
    Ok(Payload {
        adb: if helper.is_file() {
            helper
        } else {
            "adb".into()
        },
        apk: apk.is_file().then_some(apk),
    })
}

fn extract(files: &[(&str, &[u8])], base: &Path) -> Result<Payload> {
    let mut digest = Sha256::new();
    for (name, bytes) in files {
        digest.update(name.as_bytes());
        digest.update(bytes);
    }
    // Keep versions separate: an existing adb server can still hold the old
    // executable open on Windows. Publish each file atomically for concurrent starts.
    let dir = base.join(format!("{:x}", digest.finalize()));
    fs::create_dir_all(&dir)?;
    for (name, bytes) in files {
        let path = dir.join(name);
        if fs::read(&path).is_ok_and(|existing| existing == *bytes) {
            continue;
        }
        let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
        temp.write_all(bytes)?;
        #[cfg(unix)]
        if *name == "adb" {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        if let Err(error) = temp.persist(&path) {
            // Another installer may have published and started this executable;
            // Windows will not replace it while it is running.
            if !fs::read(&path).is_ok_and(|existing| existing == *bytes) {
                return Err(error).with_context(|| format!("could not prepare {}", path.display()));
            }
        }
    }
    Ok(Payload {
        adb: dir.join(if cfg!(windows) { "adb.exe" } else { "adb" }),
        apk: Some(dir.join("shock2quest.apk")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_payload_is_reused_repaired_and_versioned() {
        let base = tempfile::tempdir().unwrap();
        let files: &[(&str, &[u8])] = &[("adb", b"tool"), ("shock2quest.apk", b"apk")];
        let first = extract(files, base.path()).unwrap().apk.unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"apk");
        assert_eq!(extract(files, base.path()).unwrap().apk.unwrap(), first);
        fs::write(&first, "partial").unwrap();
        extract(files, base.path()).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"apk");
        let newer = extract(&[("shock2quest.apk", b"new apk")], base.path())
            .unwrap()
            .apk
            .unwrap();
        assert_ne!(newer, first);
        assert_eq!(fs::read(&first).unwrap(), b"apk");
    }
}
