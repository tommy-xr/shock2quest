use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

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
            println!("ls command - only_unparsed: {}, filter: {:?}", only_unparsed, filter);
            // TODO: Implement ls command
        }
        Commands::Show { entity_id, filter } => {
            println!("show command - entity_id: {}, filter: {:?}", entity_id, filter);
            // TODO: Implement show command
        }
    }

    Ok(())
}