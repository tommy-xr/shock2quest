use std::{collections::HashMap, io};

use crate::ss2_common;

#[derive(Clone, Debug)]
pub struct NameMap {
    pub name_to_index: HashMap<String, u32>,
    pub index_to_name: HashMap<u32, String>,
}

impl NameMap {
    pub fn get_index(&self, name: &str) -> Option<u32> {
        self.name_to_index.get(name).copied()
    }

    pub fn get_name(&self, index: u32) -> Option<&String> {
        self.index_to_name.get(&index)
    }

    pub fn count(&self) -> usize {
        self.name_to_index.len()
    }

    pub fn entries(&self) -> Vec<(u32, String)> {
        let mut entries = self
            .index_to_name
            .iter()
            .map(|(index, name)| (*index, name.clone()))
            .collect::<Vec<_>>();

        entries.sort_by_key(|(index, _)| *index);
        entries
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> NameMap {
        let _upper_bound = ss2_common::read_i32(reader);
        let _lower_bound = ss2_common::read_i32(reader);
        let size = ss2_common::read_u32(reader);

        let mut name_to_index = HashMap::new();
        let mut index_to_name = HashMap::new();

        for i in 0..size {
            let char = ss2_common::read_char(reader);

            if char == '+' {
                let name = ss2_common::read_string_with_size(reader, 16);
                name_to_index.insert(name.to_ascii_lowercase().to_owned(), i);
                index_to_name.insert(i, name.to_ascii_lowercase().to_owned());
            }
        }

        NameMap {
            name_to_index,
            index_to_name,
        }
    }
}
