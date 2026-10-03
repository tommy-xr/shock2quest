//! Device preferences, independent of campaign saves and developer parameters.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnMode {
    #[default]
    Snap,
    Smooth,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VignetteStrength {
    Off,
    #[default]
    Low,
    Medium,
    High,
}

impl VignetteStrength {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    /// Clear and fully masked radii, as fractions of the view's half-extents.
    pub fn radii(self) -> (f32, f32) {
        match self {
            Self::Off => (1.0, 1.2),
            Self::Low => (0.75, 1.05),
            Self::Medium => (0.55, 0.85),
            Self::High => (0.35, 0.65),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VrSettings {
    pub turning: TurnMode,
    pub snap_angle: f32,
    pub smooth_speed: f32,
    pub vignette: VignetteStrength,
    pub vignette_movement: bool,
    pub vignette_turning: bool,
}

impl Default for VrSettings {
    fn default() -> Self {
        Self {
            turning: TurnMode::Snap,
            snap_angle: 30.0,
            smooth_speed: 90.0,
            vignette: VignetteStrength::Low,
            vignette_movement: true,
            vignette_turning: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub vr: VrSettings,
}

impl UserSettings {
    fn normalized(mut self) -> Self {
        if ![30.0, 45.0, 60.0].contains(&self.vr.snap_angle) {
            self.vr.snap_angle = 30.0;
        }
        if ![45.0, 60.0, 90.0, 120.0, 180.0].contains(&self.vr.smooth_speed) {
            self.vr.smooth_speed = 90.0;
        }
        self
    }
}

struct SettingsStore {
    path: PathBuf,
    value: UserSettings,
}

impl SettingsStore {
    fn load(path: PathBuf) -> Self {
        let value = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<UserSettings>(&bytes) {
                Ok(value) => value.normalized(),
                Err(error) => {
                    tracing::warn!(%error, "Invalid user settings; using defaults");
                    UserSettings::default()
                }
            },
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(%error, "Could not read user settings");
                }
                UserSettings::default()
            }
        };
        Self { path, value }
    }

    fn save(&mut self, value: UserSettings) -> io::Result<()> {
        let value = value.normalized();
        let bytes = serde_json::to_vec_pretty(&value)?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, bytes)?;
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        // A failed write must not claim a preference was saved or apply it live.
        self.value = value;
        Ok(())
    }
}

fn store() -> &'static Mutex<SettingsStore> {
    static STORE: OnceLock<Mutex<SettingsStore>> = OnceLock::new();
    STORE.get_or_init(|| {
        let path = std::env::var_os("SHOCK2_SETTINGS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::paths::data_root().join("user-settings.json"));
        Mutex::new(SettingsStore::load(path))
    })
}

pub fn get() -> UserSettings {
    store().lock().unwrap().value
}

pub fn update(change: impl FnOnce(&mut UserSettings)) -> io::Result<()> {
    let mut store = store().lock().unwrap();
    let mut value = store.value;
    change(&mut value);
    store.save(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn preferences_survive_restart_without_a_campaign_save() {
        let root = TempDir::new("user-settings");
        let path = root.path().join("settings.json");
        let mut store = SettingsStore::load(path.clone());
        let mut value = store.value;
        value.vr.turning = TurnMode::Smooth;
        value.vr.snap_angle = 60.0;
        value.vr.vignette = VignetteStrength::High;
        store.save(value).unwrap();
        assert_eq!(SettingsStore::load(path).value, value);
    }

    #[test]
    fn failed_save_keeps_previous_preferences() {
        let root = TempDir::new("user-settings-fail");
        let mut store = SettingsStore::load(root.path().join("missing/settings.json"));
        let mut value = store.value;
        value.vr.turning = TurnMode::Smooth;
        assert!(store.save(value).is_err());
        assert_eq!(store.value, UserSettings::default());
    }

    #[test]
    fn missing_fields_and_invalid_values_use_defaults() {
        let root = TempDir::new("user-settings-invalid");
        let path = root.path().join("settings.json");
        fs::write(&path, r#"{"vr":{"snap_angle":-5,"smooth_speed":999}}"#).unwrap();
        assert_eq!(
            SettingsStore::load(path.clone()).value,
            UserSettings::default()
        );
        fs::write(&path, "not json").unwrap();
        assert_eq!(SettingsStore::load(path).value, UserSettings::default());
    }
}
