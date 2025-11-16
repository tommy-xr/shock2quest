use crate::{NameMap, TagQuery, TagQueryItem};

///
/// env_sound_query.rs
///
/// Module to assist with conversion of environmental sound queries -> tag queries

#[derive(Clone, Debug)]
pub struct EnvSoundQueryItem {
    tag: String,
    value: String,
}

#[derive(Clone, Debug)]
pub struct EnvSoundQuery {
    items: Vec<EnvSoundQueryItem>,
}

impl EnvSoundQueryItem {
    /// Create a new environmental sound query item
    pub fn new(tag: &str, value: &str) -> Self {
        Self {
            tag: tag.to_ascii_lowercase(),
            value: value.to_ascii_lowercase(),
        }
    }
}

impl EnvSoundQuery {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn from_items(items: Vec<EnvSoundQueryItem>) -> Self {
        Self { items }
    }

    pub fn from_tag_values(items: Vec<(&str, &str)>) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|(tag, value)| EnvSoundQueryItem::new(tag, value))
                .collect(),
        }
    }

    /// Convert an environmental sound query to a tag query, given the relevant name maps
    pub(crate) fn to_tag_query(&self, tag_map: &NameMap, value_map: &NameMap) -> TagQuery {
        let mut tag_query_items = Vec::new();
        for item in &self.items {
            let maybe_tag_id = tag_map.get_index(&item.tag);

            if maybe_tag_id.is_none() {
                continue;
            }

            // First try to resolve the value as a string in the value map
            let maybe_value_id = value_map.get_index(&item.value).map(|x| x as u8);

            // If that fails, try to parse it as a direct numeric value
            let final_value_id = if let Some(value_id) = maybe_value_id {
                value_id
            } else if let Ok(numeric_value) = item.value.parse::<u8>() {
                numeric_value
            } else {
                continue;
            };

            tag_query_items.push(TagQueryItem::KeyWithEnumValue(
                maybe_tag_id.unwrap(),
                final_value_id,
                false,
            ));
        }
        TagQuery::from_items(tag_query_items)
    }
}
