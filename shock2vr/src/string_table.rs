//! String tables shared by scripts that have no asset cache at update time.
use std::collections::HashMap;

use engine::assets::asset_cache::AssetCache;
use shipyard::Unique;

#[derive(Unique, Clone, Debug, Default)]
pub struct StringTable(HashMap<String, HashMap<String, String>>);

impl StringTable {
    pub fn load(assets: &mut AssetCache, files: &[&str]) -> Self {
        Self(
            files
                .iter()
                .map(|file| {
                    let entries = assets
                        .get_opt(&dark::importers::STRINGS_IMPORTER, file)
                        .map(|entries| (*entries).clone())
                        .unwrap_or_default();
                    ((*file).to_owned(), entries)
                })
                .collect(),
        )
    }

    /// File names include `.str`; absent tables and absent keys both miss.
    pub fn get(&self, file: &str, key: &str) -> Option<&String> {
        self.table(file).and_then(|table| Self::lookup(table, key))
    }

    /// Lookup policy shared with callers that already have a cached raw table.
    pub fn lookup<'a>(table: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
        table.get(&key.to_ascii_lowercase())
    }

    pub fn table(&self, file: &str) -> Option<&HashMap<String, String>> {
        self.0.get(file)
    }

    #[cfg(test)]
    pub(crate) fn from_table(file: &str, entries: HashMap<String, String>) -> Self {
        Self(HashMap::from([(file.to_owned(), entries)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_independent_and_keys_are_case_insensitive() {
        let tables = StringTable(HashMap::from([
            (
                "chargen.str".into(),
                HashMap::from([("key".into(), "Title".into())]),
            ),
            (
                "usemsg.str".into(),
                HashMap::from([("key".into(), "Message".into())]),
            ),
        ]));
        assert_eq!(
            tables.get("chargen.str", "KEY").map(String::as_str),
            Some("Title")
        );
        assert_eq!(
            tables.get("usemsg.str", "Key").map(String::as_str),
            Some("Message")
        );
        assert_eq!(tables.get("missing.str", "key"), None);
        assert_eq!(tables.get("chargen.str", "missing"), None);
    }
}
