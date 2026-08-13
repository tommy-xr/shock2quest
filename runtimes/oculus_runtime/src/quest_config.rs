use std::fs;

/// Booted when no mission is configured: the main menu, which VR drives with a
/// controller ray (see `MainMenuScene`'s world-space panel).
pub const DEFAULT_MISSION: &str = "main_menu";
pub const MISSION_CONFIG_PATH: &str = "/sdcard/shock2quest/vr-mission.txt";

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

fn parse_mission(raw: &str) -> Option<String> {
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
    use super::parse_mission;

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
