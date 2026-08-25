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
    /// Take the *deepest* schema node the query reached rather than the
    /// shallowest. The schema is a specificity tree and a match collects every
    /// node along the path, so a query that refines an already-resolving node
    /// (`material=metal` -> `landing=true`) otherwise resolves to the parent's
    /// samples and the refinement is silently inert. Off by default so
    /// existing lookups keep resolving exactly as they did.
    prefer_most_specific: bool,
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
        Self {
            items: Vec::new(),
            prefer_most_specific: false,
        }
    }

    pub fn from_items(items: Vec<EnvSoundQueryItem>) -> Self {
        Self {
            items,
            prefer_most_specific: false,
        }
    }

    pub fn from_tag_values(items: Vec<(&str, &str)>) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|(tag, value)| EnvSoundQueryItem::new(tag, value))
                .collect(),
            prefer_most_specific: false,
        }
    }

    /// Resolve to the deepest matched schema node instead of the shallowest -
    /// see [`EnvSoundQuery::prefer_most_specific`].
    pub fn most_specific(mut self) -> Self {
        self.prefer_most_specific = true;
        self
    }

    pub fn prefers_most_specific(&self) -> bool {
        self.prefer_most_specific
    }

    /// The (tag, value) pairs of this query, e.g. for logging/introspection.
    pub fn tag_values(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .map(|item| (item.tag.clone(), item.value.clone()))
            .collect()
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
