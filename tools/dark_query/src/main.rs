use anyhow::Result;
use clap::{Parser, Subcommand};
use shock2vr::zip_asset_path::ZipAssetPath;
use tracing::info;

mod data_loader;
mod entity_analyzer;
mod motion_analyzer;

use data_loader::load_entity_data;
use entity_analyzer::{analyze_entities, filter_entities, EntityType, FilterCriteria};
use motion_analyzer::MotionAnalyzer;

#[derive(Parser)]
#[command(name = "dark_query")]
#[command(about = "Query game data from System Shock 2 files")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Query entities and templates from gamesys and optional mission
    Entities {
        /// Mission file to load (loads shock2.gam by default, or shock2.gam + mission if specified)
        mission: Option<String>,

        /// Specific entity or template ID to show details for (positive for entities, negative for templates)
        id: Option<i32>,

        /// Filter by property or link name (supports wildcards)
        #[arg(long)]
        filter: Option<String>,

        /// Show only entities/templates with unparsed properties or links
        #[arg(long)]
        only_unparsed: bool,

        /// Limit the number of results displayed
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Query motion database for animations by creature type and tags
    Motion {
        /// Creature type (numeric ID like 0, 1, 2) or name (like "human", "midwife")
        creature_type: String,

        /// Tags to query (e.g., "+locomote", "+midwife", "+cs:184")
        tags: Vec<String>,

        /// Limit the number of results displayed
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Query map chunk data from interface files
    Maps {
        /// Mission name to load map data for (e.g., "MEDSCI1", "MEDSCI2")
        mission: String,
    },
}

fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    info!("Starting dark_query");

    match cli.command {
        Commands::Entities {
            mission,
            id,
            filter,
            only_unparsed,
            limit,
        } => {
            if let Some(entity_id) = id {
                handle_show_command(mission.as_deref(), entity_id, filter.as_deref())?;
            } else {
                handle_list_command(mission.as_deref(), only_unparsed, filter.as_deref(), limit)?;
            }
        }
        Commands::Motion {
            creature_type,
            tags,
            limit,
        } => {
            handle_motion_command(&creature_type, &tags, limit)?;
        }
        Commands::Maps { mission } => {
            handle_maps_command(&mission)?;
        }
    }

    Ok(())
}

fn handle_list_command(
    mission: Option<&str>,
    only_unparsed: bool,
    filter: Option<&str>,
    limit: Option<usize>,
) -> Result<()> {
    info!("Loading entity data...");
    let entity_info = load_entity_data(mission)?;

    info!("Analyzing entities...");
    let summaries = analyze_entities(&entity_info);

    // Apply filters
    let criteria = FilterCriteria {
        only_unparsed,
        property_filter: filter.map(|s| s.to_string()),
    };
    let filtered_summaries = filter_entities(&summaries, &criteria);

    // No entity type filtering needed - show both templates and entities

    // Display results
    display_entity_list(&filtered_summaries, filter.is_some(), limit);

    Ok(())
}

fn display_entity_list(
    summaries: &[entity_analyzer::EntitySummary],
    show_filter_details: bool,
    limit: Option<usize>,
) {
    if summaries.is_empty() {
        println!("No entities found matching the criteria.");
        return;
    }

    // Apply limit if specified
    let display_summaries = if let Some(limit_count) = limit {
        &summaries[..summaries.len().min(limit_count)]
    } else {
        summaries
    };

    // Print header
    if show_filter_details {
        println!(
            "{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8} | Matched Items",
            "ID", "Type", "Names", "Template", "Props", "Links", "Unparsed"
        );
        println!(
            "{:-<8}-+-{:-<8}-+-{:-<40}-+-{:-<8}-+-{:-<5}-+-{:-<5}-+-{:-<8}-+{:-<20}",
            "", "", "", "", "", "", "", ""
        );
    } else {
        println!(
            "{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8}",
            "ID", "Type", "Names", "Template", "Props", "Links", "Unparsed"
        );
        println!(
            "{:-<8}-+-{:-<8}-+-{:-<40}-+-{:-<8}-+-{:-<5}-+-{:-<5}-+-{:-<8}",
            "", "", "", "", "", "", ""
        );
    }

    // Print entities
    for summary in display_summaries {
        let entity_type = match summary.entity_type {
            EntityType::Template => "Template",
            EntityType::Entity => "Entity",
        };

        let names_display = if summary.names.display_names().len() > 40 {
            format!("{}...", &summary.names.display_names()[..37])
        } else {
            summary.names.display_names()
        };

        let template_display = summary
            .template_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string());

        let unparsed_display = if summary.has_unparsed_data {
            "Yes"
        } else {
            "No"
        };

        if show_filter_details {
            let matched_items = summary.matched_items.join(", ");
            let matched_display = if matched_items.len() > 50 {
                format!("{}...", &matched_items[..47])
            } else {
                matched_items
            };

            println!(
                "{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8} | {}",
                summary.id,
                entity_type,
                names_display,
                template_display,
                summary.property_count,
                summary.link_count,
                unparsed_display,
                matched_display
            );
        } else {
            println!(
                "{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8}",
                summary.id,
                entity_type,
                names_display,
                template_display,
                summary.property_count,
                summary.link_count,
                unparsed_display
            );
        }
    }

    // Display count information
    if let Some(limit_count) = limit {
        if summaries.len() > limit_count {
            println!(
                "\nShowing {} of {} entities (limited)",
                display_summaries.len(),
                summaries.len()
            );
        } else {
            println!("\nTotal: {} entities", summaries.len());
        }
    } else {
        println!("\nTotal: {} entities", summaries.len());
    }
}

