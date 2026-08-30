//! Window-free viewer scenes, reusable by other tools (dark_explorer's
//! model preview embeds them); the `dark_viewer` binary drives them in GLFW.

pub mod scenes;

/// Normalize a user-supplied animation name to the `<name>_.mc` asset name
/// the clip importer expects, accepting a bare name, `name.mc`, or `name_.mc`.
pub fn normalize_clip_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Animation name may not be empty".to_owned());
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("_.mc") {
        return Ok(trimmed.to_owned());
    }
    if lower.ends_with(".mc") {
        let without_ext = &trimmed[..trimmed.len() - 3];
        return Ok(format!("{}_.mc", without_ext));
    }
    Ok(format!("{}_.mc", trimmed))
}
