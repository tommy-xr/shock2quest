use std::{collections::HashMap, rc::Rc};
use glob::Pattern;
use dark::{
    ss2_entity_info::SystemShock2EntityInfo,
    properties::{Property},
};

#[derive(Debug, Clone)]
pub enum EntityType {
    Template,
    Entity,
}

#[derive(Debug, Clone)]
pub struct EntityNames {
    pub sym_name: Option<String>,
    pub obj_name: Option<String>,
    pub obj_short_name: Option<String>,
}

impl EntityNames {
    pub fn primary_name(&self) -> Option<&String> {
        self.sym_name.as_ref()
            .or(self.obj_name.as_ref())
            .or(self.obj_short_name.as_ref())
    }

    pub fn display_names(&self) -> String {
        let mut parts = Vec::new();

        if let Some(sym) = &self.sym_name {
            parts.push(format!("sym:{}", sym));
        }
        if let Some(obj) = &self.obj_name {
            parts.push(format!("obj:{}", obj));
        }
        if let Some(short) = &self.obj_short_name {
            parts.push(format!("short:{}", short));
        }

        if parts.is_empty() {
            "<no name>".to_string()
        } else {
            parts.join(", ")
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntitySummary {
    pub id: i32,
    pub entity_type: EntityType,
    pub names: EntityNames,
    pub template_id: Option<i32>,
    pub has_unparsed_data: bool,
    pub property_count: usize,
    pub link_count: usize,
    pub parsed_properties: Vec<String>,
    pub unparsed_properties: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FilterCriteria {
    pub only_unparsed: bool,
    pub property_filter: Option<String>,
}

/// Extract name properties from a property list
fn extract_names(_properties: &[Rc<Box<dyn Property>>]) -> EntityNames {
    // TODO: For now, return empty names until we figure out how to properly
    // access the property data from the trait objects
    EntityNames {
        sym_name: None,
        obj_name: None,
        obj_short_name: None,
    }
}

/// Extract template ID from properties
fn extract_template_id(_properties: &[Rc<Box<dyn Property>>]) -> Option<i32> {
    // TODO: For now, return None until we figure out how to properly
    // access the property data from the trait objects
    None
}

/// Get property type names from the property list
fn get_property_names(properties: &[Rc<Box<dyn Property>>]) -> Vec<String> {
    properties.iter()
        .map(|prop| {
            // Try to extract the property type name from the debug representation
            let debug_str = format!("{:?}", prop.as_ref());
            if debug_str.starts_with("Prop") {
                // Extract just the property name part
                debug_str.split('(').next().unwrap_or(&debug_str).to_string()
            } else {
                debug_str
            }
        })
        .collect()
}

/// Analyze all entities and create summaries
pub fn analyze_entities(entity_info: &SystemShock2EntityInfo) -> Vec<EntitySummary> {
    let mut summaries = Vec::new();

    // Collect unparsed property info
    let mut entity_unparsed_props: HashMap<i32, Vec<String>> = HashMap::new();
    for (prop_name, unparsed_list) in &entity_info.unparsed_properties {
        for unparsed_prop in unparsed_list {
            entity_unparsed_props
                .entry(unparsed_prop.entity_id)
                .or_insert_with(Vec::new)
                .push(prop_name.clone());
        }
    }

    // Process each entity/template
    for (entity_id, properties) in &entity_info.entity_to_properties {
        let entity_type = if *entity_id < 0 {
            EntityType::Template
        } else {
            EntityType::Entity
        };

        let names = extract_names(properties);
        let template_id = extract_template_id(properties);

        let unparsed_properties = entity_unparsed_props
            .get(entity_id)
            .cloned()
            .unwrap_or_default();

        let has_unparsed_data = !unparsed_properties.is_empty() ||
            entity_info.unparsed_links.values().any(|links|
                links.iter().any(|link| link.src == *entity_id || link.dest == *entity_id)
            );

        let link_count = entity_info.template_to_links
            .get(entity_id)
            .map(|links| links.to_links.len())
            .unwrap_or(0);

        let parsed_properties = get_property_names(properties);

        summaries.push(EntitySummary {
            id: *entity_id,
            entity_type,
            names,
            template_id,
            has_unparsed_data,
            property_count: properties.len(),
            link_count,
            parsed_properties,
            unparsed_properties,
        });
    }

    // Sort by ID for consistent output
    summaries.sort_by_key(|s| s.id);
    summaries
}

/// Apply filters to entity summaries
pub fn filter_entities(summaries: &[EntitySummary], criteria: &FilterCriteria) -> Vec<EntitySummary> {
    let mut filtered = summaries.to_vec();

    // Apply unparsed filter
    if criteria.only_unparsed {
        filtered.retain(|summary| summary.has_unparsed_data);
    }

    // Apply property filter
    if let Some(filter_pattern) = &criteria.property_filter {
        // Handle property value matching (e.g., "P$SymName:*Robot*")
        if let Some((prop_name, value_pattern)) = filter_pattern.split_once(':') {
            let value_glob = Pattern::new(value_pattern).ok();
            filtered.retain(|summary| {
                match prop_name {
                    "P$SymName" => {
                        if let Some(sym_name) = &summary.names.sym_name {
                            value_glob.as_ref().map_or(false, |g| g.matches(sym_name))
                        } else {
                            false
                        }
                    }
                    "P$ObjName" => {
                        if let Some(obj_name) = &summary.names.obj_name {
                            value_glob.as_ref().map_or(false, |g| g.matches(obj_name))
                        } else {
                            false
                        }
                    }
                    "P$ObjShortName" => {
                        if let Some(short_name) = &summary.names.obj_short_name {
                            value_glob.as_ref().map_or(false, |g| g.matches(short_name))
                        } else {
                            false
                        }
                    }
                    _ => {
                        // Check if the property name matches and any value exists
                        summary.parsed_properties.iter().any(|p| p.contains(prop_name.trim_start_matches("P$")))
                    }
                }
            });
        } else {
            // Property name matching only
            let prop_glob = Pattern::new(filter_pattern).ok();
            filtered.retain(|summary| {
                if let Some(glob) = &prop_glob {
                    summary.parsed_properties.iter().any(|prop| glob.matches(prop)) ||
                    summary.unparsed_properties.iter().any(|prop| glob.matches(prop))
                } else {
                    // Fallback to simple contains matching
                    summary.parsed_properties.iter().any(|prop| prop.contains(filter_pattern)) ||
                    summary.unparsed_properties.iter().any(|prop| prop.contains(filter_pattern))
                }
            });
        }
    }

    filtered
}