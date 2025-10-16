use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod database;
mod error;
mod formatters;
mod query;

use crate::commands::*;
use crate::error::EntityInspectorError;

#[derive(Parser)]
#[command(name = "entity_inspector")]
#[command(about = "A CLI tool for inspecting System Shock 2 entity data")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect specific entities
    Inspect {
        /// Path to gamesys or mission file
        #[arg(short, long)]
        file: PathBuf,

        /// Template ID to inspect
        #[arg(short, long, conflicts_with = "name")]
        template: Option<i32>,

        /// Entity name to inspect
        #[arg(short, long, conflicts_with = "template")]
        name: Option<String>,

        /// Show properties
        #[arg(long)]
        show_properties: bool,
    },

    /// Show inheritance hierarchy
    Hierarchy {
        /// Path to gamesys or mission file
        #[arg(short, long)]
        file: PathBuf,

        /// Template ID to show hierarchy for
        #[arg(short, long)]
        template: Option<i32>,

        /// Show all root templates
        #[arg(long)]
        roots: bool,

        /// Show properties in hierarchy
        #[arg(long)]
        show_properties: bool,
    },

    /// Query entities by criteria
    Query {
        /// Path to gamesys or mission file
        #[arg(short, long)]
        file: PathBuf,

        /// Property name to filter by
        #[arg(long)]
        property: Option<String>,

        /// Check if entity has specific property
        #[arg(long)]
        has_property: Option<String>,

        /// Template ID range (format: start..end)
        #[arg(long)]
        template_range: Option<String>,
    },

    /// Export entity data
    Export {
        /// Path to gamesys or mission file
        #[arg(short, long)]
        file: PathBuf,

        /// Output format
        #[arg(long, default_value = "json")]
        format: ExportFormat,

        /// Output file
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Include properties in export
        #[arg(long)]
        properties: bool,
    },

    /// Validate entity data
    Validate {
        /// Path to gamesys or mission file
        #[arg(short, long)]
        file: PathBuf,

        /// Check inheritance chains
        #[arg(long)]
        check_inheritance: bool,

        /// Check link integrity
        #[arg(long)]
        check_links: bool,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum ExportFormat {
    Json,
    Csv,
    Debug,
}

fn main() -> Result<(), EntityInspectorError> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("entity_inspector={}", log_level))
        .init();

    match cli.command {
        Commands::Inspect {
            file,
            template,
            name,
            show_properties,
        } => inspect_command(file, template, name, show_properties),

        Commands::Hierarchy {
            file,
            template,
            roots,
            show_properties,
        } => hierarchy_command(file, template, roots, show_properties),

        Commands::Query {
            file,
            property,
            has_property,
            template_range,
        } => query_command(file, property, has_property, template_range),

        Commands::Export {
            file,
            format,
            output,
            properties,
        } => export_command(file, format, output, properties),

        Commands::Validate {
            file,
            check_inheritance,
            check_links,
        } => validate_command(file, check_inheritance, check_links),
    }
}