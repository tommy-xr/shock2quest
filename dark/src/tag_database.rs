use std::{collections::HashMap, io, rc::Rc};

use crate::ss2_common::{read_i32, read_single, read_u32};

#[derive(Debug, Clone)]
pub struct TagDatabase {
    data: Vec<TagDatabaseData>,
    branches: HashMap<TagDatabaseKey, Rc<TagDatabase>>,
}

#[derive(Clone, Debug)]
pub enum TagQueryItem {
    KeyWithEnumValue(u32, u8, bool),
    KeyWithIntValue(u32, i32, bool),
    Key(u32, bool),
}

impl TagQueryItem {
    pub fn sort_key(&self) -> u32 {
        match self {
            Self::KeyWithEnumValue(key, _, _) => *key,
            Self::KeyWithIntValue(key, _, _) => *key,
            Self::Key(key, _) => *key,
        }
    }

    pub fn is_optional(&self) -> bool {
        match self {
            Self::KeyWithEnumValue(_, _, optional) => *optional,
            Self::KeyWithIntValue(_key, _, optional) => *optional,
            Self::Key(_key, optional) => *optional,
        }
    }
}

#[derive(Debug)]
pub struct TagQuery {
    items: Vec<TagQueryItem>,
}

impl TagQuery {
    pub fn new() -> TagQuery {
        TagQuery { items: Vec::new() }
    }

    pub fn from_items(items: Vec<TagQueryItem>) -> TagQuery {
        TagQuery { items }
    }

    pub fn sorted_items(&self) -> Vec<TagQueryItem> {
        let mut items = self.items.clone();
        items.sort_by_key(|a| a.sort_key());
        items
    }
}

impl TagDatabase {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T) -> TagDatabase {
        // TAG DATABASE
        let mut data = Vec::new();
        let size = read_u32(reader);

        for _ in 0..size {
            data.push(TagDatabaseData::read(reader));
        }

        // Data

        // Key
        let key_size = read_u32(reader);

        let mut branches = HashMap::new();

        for _ in 0..key_size {
            let key = TagDatabaseKey::read(reader);
            let db = TagDatabase::read(reader);

            branches.insert(key, Rc::new(db));
        }

        TagDatabase { data, branches }
    }

    pub fn query_match_all(&self, query: &TagQuery) -> Vec<i32> {
        // Get sorted items
        let sorted_query = query.sorted_items();

        let mut output = Vec::new();
        self.query_match_all_recursive(&sorted_query, 0, &mut output);
        output
    }

    fn query_match_all_recursive(
        &self,
        query_items: &Vec<TagQueryItem>,
        current_idx: usize,
        output: &mut Vec<i32>,
    ) -> Vec<i32> {
        if current_idx >= query_items.len() {
            // We've found all we needed to - collect the values!
            self.collect_all_values(output)
        } else {
            // Otherwise, continue traversing the tree... let's see if we can find a match

            let current_query = &query_items[current_idx];
            let count_before = output.len();
            for (key, tag_db) in &self.branches {
                if key.matches_query(current_query) {
                    for data in &tag_db.data {
                        output.push(data.data)
                    }

                    // Found a match - let's dive in!
                    tag_db.query_match_all_recursive(query_items, current_idx + 1, output);
                } else {
                    // No match, keep traversing
                    continue;
                }
            }

            let count_after = output.len();

            // If we didn't find anything, and the current query param is optional, try jumping to to the next query parameter
            if count_before == count_after && current_query.is_optional() {
                self.query_match_all_recursive(query_items, current_idx + 1, output);
            }
        }
        vec![]
    }

    pub fn query_one(&self, tag: u32) -> Vec<i32> {
        let mut ret = Vec::new();
        self.query_one_recursive(tag, &mut ret);
        ret.sort();
        ret
    }

    pub fn debug_print(
        &self,
        tag_to_name: &HashMap<u32, String>,
        data_to_name: &HashMap<i32, String>,
        enum_value_to_name: &HashMap<u32, String>,
    ) {
        self.debug_print_helper(tag_to_name, data_to_name, enum_value_to_name, "".to_owned());
    }

    pub fn debug_print_helper(
        &self,
        tag_to_name: &HashMap<u32, String>,
        data_to_name: &HashMap<i32, String>,
        enum_value_to_name: &HashMap<u32, String>,
        indentation: String,
    ) {
        for data in &self.data {
            let maybe_data = data_to_name.get(&data.data);
            tracing::debug!(
                "{} - {} [{}] (Weight: {})",
                indentation,
                maybe_data.unwrap(),
                data.data,
                data.weight
            );
        }

        for (key, branch) in &self.branches {
            let maybe_name = tag_to_name.get(&key.key_type);

            let mut enum_value_strings = Vec::new();
            for enum_value in &key.enum_values {
                if let Some(enum_name) = enum_value_to_name.get(&(*enum_value as u32)) {
                    enum_value_strings.push(format!("{} ({})", enum_name, &enum_value).to_owned())
                }
            }
            let enum_value_string = enum_value_strings.join(",#");

            tracing::debug!(
                "{}Key +{} [{}] {}|{} [#{}]:",
                indentation,
                maybe_name.unwrap(),
                key.key_type,
                key.min,
                key.max,
                enum_value_string
            );

            let new_indentation = indentation.to_owned() + "  ";
            branch.debug_print_helper(
                tag_to_name,
                data_to_name,
                enum_value_to_name,
                new_indentation,
            )
        }
    }

    fn collect_all_values(&self, values: &mut Vec<i32>) {
        for (_, branch) in &self.branches {
            branch.collect_all_values(values);
        }
    }

    fn query_one_recursive(&self, tag: u32, values: &mut Vec<i32>) {
        for (key, branch) in &self.branches {
            if key.key_type == tag {
                for data in &branch.data {
                    values.push(data.data)
                }

                branch.query_one_recursive(tag, values);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TagDatabaseData {
    pub data: i32,
    pub weight: f32,
}

impl TagDatabaseData {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T) -> TagDatabaseData {
        let data = read_i32(reader);
        let weight = read_single(reader);
        TagDatabaseData { data, weight }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagDatabaseKey {
    pub key_type: u32,
    pub min: i32,
    pub max: i32,

    pub enum_values: Vec<u8>,
}

impl TagDatabaseKey {
    pub fn matches_query(&self, query: &TagQueryItem) -> bool {
        match query {
            TagQueryItem::KeyWithEnumValue(key, enum_value, _) => {
                self.key_type == *key && {
                    for value in &self.enum_values {
                        if value == enum_value {
                            return true;
                        }
                    }
                    false
                }
            }
            TagQueryItem::Key(key, _optional) => self.key_type == *key,
            TagQueryItem::KeyWithIntValue(key, val, _optional) => {
                self.key_type == *key && self.min <= *val && self.max >= *val
            }
        }
    }

    pub fn read<T: io::Seek + io::Read>(reader: &mut T) -> TagDatabaseKey {
        let key_type = read_u32(reader);
        let min = read_i32(reader);
        let max = read_i32(reader);

        let mut enum_values = Vec::new();
        let bytes0 = min.to_le_bytes();
        let bytes1 = max.to_le_bytes();

        for i in 0..4 {
            if bytes0[i] != 255 {
                enum_values.push(bytes0[i])
            }

            if bytes1[i] != 255 {
                enum_values.push(bytes1[i])
            }
        }

        TagDatabaseKey {
            key_type,
            min,
            max,
            enum_values,
        }
    }
}
