//! Developer UI access, persisted independently of saves by a data-root sentinel.
use std::{
    fs, io,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

struct DeveloperMode {
    sentinel: PathBuf,
    enabled: bool,
}

impl DeveloperMode {
    fn load(sentinel: PathBuf) -> Self {
        let enabled = sentinel.is_file();
        Self { sentinel, enabled }
    }

    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            fs::write(&self.sentinel, b"Developer mode enabled\n")?;
        } else {
            match fs::remove_file(&self.sentinel) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
        // Keep the UI honest if storage is read-only or removal fails.
        self.enabled = enabled;
        Ok(())
    }
}

fn state() -> &'static Mutex<DeveloperMode> {
    static STATE: OnceLock<Mutex<DeveloperMode>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(DeveloperMode::load(
            crate::paths::data_root().join("developer-mode"),
        ))
    })
}

pub fn enabled() -> bool {
    state().lock().unwrap().enabled
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    state().lock().unwrap().set_enabled(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn sentinel_survives_restart_and_disable_removes_it() {
        let root = TempDir::new("developer-mode");
        let sentinel = root.path().join("developer-mode");
        let mut mode = DeveloperMode::load(sentinel.clone());
        assert!(!mode.enabled);
        mode.set_enabled(true).unwrap();
        let mut restarted = DeveloperMode::load(sentinel.clone());
        assert!(restarted.enabled);
        restarted.set_enabled(false).unwrap();
        assert!(!DeveloperMode::load(sentinel).enabled);
        restarted.set_enabled(false).unwrap();
    }

    #[test]
    fn failed_writes_or_removal_do_not_change_session_state() {
        let root = TempDir::new("developer-mode-errors");
        let sentinel = root.path().join("missing").join("developer-mode");
        let mut mode = DeveloperMode::load(sentinel);
        assert!(mode.set_enabled(true).is_err());
        assert!(!mode.enabled);
        // A directory cannot be removed with remove_file, even with root access.
        mode.sentinel = root.path().to_owned();
        mode.enabled = true;
        assert!(mode.set_enabled(false).is_err());
        assert!(mode.enabled);
    }
}
