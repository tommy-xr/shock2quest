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

/// Inverse of [`normalize_clip_name`]: the motion-DB name behind a `.mc` asset
/// key, tolerating a plain `<name>.mc` and a leading directory.
pub fn clip_base_name(asset_key: &str) -> String {
    let file = asset_key
        .rsplit('/')
        .next()
        .unwrap_or(asset_key)
        .to_ascii_lowercase();
    let stem = file.strip_suffix(".mc").unwrap_or(&file);
    stem.strip_suffix('_').unwrap_or(stem).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_name_round_trip() {
        assert_eq!(normalize_clip_name("bh111001").unwrap(), "bh111001_.mc");
        assert_eq!(clip_base_name("bh111001_.mc"), "bh111001");
        assert_eq!(clip_base_name("name.mc"), "name");
        assert_eq!(clip_base_name("sub/dir/NAME_.MC"), "name");
        assert_eq!(clip_base_name("name"), "name");
    }
}