fn handle_show_command(mission: Option<&str>, entity_id: i32, _filter: Option<&str>) -> Result<()> {
    info!("Loading entity data...");
    let entity_info = load_entity_data(mission)?;

    // Find the specific entity
    if let Some(_properties) = entity_info.entity_to_properties.get(&entity_id) {
        let entity_type = if entity_id < 0 { "Template" } else { "Entity" };

        println!("=== {} {} ===", entity_type, entity_id);

        // Extract names with inheritance
        let inherited_names =
            entity_analyzer::extract_names_with_inheritance(entity_id, &entity_info);
        println!("Name: {}", inherited_names.display_names());

        // Extract template ID with inheritance
        let template_id =
            entity_analyzer::extract_template_id_with_inheritance(entity_id, &entity_info);
        if let Some(tid) = template_id {
            println!("Template: {}", tid);
        }

        println!();

        // Show links
        show_entity_links(entity_id, &entity_info);

        println!();

        // Show inheritance hierarchy as a tree
        show_inheritance_tree(entity_id, &entity_info);

        println!();

        // Show unparsed data for this entity
        show_unparsed_data(entity_id, &entity_info);
    } else {
        println!("Entity {} not found", entity_id);
    }

    Ok(())
}

fn show_inheritance_tree(
    entity_id: i32,
    entity_info: &dark::ss2_entity_info::SystemShock2EntityInfo,
) {
    println!("Inheritance Tree:");

    // Get the inheritance chain
    let hierarchy = dark::ss2_entity_info::get_hierarchy(entity_info);
    let ancestors = dark::ss2_entity_info::get_ancestors(hierarchy, &entity_id);

    // Build the full chain from most general to most specific
    let mut full_chain = ancestors.clone();
    full_chain.reverse(); // Now goes from most general to most specific
    full_chain.push(entity_id); // Add the entity itself at the end

    // Display each level in the tree
    for (depth, &current_id) in full_chain.iter().enumerate() {
        let indent = "  ".repeat(depth);

        // Get entity info
        let entity_type = if current_id < 0 { "Template" } else { "Entity" };
        let names = entity_analyzer::extract_names_public(
            entity_info
                .entity_to_properties
                .get(&current_id)
                .unwrap_or(&vec![]),
        );

        let name_display = if names.sym_name.is_some()
            || names.obj_name.is_some()
            || names.obj_short_name.is_some()
        {
            names.display_names()
        } else {
            "<no name>".to_string()
        };

        println!(
            "{}├─ {} {} ({})",
            indent, entity_type, current_id, name_display
        );

        // Show properties for this entity
        if let Some(properties) = entity_info.entity_to_properties.get(&current_id) {
            show_properties_for_entity(current_id, properties, depth + 1);
        }

        println!();
    }
}

