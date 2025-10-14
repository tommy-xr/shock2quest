use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, warn};

mod config;
mod git;
mod prompts;

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
    /// List available prompts and show statistics
    ListPrompts,
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
        Commands::ListPrompts => {
            info!("Listing available prompts");
            list_prompts(&config).await?;
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

    // Get complete repository state
    let repo_state = git::get_repository_state().await?;

    // Display Git status
    info!("Git Status:");
    info!("  Current branch: {}", repo_state.git_status.current_branch);
    info!("  Is clean: {}", repo_state.git_status.is_clean);
    info!("  Uncommitted changes: {}", repo_state.git_status.has_uncommitted_changes);
    info!("  Untracked files: {}", repo_state.git_status.has_untracked_files);
    info!("  Ahead of upstream: {}", repo_state.git_status.ahead_of_upstream);
    info!("  Behind upstream: {}", repo_state.git_status.behind_upstream);

    // Display open PRs
    info!("Open Pull Requests: {}", repo_state.open_prs.len());
    for pr in &repo_state.open_prs {
        info!("  PR #{}: {} ({}) - by {}", pr.number, pr.title, pr.state, pr.author);
        info!("    {} -> {}", pr.head_ref, pr.base_ref);
        info!("    URL: {}", pr.url);
    }

    // Display active Claude Code sessions
    if repo_state.active_claude_sessions.is_empty() {
        info!("Active Claude Code sessions: None");
    } else {
        info!("Active Claude Code sessions: {}", repo_state.active_claude_sessions.len());
        for session in &repo_state.active_claude_sessions {
            info!("  {}", session);
        }
    }

    // Check if ready for orchestration
    let ready_for_orchestration = repo_state.git_status.is_clean
        && repo_state.active_claude_sessions.is_empty()
        && repo_state.git_status.current_branch == config.shodan.main_branch;

    if ready_for_orchestration {
        info!("✅ Repository is ready for Shodan orchestration");
    } else {
        warn!("⚠️  Repository is NOT ready for Shodan orchestration");
        if !repo_state.git_status.is_clean {
            warn!("   - Repository has uncommitted changes or untracked files");
        }
        if !repo_state.active_claude_sessions.is_empty() {
            warn!("   - Active Claude Code sessions detected");
        }
        if repo_state.git_status.current_branch != config.shodan.main_branch {
            warn!("   - Not on main branch ({})", config.shodan.main_branch);
        }
    }

    Ok(())
}

async fn test_prompt(config: &Config, prompt_file: &PathBuf, dry_run: bool) -> Result<()> {
    info!("Testing prompt file: {}", prompt_file.display());

    // Load and validate the specific prompt
    let prompt = prompts::load_prompt(prompt_file, config).await?;
    info!("✅ Prompt loaded successfully: {}", prompt.name);
    info!("   Weight: {}", prompt.weight);
    info!("   Risk Level: {:?}", prompt.metadata.risk_level);

    if let Some(title) = &prompt.metadata.title {
        info!("   Title: {}", title);
    }

    if let Some(description) = &prompt.metadata.description {
        info!("   Description: {}", description);
    }

    if !prompt.metadata.tags.is_empty() {
        info!("   Tags: {}", prompt.metadata.tags.join(", "));
    }

    // Show formatted content
    info!("Formatted prompt content:");
    println!("---");
    println!("{}", prompts::format_prompt_for_execution(&prompt));
    println!("---");

    if dry_run {
        info!("✅ Dry run mode - prompt validation completed successfully");
        return Ok(());
    }

    // TODO: Execute with Claude Code
    warn!("Claude Code execution not yet implemented");
    Ok(())
}

async fn list_prompts(config: &Config) -> Result<()> {
    info!("Discovering available prompts");

    let prompts = prompts::discover_prompts(config).await?;
    let stats = prompts::get_prompt_stats(&prompts);

    // Display statistics
    info!("Prompt Statistics:");
    info!("  Total prompts: {}", stats.total_prompts);
    info!("  Total weight: {}", stats.total_weight);
    info!("  Average weight: {:.1}", stats.average_weight);

    // Display risk distribution
    if !stats.risk_distribution.is_empty() {
        info!("  Risk level distribution:");
        for (risk_level, count) in &stats.risk_distribution {
            info!("    {}: {}", risk_level, count);
        }
    }

    // Display tag distribution
    if !stats.tag_distribution.is_empty() {
        info!("  Tag distribution:");
        for (tag, count) in &stats.tag_distribution {
            info!("    {}: {}", tag, count);
        }
    }

    // List individual prompts
    info!("Available Prompts:");
    for prompt in &prompts {
        info!("  📄 {} (weight: {})", prompt.name, prompt.weight);
        if let Some(title) = &prompt.metadata.title {
            info!("     Title: {}", title);
        }
        if let Some(description) = &prompt.metadata.description {
            info!("     Description: {}", description);
        }
        info!("     Risk: {:?}", prompt.metadata.risk_level);
        if !prompt.metadata.tags.is_empty() {
            info!("     Tags: {}", prompt.metadata.tags.join(", "));
        }
        info!("     Path: {}", prompt.file_path.display());
        println!();
    }

    if !prompts.is_empty() {
        // Show a random selection example
        let selected = prompts::select_random_prompt(&prompts)?;
        info!("🎲 Random selection example: {} (weight: {})", selected.name, selected.weight);
    }

    Ok(())
}