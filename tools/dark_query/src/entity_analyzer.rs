use dark::{
    properties::{
        Link, PropObjName, PropObjShortName, PropScripts, PropSymName, PropTemplateId, Property,
    },
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use glob::Pattern;
use shipyard::{Get, View, World};
use std::{collections::HashMap, rc::Rc};

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
    pub link_types: Vec<String>,
    pub script_names: Vec<String>,
    pub matched_items: Vec<String>, // What actually matched the filter
}

#[derive(Debug, Clone)]
pub struct FilterCriteria {
    pub only_unparsed: bool,
    pub property_filter: Option<String>,
}

/// Extract name properties by creating a temporary world and reading components
pub fn extract_names_public(properties: &[Rc<Box<dyn Property>>]) -> EntityNames {
    extract_names(properties)
}

/// Extract names with inheritance support - walks up the hierarchy to find names
pub fn extract_names_with_inheritance(
    entity_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> EntityNames {
    // First try direct properties
    if let Some(properties) = entity_info.entity_to_properties.get(&entity_id) {
        let direct_names = extract_names(properties);
        if direct_names.sym_name.is_some()
            || direct_names.obj_name.is_some()
            || direct_names.obj_short_name.is_some()
        {
            return direct_names;
        }
    }

    // If no direct names, walk up the inheritance hierarchy (from most specific to most general)
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let ancestors = ss2_entity_info::get_ancestors(hierarchy, &entity_id);

    // Walk from most specific to most general (reverse order)
    for ancestor_id in ancestors.iter().rev() {
        if let Some(properties) = entity_info.entity_to_properties.get(ancestor_id) {
            let ancestor_names = extract_names(properties);
            if ancestor_names.sym_name.is_some()
                || ancestor_names.obj_name.is_some()
                || ancestor_names.obj_short_name.is_some()
            {
                return ancestor_names;
            }
        }
    }

    // No names found in hierarchy
    EntityNames {
        sym_name: None,
        obj_name: None,
        obj_short_name: None,
    }
}

/// Extract template ID with inheritance support - walks up the hierarchy to find template ID
pub fn extract_template_id_with_inheritance(
    entity_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Option<i32> {
    // First try direct properties
    if let Some(properties) = entity_info.entity_to_properties.get(&entity_id) {
        let direct_template_id = extract_template_id(properties);
        if direct_template_id.is_some() {
            return direct_template_id;
        }
    }

    // If no direct template ID, use the most specific ancestor as the template ID
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let ancestors = ss2_entity_info::get_ancestors(hierarchy, &entity_id);

    // Return the most specific ancestor (last in the list) as the template ID
    ancestors.last().copied()
}

fn extract_names(properties: &[Rc<Box<dyn Property>>]) -> EntityNames {
    let mut world = World::new();
    let entity = world.add_entity(());

    // Initialize all properties into the world
    for prop in properties {
        prop.initialize(&mut world, entity);
    }

    // Now read the components
    let mut sym_name = None;
    let mut obj_name = None;
    let mut obj_short_name = None;

    // Try to get each name property
    if let Ok(view) = world.borrow::<View<PropSymName>>() {
        if let Ok(prop) = view.get(entity) {
            sym_name = Some(prop.0.clone());
        }
    }

    if let Ok(view) = world.borrow::<View<PropObjName>>() {
        if let Ok(prop) = view.get(entity) {
            obj_name = Some(prop.0.clone());
        }
    }

    if let Ok(view) = world.borrow::<View<PropObjShortName>>() {
        if let Ok(prop) = view.get(entity) {
            obj_short_name = Some(prop.0.clone());
        }
    }

    EntityNames {
        sym_name,
        obj_name,
        obj_short_name,
    }
}

fn extract_template_id(properties: &[Rc<Box<dyn Property>>]) -> Option<i32> {
    let mut world = World::new();
    let entity = world.add_entity(());

    // Initialize all properties into the world
    for prop in properties {
        prop.initialize(&mut world, entity);
    }

    // Try to get template ID
    if let Ok(view) = world.borrow::<View<PropTemplateId>>() {
        if let Ok(prop) = view.get(entity) {
            return Some(prop.template_id);
        }
    }

    None
}

/// Get a set of all known P$ property names from the property definitions
fn get_known_p_property_names() -> std::collections::HashSet<String> {
    let mut p_names = std::collections::HashSet::new();

    // Get the property definitions - we only need the names
    let (properties, _links, _links_with_data) =
        dark::properties::get::<std::io::Cursor<Vec<u8>>>();

    for prop_def in properties {
        let original_name = prop_def.name();
        if original_name.starts_with("P$") {
            p_names.insert(original_name);
        }
    }

    p_names
}

/// Check if a filter pattern could match a property name in either cleaned or P$ form
fn property_matches_pattern(
    clean_prop_name: &str,
    pattern: &str,
    known_p_names: &std::collections::HashSet<String>,
) -> bool {
    // Check if the pattern matches the clean name directly
    if clean_prop_name.contains(pattern) {
        return true;
    }

    // Check if the pattern is a P$ name and matches this property
    if pattern.starts_with("P$") && known_p_names.contains(pattern) {
        // Simple heuristic: check if the pattern could correspond to this clean name
        // For example, P$FrobInfo should match PropFrobInfo
        let pattern_part = &pattern[2..]; // Remove "P$"
        if clean_prop_name.starts_with("Prop") && clean_prop_name[4..].contains(pattern_part) {
            return true;
        }
    }

    false
}

/// Check if a glob pattern could match a property name in either cleaned or P$ form
fn property_matches_glob(
    clean_prop_name: &str,
    glob: &Pattern,
    known_p_names: &std::collections::HashSet<String>,
) -> bool {
    // Check if the glob matches the clean name directly
    if glob.matches(clean_prop_name) {
        return true;
    }

    // Check if any P$ name would match and correspond to this property
    for p_name in known_p_names {
        if glob.matches(p_name) {
            // Simple heuristic: check if this P$ name could correspond to the clean name
            let pattern_part = &p_name[2..]; // Remove "P$"
            if clean_prop_name.starts_with("Prop") && clean_prop_name[4..].contains(pattern_part) {
                return true;
            }
        }
    }

    false
}

/// Get property type names from the property list
fn get_property_names(properties: &[Rc<Box<dyn Property>>]) -> Vec<String> {
    properties
        .iter()
        .map(|prop| {
            // Try to extract the property type name from the debug representation
            let debug_str = format!("{:?}", prop.as_ref());

            // Handle WrappedProperty format
            let prop_str = if debug_str.starts_with("WrappedProperty { inner_property: ") {
                if let Some(start) = debug_str.find("inner_property: ") {
                    let remaining = &debug_str[start + 16..];
                    if let Some(end) = remaining.find(", accumulator:") {
                        remaining[..end].to_string()
                    } else {
                        remaining.to_string()
                    }
                } else {
                    debug_str.clone()
                }
            } else {
                debug_str.clone()
            };

            // Extract just the property type name (before the opening parenthesis or space/brace)
            if prop_str.starts_with("Prop") {
                // Split on multiple possible delimiters: '(', ' ', '{'
                prop_str
                    .split(['(', ' ', '{'])
                    .next()
                    .unwrap_or(&prop_str)
                    .to_string()
            } else {
                prop_str
            }
        })
        .collect()
}

/// Get property type names with inheritance support - walks up the hierarchy to collect all properties
fn get_property_names_with_inheritance(
    entity_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Vec<String> {
    let mut all_properties = Vec::new();

    // Collect direct properties
    if let Some(properties) = entity_info.entity_to_properties.get(&entity_id) {
        all_properties.extend(get_property_names(properties));
    }

    // Walk up the inheritance hierarchy
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let ancestors = ss2_entity_info::get_ancestors(hierarchy, &entity_id);

    for ancestor_id in ancestors.iter().rev() {
        if let Some(properties) = entity_info.entity_to_properties.get(ancestor_id) {
            all_properties.extend(get_property_names(properties));
        }
    }

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    all_properties
        .into_iter()
        .filter(|prop| seen.insert(prop.clone()))
        .collect()
}

/// Get link type names from the entity's links
fn get_link_types(entity_id: i32, entity_info: &SystemShock2EntityInfo) -> Vec<String> {
    if let Some(template_links) = entity_info.template_to_links.get(&entity_id) {
        let link_types: Vec<String> = template_links
            .to_links
            .iter()
            .map(|link| match &link.link {
                Link::SwitchLink => "SwitchLink".to_string(),
                Link::Contains(_) => "Contains".to_string(),
                Link::Flinderize(_) => "Flinderize".to_string(),
                Link::AIWatchObj(_) => "AIWatchObj".to_string(),
                Link::Projectile(_) => "Projectile".to_string(),
                Link::Corpse(_) => "Corpse".to_string(),
                Link::AIProjectile(_) => "AIProjectile".to_string(),
                Link::AIRangedWeapon => "AIRangedWeapon".to_string(),
                Link::GunFlash(_) => "GunFlash".to_string(),
                Link::LandingPoint => "LandingPoint".to_string(),
                Link::Replicator => "Replicator".to_string(),
                Link::MissSpang => "MissSpang".to_string(),
                Link::TPathInit => "TPathInit".to_string(),
                Link::TPath(_) => "TPath".to_string(),
            })
            .collect();

        link_types
    } else {
        vec![]
    }
}

/// Get link type names with inheritance support - walks up the hierarchy to collect all link types
fn get_link_types_with_inheritance(
    entity_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Vec<String> {
    let mut all_link_types = Vec::new();

    // Collect direct link types
    all_link_types.extend(get_link_types(entity_id, entity_info));

    // Walk up the inheritance hierarchy
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let ancestors = ss2_entity_info::get_ancestors(hierarchy, &entity_id);

    for ancestor_id in ancestors.iter().rev() {
        all_link_types.extend(get_link_types(*ancestor_id, entity_info));
    }

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    all_link_types
        .into_iter()
        .filter(|link_type| seen.insert(link_type.clone()))
        .collect()
}

/// Extract script names from properties
fn get_script_names(properties: &[Rc<Box<dyn Property>>]) -> Vec<String> {
    // Create a temporary world to extract script information
    let mut world = World::new();
    let entity = world.add_entity(());

    // Initialize all properties into the world
    for prop in properties {
        prop.initialize(&mut world, entity);
    }

    // Try to get script property
    if let Ok(view) = world.borrow::<View<PropScripts>>() {
        if let Ok(scripts_prop) = view.get(entity) {
            return scripts_prop.scripts.clone();
        }
    }

    vec![]
}

/// Get script names with inheritance support - walks up the hierarchy to collect all scripts
fn get_script_names_with_inheritance(
    entity_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Vec<String> {
    let mut all_scripts = Vec::new();

    // Collect direct scripts
    if let Some(properties) = entity_info.entity_to_properties.get(&entity_id) {
        all_scripts.extend(get_script_names(properties));
    }

    // Walk up the inheritance hierarchy
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let ancestors = ss2_entity_info::get_ancestors(hierarchy, &entity_id);

    for ancestor_id in ancestors.iter().rev() {
        if let Some(properties) = entity_info.entity_to_properties.get(ancestor_id) {
            all_scripts.extend(get_script_names(properties));
        }
    }

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    all_scripts
        .into_iter()
        .filter(|script| seen.insert(script.clone()))
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

        let names = extract_names_with_inheritance(*entity_id, entity_info);
        let template_id = extract_template_id_with_inheritance(*entity_id, entity_info);

        let unparsed_properties = entity_unparsed_props
            .get(entity_id)
            .cloned()
            .unwrap_or_default();

        let has_unparsed_data = !unparsed_properties.is_empty()
            || entity_info.unparsed_links.values().any(|links| {
                links
                    .iter()
                    .any(|link| link.src == *entity_id || link.dest == *entity_id)
            });

        let link_count = entity_info
            .template_to_links
            .get(entity_id)
            .map(|links| links.to_links.len())
            .unwrap_or(0);

        let parsed_properties = get_property_names_with_inheritance(*entity_id, entity_info);
        let link_types = get_link_types_with_inheritance(*entity_id, entity_info);
        let script_names = get_script_names_with_inheritance(*entity_id, entity_info);

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
            link_types,
            script_names,
            matched_items: Vec::new(), // Initially empty, populated during filtering
        });
    }

    // Sort by ID for consistent output
    summaries.sort_by_key(|s| s.id);
    summaries
}

/// Apply filters to entity summaries
pub fn filter_entities(
    summaries: &[EntitySummary],
    criteria: &FilterCriteria,
) -> Vec<EntitySummary> {
    let mut filtered = summaries.to_vec();

    // Get known P$ property names for dual-name support
    let known_p_names = get_known_p_property_names();

    // Apply unparsed filter
    if criteria.only_unparsed {
        filtered.retain(|summary| summary.has_unparsed_data);
    }

    // Apply property filter
    if let Some(filter_pattern) = &criteria.property_filter {
        // Handle property value matching (e.g., "P$SymName:*Robot*")
        if let Some((prop_name, value_pattern)) = filter_pattern.split_once(':') {
            let value_glob = Pattern::new(&value_pattern.to_lowercase()).ok();
            filtered.retain_mut(|summary| {
                let mut matches = false;

                match prop_name {
                    "P$SymName" => {
                        if let Some(sym_name) = &summary.names.sym_name {
                            if value_glob.as_ref().map_or(false, |g| g.matches(&sym_name.to_lowercase())) {
                                summary.matched_items = vec![format!("P$SymName:{}", sym_name)];
                                matches = true;
                            }
                        }
                    }
                    "P$ObjName" => {
                        if let Some(obj_name) = &summary.names.obj_name {
                            if value_glob.as_ref().map_or(false, |g| g.matches(&obj_name.to_lowercase())) {
                                summary.matched_items = vec![format!("P$ObjName:{}", obj_name)];
                                matches = true;
                            }
                        }
                    }
                    "P$ObjShortName" => {
                        if let Some(short_name) = &summary.names.obj_short_name {
                            if value_glob.as_ref().map_or(false, |g| g.matches(&short_name.to_lowercase())) {
                                summary.matched_items =
                                    vec![format!("P$ObjShortName:{}", short_name)];
                                matches = true;
                            }
                        }
                    }
                    "P$Scripts" => {
                        // Check if any script names match the pattern (case-insensitive)
                        let matching_scripts: Vec<String> = if let Some(glob) = &value_glob {
                            summary
                                .script_names
                                .iter()
                                .filter(|script| glob.matches(&script.to_lowercase()))
                                .map(|script| format!("P$Scripts:{}", script))
                                .collect()
                        } else {
                            let pattern_lower = value_pattern.to_lowercase();
                            summary
                                .script_names
                                .iter()
                                .filter(|script| script.to_lowercase().contains(&pattern_lower))
                                .map(|script| format!("P$Scripts:{}", script))
                                .collect()
                        };

                        if !matching_scripts.is_empty() {
                            summary.matched_items = matching_scripts;
                            matches = true;
                        }
                    }
                    _ => {
                        // For other properties, just check if the property name exists
                        let prop_exists = summary
                            .parsed_properties
                            .iter()
                            .any(|p| p.contains(prop_name.trim_start_matches("P$")));
                        if prop_exists {
                            summary.matched_items = vec![prop_name.to_string()];
                            matches = true;
                        }
                    }
                }

                matches
            });
        } else {
            // Check for link filtering (L$LinkType)
            if filter_pattern.starts_with("L$") {
                let link_pattern = &filter_pattern[2..]; // Remove "L$" prefix
                let link_glob = Pattern::new(link_pattern).ok();
                filtered.retain_mut(|summary| {
                    let mut matches = Vec::new();

                    if let Some(glob) = &link_glob {
                        for link_type in &summary.link_types {
                            if glob.matches(link_type) {
                                matches.push(format!("L${}", link_type));
                            }
                        }
                    } else {
                        for link_type in &summary.link_types {
                            if link_type.contains(link_pattern) {
                                matches.push(format!("L${}", link_type));
                            }
                        }
                    }

                    if !matches.is_empty() {
                        summary.matched_items = matches;
                        true
                    } else {
                        false
                    }
                });
            }
            // Check for script filtering (S$ScriptName)
            else if filter_pattern.starts_with("S$") {
                let script_pattern = &filter_pattern[2..]; // Remove "S$" prefix
                let script_glob = Pattern::new(&script_pattern.to_lowercase()).ok();
                filtered.retain_mut(|summary| {
                    let mut matches = Vec::new();

                    if let Some(glob) = &script_glob {
                        for script in &summary.script_names {
                            if glob.matches(&script.to_lowercase()) {
                                matches.push(format!("S${}", script));
                            }
                        }
                    } else {
                        let pattern_lower = script_pattern.to_lowercase();
                        for script in &summary.script_names {
                            if script.to_lowercase().contains(&pattern_lower) {
                                matches.push(format!("S${}", script));
                            }
                        }
                    }

                    if !matches.is_empty() {
                        summary.matched_items = matches;
                        true
                    } else {
                        false
                    }
                });
            }
            // No prefix: search across all categories (properties, links, scripts)
            else {
                let glob = Pattern::new(filter_pattern).ok();
                filtered.retain_mut(|summary| {
                    let mut matches = Vec::new();

                    if let Some(g) = &glob {
                        // Check properties (check both cleaned and original P$ names)
                        for prop in &summary.parsed_properties {
                            if property_matches_glob(prop, g, &known_p_names) {
                                matches.push(prop.clone()); // Always show the clean property name
                            }
                        }
                        for prop in &summary.unparsed_properties {
                            if g.matches(prop) {
                                matches.push(prop.clone());
                            }
                        }
                        // Check link types
                        for link_type in &summary.link_types {
                            if g.matches(link_type) {
                                matches.push(format!("L${}", link_type));
                            }
                        }
                        // Check script names (case-insensitive)
                        for script in &summary.script_names {
                            if g.matches(&script.to_lowercase()) {
                                matches.push(format!("S${}", script));
                            }
                        }
                    } else {
                        // Fallback to simple contains matching across all categories
                        for prop in &summary.parsed_properties {
                            if property_matches_pattern(prop, filter_pattern, &known_p_names) {
                                matches.push(prop.clone()); // Always show the clean property name
                            }
                        }
                        for prop in &summary.unparsed_properties {
                            if prop.contains(filter_pattern) {
                                matches.push(prop.clone());
                            }
                        }
                        for link_type in &summary.link_types {
                            if link_type.contains(filter_pattern) {
                                matches.push(format!("L${}", link_type));
                            }
                        }
                        let pattern_lower = filter_pattern.to_lowercase();
                        for script in &summary.script_names {
                            if script.to_lowercase().contains(&pattern_lower) {
                                matches.push(format!("S${}", script));
                            }
                        }
                    }

                    if !matches.is_empty() {
                        summary.matched_items = matches;
                        true
                    } else {
                        false
                    }
                });
            }
        }
    }

    filtered
}
