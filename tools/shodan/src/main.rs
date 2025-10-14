use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, warn};

mod config;

use config::Config;

#[derive(Parser)]
#[command(name = "shodan")]
#[command(about = "Claude Code orchestrator for automated project development")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Configuration file path
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the orchestration loop
    Run {
        /// Override the default interval (e.g., "30m", "2h")
        #[arg(short, long)]
        interval: Option<String>,

        /// Run once and exit (don't loop)
        #[arg(long)]
        once: bool,
    },
    /// Check current repository state
    Check,
    /// Test a specific prompt
    TestPrompt {
        /// Path to the prompt file
        prompt_file: PathBuf,

        /// Don't actually run Claude Code, just validate
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Load configuration
    let config = Config::load(cli.config.as_deref()).await?;
    info!("Loaded configuration");

    match cli.command {
        Commands::Run { interval, once } => {
            info!("Starting Shodan orchestration loop");
            if once {
                info!("Running once and exiting");
                run_once(&config).await?;
            } else {
                let interval = interval.unwrap_or_else(|| config.shodan.interval.clone());
                info!("Running with interval: {}", interval);
                run_loop(&config, &interval).await?;
            }
        }
        Commands::Check => {
            info!("Checking repository state");
            check_state(&config).await?;
        }
        Commands::TestPrompt { prompt_file, dry_run } => {
            info!("Testing prompt: {}", prompt_file.display());
            test_prompt(&config, &prompt_file, dry_run).await?;
        }
    }

    Ok(())
}

fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level))
        )
        .init();

    Ok(())
}

async fn run_once(config: &Config) -> Result<()> {
    info!("Executing single orchestration cycle");

    // TODO: Implement single run cycle
    // 1. Check if Claude Code is active
    // 2. Ensure clean git state
    // 3. Select random prompt
    // 4. Execute Claude Code
    // 5. Monitor PR

    warn!("Single run cycle not yet implemented");
    Ok(())
}

async fn run_loop(config: &Config, interval: &str) -> Result<()> {
    info!("Starting continuous orchestration loop with interval: {}", interval);

    // TODO: Implement continuous loop
    // Parse interval and run orchestration cycles

    warn!("Continuous loop not yet implemented");
    Ok(())
}

async fn check_state(config: &Config) -> Result<()> {
    info!("Checking current repository and system state");

    // TODO: Implement state checking
    // 1. Git status
    // 2. Open PRs
    // 3. Claude Code sessions
    // 4. CI status

    warn!("State checking not yet implemented");
    Ok(())
}

async fn test_prompt(config: &Config, prompt_file: &PathBuf, dry_run: bool) -> Result<()> {
    info!("Testing prompt file: {}", prompt_file.display());

    if dry_run {
        info!("Dry run mode - validating prompt without execution");
    }

    // TODO: Implement prompt testing
    // 1. Validate prompt file exists and is readable
    // 2. Parse prompt content
    // 3. If not dry run, execute with Claude Code

    warn!("Prompt testing not yet implemented");
    Ok(())
}