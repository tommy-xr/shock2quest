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
    let mut map = HashMap::new();
    let inner_content = content.iter();

    let mut current_key: Option<String> = None;
    let mut current_value = String::new();

    for line_str in inner_content {
        if let Some(pos) = line_str.find(":\"") {
            if let Some(key) = current_key.take() {
                map.insert(
                    key.to_ascii_lowercase(),
                    current_value.trim_end_matches("\"").trim().to_string(),
                );
                current_value.clear();
            }

            let (key, value) = line_str.split_at(pos);
            current_key = Some(key.trim().to_string());
            current_value = value[2..].to_string();
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
