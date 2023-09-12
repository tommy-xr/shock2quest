use crate::{NameMap, TagQuery, TagQueryItem};

///
/// motion_query.rs
///
/// Module to assist with conversion of motion items -> db items

#[derive(Clone, Debug)]
pub enum MotionQueryItem {
    Tag(String, bool),
    TagWithValue(String, i32, bool),
}

#[derive(Clone, Debug)]
pub enum MotionQuerySelectionStrategy {
    Sequential(u32),
    Random,
}

#[derive(Clone, Debug)]
pub struct MotionQuery {
    pub creature_type: u32,
    pub items: Vec<MotionQueryItem>,
    pub selection_strategy: MotionQuerySelectionStrategy,
}

impl MotionQueryItem {
    /// Create a new environmental sound query item
    pub fn new(tag: &str) -> Self {
        Self::Tag(tag.to_ascii_lowercase(), false)
    }

    pub fn with_value(tag: &str, val: i32) -> Self {
        Self::TagWithValue(tag.to_ascii_lowercase(), val, false)
    }

    pub fn tag_name(&self) -> &str {
        match self {
            MotionQueryItem::Tag(str, _) => str,
            MotionQueryItem::TagWithValue(str, _v, _) => str,
        }
    }

    pub fn optional(self) -> Self {
        match self {
            MotionQueryItem::Tag(str, _) => MotionQueryItem::Tag(str, true),
            MotionQueryItem::TagWithValue(str, v, _) => MotionQueryItem::TagWithValue(str, v, true),
        }
    }
}

impl MotionQuery {
    pub fn new(creature_type: u32, items: Vec<MotionQueryItem>) -> MotionQuery {
        Self {
            creature_type,
            items,
            selection_strategy: MotionQuerySelectionStrategy::Random,
        }
    }

    pub fn with_selection_strategy(self, selection_strategy: MotionQuerySelectionStrategy) -> Self {
        Self {
            selection_strategy,
            ..self
        }
    }

    /// Convert an environmental sound query to a tag query, given the relevant name maps
    pub(crate) fn to_tag_query(&self, tag_map: &NameMap) -> TagQuery {
        let mut tag_query_items = Vec::new();

        for item in &self.items {
            let maybe_tag_id = tag_map.get_index(item.tag_name());

            if maybe_tag_id.is_none() {
                continue;
            }

            let tag_id = maybe_tag_id.unwrap();

            let tag_query_item = match item {
                MotionQueryItem::Tag(_, optional) => TagQueryItem::Key(tag_id, *optional),
                MotionQueryItem::TagWithValue(_, v, optional) => {
                    TagQueryItem::KeyWithIntValue(tag_id, *v, *optional)
                }
            };

            tag_query_items.push(tag_query_item);
        }
        TagQuery::from_items(tag_query_items)
    }
}
