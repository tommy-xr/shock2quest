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

pub static STRINGS_IMPORTER: Lazy<AssetImporter<Vec<String>, HashMap<String, String>, ()>> =
    Lazy::new(|| AssetImporter::define(import_strings, process_strings));

#[cfg(test)]
mod tests {
    use super::parse_strings;

    #[test]
    fn accepts_whitespace_between_colon_and_opening_quote() {
        let strings = parse_strings(&[r#"LogText12:  "Glory to the Many!""#.to_string()]);

        assert_eq!(
            strings.get("logtext12").map(String::as_str),
            Some("Glory to the Many!")
        );
    }
}