fn show_properties_for_entity(
    _entity_id: i32,
    properties: &[std::rc::Rc<Box<dyn dark::properties::Property>>],
    depth: usize,
) {
    let indent = "  ".repeat(depth);

    println!("{}Properties ({}):", indent, properties.len());

    for (i, prop) in properties.iter().enumerate() {
        let prop_debug = format!("{:?}", prop);

        // Extract a more comprehensive property representation
        let prop_display = if prop_debug.starts_with("WrappedProperty { inner_property: ") {
            if let Some(start) = prop_debug.find("inner_property: ") {
                let remaining = &prop_debug[start + 16..];
                if let Some(end) = remaining.find(", accumulator:") {
                    remaining[..end].to_string()
                } else {
                    // Fallback to the full remaining string
                    remaining.to_string()
                }
            } else {
                prop_debug
            }
        } else {
            prop_debug
        };

        // Show full property display without truncation
        println!("{}  {}. {}", indent, i + 1, prop_display);
    }
}

fn show_entity_links(entity_id: i32, entity_info: &dark::ss2_entity_info::SystemShock2EntityInfo) {
    // Collect outgoing links
    let outgoing_links = if let Some(template_links) = entity_info.template_to_links.get(&entity_id)
    {
        &template_links.to_links
    } else {
        &vec![]
    };

    // Collect incoming links by scanning all entities
    let mut incoming_links = Vec::new();
    for (source_id, template_links) in &entity_info.template_to_links {
        for link in &template_links.to_links {
            if link.to_template_id == entity_id {
                incoming_links.push((source_id, link));
            }
        }
    }

    println!("Links:");

    // Show outgoing links
    println!("  Outgoing Links:");
    if outgoing_links.is_empty() {
        println!("    (none)");
    } else {
        for (i, link) in outgoing_links.iter().enumerate() {
            let target_names =
                entity_analyzer::extract_names_with_inheritance(link.to_template_id, entity_info);
            let target_display = if target_names.sym_name.is_some()
                || target_names.obj_name.is_some()
                || target_names.obj_short_name.is_some()
            {
                format!(" ({})", target_names.display_names())
            } else {
                "".to_string()
            };

            let link_type = format_link_type(&link.link);

            println!(
                "    {}. {} -> Entity {}{}",
                i + 1,
                link_type,
                link.to_template_id,
                target_display
            );
        }
    }

    // Show incoming links
    println!("  Incoming Links:");
    if incoming_links.is_empty() {
        println!("    (none)");
    } else {
        for (i, (source_id, link)) in incoming_links.iter().enumerate() {
            let source_names =
                entity_analyzer::extract_names_with_inheritance(**source_id, entity_info);
            let source_display = if source_names.sym_name.is_some()
                || source_names.obj_name.is_some()
                || source_names.obj_short_name.is_some()
            {
                format!(" ({})", source_names.display_names())
            } else {
                "".to_string()
            };

            let link_type = format_link_type(&link.link);

            println!(
                "    {}. Entity {}{} -> {} here",
                i + 1,
                source_id,
                source_display,
                link_type
            );
        }
    }
}

fn format_link_type(link: &dark::properties::Link) -> String {
    match link {
        dark::properties::Link::SwitchLink => "SwitchLink".to_string(),
        dark::properties::Link::Contains(_) => "Contains".to_string(),
        dark::properties::Link::Flinderize(_) => "Flinderize".to_string(),
        dark::properties::Link::AIWatchObj(_) => "AIWatchObj".to_string(),
        dark::properties::Link::Projectile(_) => "Projectile".to_string(),
        dark::properties::Link::Corpse(_) => "Corpse".to_string(),
        dark::properties::Link::AIProjectile(_) => "AIProjectile".to_string(),
        dark::properties::Link::AIRangedWeapon => "AIRangedWeapon".to_string(),
        dark::properties::Link::GunFlash(_) => "GunFlash".to_string(),
        dark::properties::Link::LandingPoint => "LandingPoint".to_string(),
        dark::properties::Link::Replicator => "Replicator".to_string(),
        dark::properties::Link::MissSpang => "MissSpang".to_string(),
        dark::properties::Link::TPathInit => "TPathInit".to_string(),
        dark::properties::Link::TPath(_) => "TPath".to_string(),
    }
}

fn handle_motion_command(creature_type: &str, tags: &[String], limit: Option<usize>) -> Result<()> {
    info!("Loading motion database...");
    let motion_analyzer = MotionAnalyzer::new()?;

    let creature_id = motion_analyzer.parse_creature_type(creature_type)?;

    if tags.is_empty() {
        motion_analyzer.list_all_tags_and_animations(creature_id, limit)?;
    } else {
        motion_analyzer.query_with_tags(creature_id, tags, limit)?;
    }

    Ok(())
}

