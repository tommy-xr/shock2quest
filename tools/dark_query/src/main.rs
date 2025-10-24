use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

mod data_loader;
mod entity_analyzer;

use data_loader::load_entity_data;
use entity_analyzer::{analyze_entities, filter_entities, FilterCriteria, EntityType};

#[derive(Parser)]
#[command(name = "dark_query")]
#[command(about = "Query game data from System Shock 2 files")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Mission file to load (loads shock2.gam by default, or shock2.gam + mission if specified)
    #[arg(short, long)]
    mission: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// List all templates and entities
    Ls {
        /// Show only templates and entities with unparsed properties or links
        #[arg(long)]
        only_unparsed: bool,

        /// Filter by property or link name (supports wildcards)
        #[arg(long)]
        filter: Option<String>,
    },
    /// Show details for a particular entity or template
    Show {
        /// Entity ID or template ID to show details for
        entity_id: i32,

        /// Filter properties or links by name (supports wildcards)
        #[arg(long)]
        filter: Option<String>,
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

    if let Some(mission) = &cli.mission {
        info!("Mission specified: {}", mission);
    } else {
        info!("No mission specified, will use shock2.gam only");
    }

    match cli.command {
        Commands::Ls { only_unparsed, filter } => {
            handle_ls_command(cli.mission.as_deref(), only_unparsed, filter.as_deref())?;
        }
        Commands::Show { entity_id, filter } => {
            println!("show command - entity_id: {}, filter: {:?}", entity_id, filter);
            // TODO: Implement show command
        }
    }

    Ok(())
}

fn handle_ls_command(mission: Option<&str>, only_unparsed: bool, filter: Option<&str>) -> Result<()> {
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

    // Display results
    display_entity_list(&filtered_summaries, filter.is_some());

    Ok(())
}

fn display_entity_list(summaries: &[entity_analyzer::EntitySummary], show_filter_details: bool) {
    if summaries.is_empty() {
        println!("No entities found matching the criteria.");
        return;
    }

    // Print header
    if show_filter_details {
        println!("{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8} | Matched Properties",
                 "ID", "Type", "Names", "Template", "Props", "Links", "Unparsed");
        println!("{:-<8}-+-{:-<8}-+-{:-<40}-+-{:-<8}-+-{:-<5}-+-{:-<5}-+-{:-<8}-+{:-<20}",
                 "", "", "", "", "", "", "", "");
    } else {
        println!("{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8}",
                 "ID", "Type", "Names", "Template", "Props", "Links", "Unparsed");
        println!("{:-<8}-+-{:-<8}-+-{:-<40}-+-{:-<8}-+-{:-<5}-+-{:-<5}-+-{:-<8}",
                 "", "", "", "", "", "", "");
    }

    // Print entities
    for summary in summaries {
        let entity_type = match summary.entity_type {
            EntityType::Template => "Template",
            EntityType::Entity => "Entity",
        };

        let names_display = if summary.names.display_names().len() > 40 {
            format!("{}...", &summary.names.display_names()[..37])
        } else {
            summary.names.display_names()
        };

        let template_display = summary.template_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string());

        let unparsed_display = if summary.has_unparsed_data { "Yes" } else { "No" };

        if show_filter_details {
            let matched_props = summary.parsed_properties.join(", ");
            let matched_props_display = if matched_props.len() > 20 {
                format!("{}...", &matched_props[..17])
            } else {
                matched_props
            };

            println!("{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8} | {}",
                     summary.id, entity_type, names_display, template_display,
                     summary.property_count, summary.link_count, unparsed_display,
                     matched_props_display);
        } else {
            println!("{:<8} | {:<8} | {:<40} | {:<8} | {:<5} | {:<5} | {:<8}",
                     summary.id, entity_type, names_display, template_display,
                     summary.property_count, summary.link_count, unparsed_display);
        }
    }

    println!("\nTotal: {} entities", summaries.len());
}