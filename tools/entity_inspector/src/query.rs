use std::ops::Range;
use crate::database::EntityDatabase;
use crate::error::{EntityInspectorError, Result};

pub struct TemplateQuery {
    pub id_filter: Option<i32>,
    pub name_filter: Option<String>,
    pub range_filter: Option<Range<i32>>,
}

impl TemplateQuery {
    pub fn new() -> Self {
        Self {
            id_filter: None,
            name_filter: None,
            range_filter: None,
        }
    }

    pub fn with_id(mut self, id: i32) -> Self {
        self.id_filter = Some(id);
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name_filter = Some(name);
        self
    }

    pub fn with_range(mut self, range: Range<i32>) -> Self {
        self.range_filter = Some(range);
        self
    }

    pub fn execute(&self, database: &EntityDatabase) -> Result<Vec<i32>> {
        let mut results = database.get_all_template_ids();

        if let Some(id) = self.id_filter {
            results.retain(|&template_id| template_id == id);
        }

        if let Some(ref name) = self.name_filter {
            let name_matches = database.find_entities_by_name(name);
            results.retain(|template_id| name_matches.contains(template_id));
        }

        if let Some(ref range) = self.range_filter {
            results.retain(|&template_id| range.contains(&template_id));
        }

        results.sort();
        Ok(results)
    }
}

pub fn parse_template_range(range_str: &str) -> Result<Range<i32>> {
    let parts: Vec<&str> = range_str.split("..").collect();
    if parts.len() != 2 {
        return Err(EntityInspectorError::InvalidTemplateRange {
            range: range_str.to_string(),
        });
    }

    let start = parts[0].parse::<i32>().map_err(|_| {
        EntityInspectorError::InvalidTemplateRange {
            range: range_str.to_string(),
        }
    })?;

    let end = parts[1].parse::<i32>().map_err(|_| {
        EntityInspectorError::InvalidTemplateRange {
            range: range_str.to_string(),
        }
    })?;

    Ok(start..end)
}