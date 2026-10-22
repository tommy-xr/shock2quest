use std::fs;

/// Booted when no mission is configured: the main menu, which VR drives with a
/// controller ray (see `MainMenuScene`'s world-space panel).
pub const DEFAULT_MISSION: &str = "main_menu";
pub const MISSION_CONFIG_PATH: &str = "/sdcard/shock2quest/vr-mission.txt";

/// A data-root file read once at launch to opt into a feature. `cargo dvr
/// features` lists and edits every entry of `SWITCHES`.
// `name` and `SWITCHES` are read only by that host tool.
#[cfg_attr(target_os = "android", allow(dead_code))]
pub struct Switch {
    pub name: &'static str,
    pub file: &'static str,
    /// A two-state switch lists `[off, on]`, and `off` is its default.
    pub values: &'static [&'static str],
    /// Used when the file is missing.
    pub default: &'static str,
}

pub const SIMULATION_CLOCK: Switch = Switch {
    name: "simulation-clock",
    file: "simulation-clock.txt",
    values: &["legacy", "fixed"],
    default: "legacy",
};
pub const MULTIVIEW: Switch = Switch {
    name: "multiview",
    file: "multiview.txt",
    values: &["off", "on"],
    default: "off",
};
/// Parsed by `foveation::Level`; invalid text falls back to `off`, not the default.
pub const FFR_LEVEL: Switch = Switch {
    name: "ffr-level",
    file: "ffr-level.txt",
    values: &["off", "low", "medium", "high"],
    default: "low",
};
#[cfg_attr(target_os = "android", allow(dead_code))]
pub const SWITCHES: &[Switch] = &[SIMULATION_CLOCK, MULTIVIEW, FFR_LEVEL];

/// Explicit opt-in for interaction testing outside a benchmark fixture.
// The host CLI shares this module's parser and switch table, but does not link the game.
#[cfg(target_os = "android")]
pub fn fixed_simulation_enabled() -> bool {
    read_switch(&SIMULATION_CLOCK, "SIMULATION")
}

/// Opt-in single-pass stereo (OVR_multiview2) for A/B profiling.
#[cfg(target_os = "android")]
pub fn multiview_requested() -> bool {
    read_switch(&MULTIVIEW, "MULTIVIEW")
}

/// A two-state switch: `on` -> true; missing, `off` or invalid -> false.
#[cfg(target_os = "android")]
fn read_switch(switch: &Switch, tag: &str) -> bool {
    let &[off, on] = switch.values else {
        unreachable!("{} is not a two-state switch", switch.name)
    };
    let path = shock2vr::paths::data_root().join(switch.file);
    match fs::read_to_string(path) {
        Ok(raw) if raw.trim() == on => true,
        Ok(raw) if raw.trim() == off => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        other => {
            println!("SHOCK2QUEST_{tag}_CONFIG invalid={other:?} fallback={off}");
            false
        }
    }
}

pub fn configured_mission() -> String {
    match fs::read_to_string(MISSION_CONFIG_PATH) {
        Ok(raw) => match parse_mission(&raw) {
            Some(mission) => mission,
            None => {
                println!(
                    "SHOCK2QUEST_CONFIG invalid_mission={:?} fallback={DEFAULT_MISSION}",
                    raw.trim()
                );
                DEFAULT_MISSION.to_owned()
            }
        },
        Err(error) => {
            println!(
                "SHOCK2QUEST_CONFIG mission_file={MISSION_CONFIG_PATH} error={error} fallback={DEFAULT_MISSION}"
            );
            DEFAULT_MISSION.to_owned()
        }
    }
}

pub fn parse_mission(raw: &str) -> Option<String> {
    let mission = raw.trim();
    let valid_characters = mission
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
    // `main_menu` is a scene name rather than a file, and it is the default -
    // so the parser must accept what `DEFAULT_MISSION` already is, otherwise a
    // player could not configure the value the game boots into anyway.
    let valid_name =
        mission.starts_with("debug_") || mission.ends_with(".mis") || mission == DEFAULT_MISSION;

    (!mission.is_empty() && valid_characters && valid_name).then(|| mission.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MISSION, MULTIVIEW, SIMULATION_CLOCK, SWITCHES, parse_mission};

    #[test]
    fn switch_defaults_are_listed_and_two_state_switches_default_off() {
        for switch in SWITCHES {
            assert!(switch.values.contains(&switch.default), "{}", switch.name);
        }
        // `read_switch` reads these as `[off, on]` and treats a missing file as off.
        for switch in [SIMULATION_CLOCK, MULTIVIEW] {
            assert_eq!(switch.values.len(), 2, "{}", switch.name);
            assert_eq!(switch.values[0], switch.default, "{}", switch.name);
        }
    }

    #[test]
    fn accepts_missions_and_debug_scenes() {
        assert_eq!(parse_mission(" medsci1.mis\n"), Some("medsci1.mis".into()));
        assert_eq!(parse_mission("debug_weapons"), Some("debug_weapons".into()));
    }

    #[test]
    fn rejects_paths_and_unrecognized_names() {
        assert_eq!(parse_mission("../earth.mis"), None);
        assert_eq!(parse_mission("/sdcard/earth.mis"), None);
        assert_eq!(parse_mission("earth"), None);
        // The default must round-trip through the validator.
        assert_eq!(
            parse_mission(DEFAULT_MISSION),
            Some(DEFAULT_MISSION.to_owned())
        );
        assert_eq!(parse_mission("  main_menu  "), Some("main_menu".to_owned()));
        assert_eq!(parse_mission("earth.mis --flag"), None);
    }
}
