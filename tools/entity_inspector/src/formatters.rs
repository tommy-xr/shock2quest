use serde_json::Value;
use std::collections::HashMap;

pub fn format_template_info(template_id: i32, properties: Option<&Vec<std::rc::Rc<Box<dyn dark::properties::Property>>>>) -> String {
    let mut output = format!("Template ID: {}\n", template_id);

    if let Some(props) = properties {
        output.push_str(&format!("Properties: {} found\n", props.len()));

        // For now, just show count and type info since we need property inspection
        for (i, prop) in props.iter().enumerate() {
            output.push_str(&format!("  {}: [Property - type information not yet available]\n", i + 1));
        }
    } else {
        output.push_str("No properties found\n");
    }

    output
}

pub fn format_template_list(template_ids: &[i32]) -> String {
    let mut output = format!("Found {} templates:\n", template_ids.len());

    for &id in template_ids {
        output.push_str(&format!("  Template {}\n", id));
    }

    output
}

pub fn format_hierarchy_tree(template_id: i32, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    format!("{}Template {}\n", indent, template_id)
}

pub fn format_json_export(data: &HashMap<String, Value>) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(data)
}

pub fn format_database_info(database: &crate::database::EntityDatabase) -> String {
    format!(
        "Database Info:\n  File: {}\n  Type: {:?}\n  Entities: {}\n",
        database.file_path,
        database.file_type,
        database.entity_count()
    )
}