fn handle_maps_command(mission: &str) -> Result<()> {
    info!("Loading map data for {}...", mission);

    // Use proper data root resolution from shock2vr
    let data_root = shock2vr::paths::data_root();
    let data_path = data_root.to_string_lossy().to_string();

    // Production runtime ONLY has intrface.crf - no fallback to folders
    let intrface_crf_path = data_root.join("res/intrface.crf");

    println!(
        "Using intrface.crf archive: {}",
        intrface_crf_path.display()
    );
    let asset_path = ZipAssetPath::new(intrface_crf_path.to_string_lossy().to_string());
    let mut asset_cache =
        engine::assets::asset_cache::AssetCache::new(data_path.clone(), asset_path);

    match dark::map::MapChunkData::load_from_mission(&mut asset_cache, mission) {
        Ok(map_data) => {
            println!("=== Map Chunk Data for {} ===", map_data.mission_name);
            println!("Found {} map chunks", map_data.chunk_count());
            println!();

            println!("Revealed Rectangles (P001RA.BIN):");
            for (i, rect) in map_data.revealed_rects.iter().enumerate() {
                println!(
                    "  Slot {}: ({}, {}) -> ({}, {}) [{}x{}]",
                    i,
                    rect.ul_x,
                    rect.ul_y,
                    rect.lr_x,
                    rect.lr_y,
                    rect.width(),
                    rect.height()
                );
            }

            println!();
            println!("Explored Rectangles (P001XA.BIN):");
            for (i, rect) in map_data.explored_rects.iter().enumerate() {
                println!(
                    "  Slot {}: ({}, {}) -> ({}, {}) [{}x{}]",
                    i,
                    rect.ul_x,
                    rect.ul_y,
                    rect.lr_x,
                    rect.lr_y,
                    rect.width(),
                    rect.height()
                );
            }

            // Show corresponding PCX files
            println!();
            println!("Corresponding PCX files:");
            for i in 0..map_data.chunk_count() {
                println!(
                    "  Slot {}: P001R{:03}.PCX (revealed), P001X{:03}.PCX (explored)",
                    i, i, i
                );
            }
        }
        Err(e) => {
            println!("Error loading map data for {}: {}", mission, e);
            println!("Expected asset paths:");
            println!("  {}/english/P001RA.BIN", mission.to_uppercase());
            println!("  {}/english/P001XA.BIN", mission.to_uppercase());
            println!(
                "Note: These should be accessible via asset cache from {}/res/intrface/",
                data_path
            );
        }
    }

    Ok(())
}

fn show_unparsed_data(entity_id: i32, entity_info: &dark::ss2_entity_info::SystemShock2EntityInfo) {
    println!("Unparsed Data:");

    // Collect unparsed properties for this entity
    let mut unparsed_props = Vec::new();
    for (prop_name, unparsed_list) in &entity_info.unparsed_properties {
        for unparsed_prop in unparsed_list {
            if unparsed_prop.entity_id == entity_id {
                unparsed_props.push((prop_name.clone(), unparsed_prop.byte_len));
            }
        }
    }

    // Collect unparsed links for this entity
    let mut unparsed_links = Vec::new();
    for (link_name, link_list) in &entity_info.unparsed_links {
        for link in link_list {
            if link.src == entity_id || link.dest == entity_id {
                unparsed_links.push(link_name.clone());
            }
        }
    }

    // Show unparsed properties
    println!("  Unparsed Properties:");
    if unparsed_props.is_empty() {
        println!("    (none)");
    } else {
        for (i, (prop_name, byte_len)) in unparsed_props.iter().enumerate() {
            println!("    {}. {} ({} bytes)", i + 1, prop_name, byte_len);
        }
    }

    // Show unparsed links
    println!("  Unparsed Links:");
    if unparsed_links.is_empty() {
        println!("    (none)");
    } else {
        // Deduplicate link names
        let mut unique_links: Vec<String> = unparsed_links.into_iter().collect();
        unique_links.sort();
        unique_links.dedup();

        for (i, link_name) in unique_links.iter().enumerate() {
            println!("    {}. {}", i + 1, link_name);
        }
    }
}
