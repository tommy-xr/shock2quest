use std::path::PathBuf;
use crate::database::EntityDatabase;
use crate::error::{EntityInspectorError, Result};
use crate::formatters::{format_template_info, format_template_list, format_database_info};
use crate::query::{TemplateQuery, parse_template_range};
use crate::ExportFormat;

pub fn inspect_command(
    file: PathBuf,
    template: Option<i32>,
    name: Option<String>,
    show_properties: bool,
) -> Result<()> {
    tracing::info!("Loading database from: {}", file.display());
    let database = EntityDatabase::from_file(&file)?;

    println!("{}", format_database_info(&database));

    if let Some(template_id) = template {
        let properties = if show_properties {
            database.get_entity_by_id(template_id)
        } else {
            None
        };

        println!("{}", format_template_info(template_id, properties));

    } else if let Some(entity_name) = name {
        let matching_ids = database.find_entities_by_name(&entity_name);
        if matching_ids.is_empty() {
            return Err(EntityInspectorError::EntityNotFound { name: entity_name });
        }

        for template_id in matching_ids {
            let properties = if show_properties {
                database.get_entity_by_id(template_id)
            } else {
                None
            };
            println!("{}", format_template_info(template_id, properties));
        }
    } else {
        // Show overview
        let all_ids = database.get_all_template_ids();
        println!("{}", format_template_list(&all_ids[..std::cmp::min(20, all_ids.len())]));
        if all_ids.len() > 20 {
            println!("... and {} more templates", all_ids.len() - 20);
        }
    }

    Ok(())
}

pub fn hierarchy_command(
    file: PathBuf,
    template: Option<i32>,
    roots: bool,
    show_properties: bool,
) -> Result<()> {
    tracing::info!("Loading database from: {}", file.display());
    let database = EntityDatabase::from_file(&file)?;

    println!("{}", format_database_info(&database));

    if roots {
        println!("Hierarchy analysis not yet fully implemented - showing basic template list:");
        let all_ids = database.get_all_template_ids();
        println!("{}", format_template_list(&all_ids));
    } else if let Some(template_id) = template {
        println!("Hierarchy for template {}:", template_id);
        println!("  (Hierarchy traversal not yet implemented)");

        if show_properties {
            let properties = database.get_entity_by_id(template_id);
            println!("{}", format_template_info(template_id, properties));
        }
    } else {
        return Err(EntityInspectorError::ValidationError {
            message: "Either --template or --roots must be specified".to_string(),
        });
    }

    Ok(())
}

pub fn query_command(
    file: PathBuf,
    property: Option<String>,
    has_property: Option<String>,
    template_range: Option<String>,
) -> Result<()> {
    tracing::info!("Loading database from: {}", file.display());
    let database = EntityDatabase::from_file(&file)?;

    println!("{}", format_database_info(&database));

    let mut query = TemplateQuery::new();

    if let Some(range_str) = template_range {
        let range = parse_template_range(&range_str)?;
        query = query.with_range(range);
    }

    let results = query.execute(&database)?;

    if property.is_some() || has_property.is_some() {
        println!("Property filtering not yet fully implemented - showing template matches:");
    }

    println!("{}", format_template_list(&results));

    Ok(())
}

pub fn export_command(
    file: PathBuf,
    format: ExportFormat,
    output: Option<PathBuf>,
    properties: bool,
) -> Result<()> {
    tracing::info!("Loading database from: {}", file.display());
    let database = EntityDatabase::from_file(&file)?;

    let all_ids = database.get_all_template_ids();

    match format {
        ExportFormat::Json => {
            let json_data = serde_json::json!({
                "file": database.file_path,
                "file_type": format!("{:?}", database.file_type),
                "entity_count": database.entity_count(),
                "template_ids": all_ids
            });

            let json_string = serde_json::to_string_pretty(&json_data)
                .map_err(|e| EntityInspectorError::ExportError {
                    message: e.to_string(),
                })?;

            if let Some(output_path) = output {
                std::fs::write(output_path, json_string)?;
            } else {
                println!("{}", json_string);
            }
        }
        ExportFormat::Csv => {
            let output_text = if properties {
                "Template ID,Property Count\n".to_string()
                    + &all_ids
                        .iter()
                        .map(|&id| {
                            let prop_count = database
                                .get_entity_by_id(id)
                                .map(|p| p.len())
                                .unwrap_or(0);
                            format!("{},{}", id, prop_count)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
            } else {
                "Template ID\n".to_string() + &all_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join("\n")
            };

            if let Some(output_path) = output {
                std::fs::write(output_path, output_text)?;
            } else {
                println!("{}", output_text);
            }
        }
        ExportFormat::Debug => {
            let debug_output = format!(
                "Debug Export:\n{}\nTemplate IDs: {:?}",
                format_database_info(&database),
                all_ids
            );

            if let Some(output_path) = output {
                std::fs::write(output_path, debug_output)?;
            } else {
                println!("{}", debug_output);
            }
        }
    }

    Ok(())
}

pub fn validate_command(
    file: PathBuf,
    check_inheritance: bool,
    check_links: bool,
) -> Result<()> {
    tracing::info!("Loading database from: {}", file.display());
    let database = EntityDatabase::from_file(&file)?;

    println!("{}", format_database_info(&database));
    println!("Validation Results:");

    if check_inheritance {
        println!("  Inheritance validation: Not yet implemented");
    }

    if check_links {
        println!("  Link validation: Not yet implemented");
    }

    if !check_inheritance && !check_links {
        println!("  Basic validation: File loaded successfully");
        println!("  Template count: {}", database.entity_count());
    }

    println!("Validation complete.");
    Ok(())
}