use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
};

use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

fn import_strings(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> Vec<String> {
    let buffered = BufReader::new(reader);
    let lines = buffered.lines();
    let mut out = Vec::new();
    for maybe_line in lines {
        if let Ok(line) = maybe_line {
            out.push(line)
        }
    }
    out
}

fn process_strings(
    content: Vec<String>,
    _asset_cache: &mut AssetCache,
    _config: &(),
) -> HashMap<String, String> {
    parse_strings(&content)
}

/// Parse the lines of a Dark `.STR` table into its `key -> value` map.
///
/// Split out of the importer so screens can unit-test their key names against
/// the verbatim shipped table, without an `AssetCache` or the filesystem.
/// Keys are lowercased; values may span multiple lines.
pub fn parse_strings(content: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let inner_content = content.iter();

    let mut current_key: Option<String> = None;
    let mut current_value = String::new();

    for line_str in inner_content {
        let entry = line_str.split_once(':').and_then(|(key, value)| {
            value
                .trim_start()
                .strip_prefix('"')
                .map(|value| (key, value))
        });
        if let Some((key, value)) = entry {
            if let Some(key) = current_key.take() {
                map.insert(
                    key.to_ascii_lowercase(),
                    current_value.trim_end_matches("\"").trim().to_string(),
                );
                current_value.clear();
            }

            current_key = Some(key.trim().to_string());
            current_value = value.to_string();
        } else if current_key.is_some() {
            current_value.push_str("\n");
            current_value.push_str(&line_str);
        }

        if line_str.ends_with("\"") && current_key.is_some() {
            if let Some(key) = current_key.take() {
                map.insert(
                    key.to_ascii_lowercase(),
                    current_value.trim_end_matches("\"").trim().to_string(),
                );
                current_value.clear();
            }
        }
    }
    map
}

/// Split a Dark object string (`key: "fallback text"`) into its lookup key and
/// its inline fallback text.
fn split_object_string(raw: &str) -> (&str, &str) {
    match raw.split_once(':') {
        Some((key, remainder)) => {
            let remainder = remainder.trim();
            let fallback = match remainder.strip_prefix('"') {
                Some(quoted) => quoted.split_once('"').map_or("", |(value, _)| value),
                None => remainder,
            };
            (key.trim(), fallback)
        }
        None => (raw.trim(), ""),
    }
}

fn lookup(strings: &HashMap<String, String>, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    strings.get(&key.to_ascii_lowercase()).cloned()
}

/// Resolve an object string against a string table, falling back to the inline
/// text the property carries.
pub fn resolve_localized_property_string(raw: &str, strings: &HashMap<String, String>) -> String {
    let (key, fallback) = split_object_string(raw);
    lookup(strings, key).unwrap_or_else(|| fallback.to_owned())
}

/// Resolve a gun fire-setting string (`P$Sett1`/`P$SHead2`/...) against its
/// `SETT*`/`SHEAD*` table.
///
/// Some guns author no setting property at all - the Stasis Field Generator,
/// Worm Launcher and Viral Proliferator - yet the tables carry entries for
/// them, keyed by their symbolic name with spaces underscored
/// (`Stasis_Field_Generator`). So the symbolic name is tried whenever the
/// property is missing or its own key misses.
pub fn resolve_gun_setting_string(
    raw: Option<&str>,
    sym_name: Option<&str>,
    strings: &HashMap<String, String>,
) -> Option<String> {
    let (key, fallback) = raw.map_or(("", ""), split_object_string);
    lookup(strings, key)
        .or_else(|| lookup(strings, &sym_name.unwrap_or_default().replace(' ', "_")))
        .or_else(|| (!fallback.is_empty()).then(|| fallback.to_owned()))
}

pub static STRINGS_IMPORTER: Lazy<AssetImporter<Vec<String>, HashMap<String, String>, ()>> =
    Lazy::new(|| AssetImporter::define(import_strings, process_strings));

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{parse_strings, resolve_gun_setting_string, resolve_localized_property_string};

    fn table(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn object_string_resolves_its_key_against_the_table() {
        let strings = table(&[("elevator_button", "Localized elevator button")]);

        assert_eq!(
            resolve_localized_property_string(
                r#"Elevator_Button: "A two-state button.""#,
                &strings
            ),
            "Localized elevator button"
        );
    }

    #[test]
    fn object_string_falls_back_to_its_embedded_text() {
        assert_eq!(
            resolve_localized_property_string(r#"HumanCorpses: "A corpse.""#, &table(&[])),
            "A corpse."
        );
    }

    #[test]
    fn a_bare_key_with_no_embedded_text_still_resolves() {
        let strings = table(&[("basketball", "A basketball.")]);

        assert_eq!(
            resolve_localized_property_string("Basketball", &strings),
            "A basketball."
        );
    }

    #[test]
    fn setting_string_prefers_the_table_over_the_inline_fallback() {
        let strings = table(&[("pistol", "BURST")]);

        assert_eq!(
            resolve_gun_setting_string(Some(r#"Pistol: "stale""#), Some("Pistol"), &strings),
            Some("BURST".to_string())
        );
    }

    #[test]
    fn setting_string_falls_back_to_the_inline_text() {
        assert_eq!(
            resolve_gun_setting_string(Some(r#"Pistol: "BURST""#), Some("Pistol"), &table(&[])),
            Some("BURST".to_string())
        );
    }

    #[test]
    fn setting_string_falls_back_to_the_underscored_symbolic_name() {
        let strings = table(&[("stasis_field_generator", "AREA")]);

        assert_eq!(
            resolve_gun_setting_string(None, Some("Stasis Field Generator"), &strings),
            Some("AREA".to_string())
        );
    }

    #[test]
    fn setting_string_is_absent_when_nothing_resolves() {
        assert_eq!(
            resolve_gun_setting_string(None, Some("Rick Turret Gun"), &table(&[])),
            None
        );
    }

    #[test]
    fn accepts_whitespace_between_colon_and_opening_quote() {
        let strings = parse_strings(&[r#"LogText12:  "Glory to the Many!""#.to_string()]);

        assert_eq!(
            strings.get("logtext12").map(String::as_str),
            Some("Glory to the Many!")
        );
    }
}
