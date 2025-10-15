use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, warn};

mod config;
mod git;
mod prompts;
mod claude_code;
mod github;
mod orchestrator;

use config::Config;
use orchestrator::Orchestrator;

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
    /// Monitor a specific PR until it's ready
    MonitorPr {
        /// PR number to monitor
        pr_number: u32,

        /// Maximum time to wait (e.g., "30m", "2h")
        #[arg(long, default_value = "30m")]
        timeout: String,
    },
    /// Check detailed status of a PR
    CheckPr {
        /// PR number to check
        pr_number: u32,

        /// Show detailed failure analysis
        #[arg(long)]
        analyze_failures: bool,
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
        Commands::ListPrompts => {
            info!("Listing available prompts");
            list_prompts(&config).await?;
        }
        Commands::MonitorPr { pr_number, timeout } => {
            info!("Monitoring PR #{} with timeout: {}", pr_number, timeout);
            monitor_pr(&config, pr_number, &timeout).await?;
        }
        Commands::CheckPr { pr_number, analyze_failures } => {
            info!("Checking status of PR #{}", pr_number);
            check_pr(&config, pr_number, analyze_failures).await?;
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
    info!("🎯 Executing single orchestration cycle");

    let mut orchestrator = Orchestrator::new(config.clone()).await
        .context("Failed to create orchestrator")?;

    let cycle = orchestrator.run_once().await
        .context("Orchestration cycle failed")?;

    info!("✅ Orchestration cycle completed: {}", cycle.id);
    info!("   Prompt: {}", cycle.selected_prompt);
    info!("   Phase: {:?}", cycle.phase);
    info!("   Duration: {:.2}s", cycle.start_time.elapsed().as_secs_f64());

    if let Some(pr_number) = cycle.created_pr_number {
        info!("   PR created: #{}", pr_number);
    }

    // Show execution log
    if !cycle.execution_log.is_empty() {
        info!("Execution log:");
        for log_entry in &cycle.execution_log {
            info!("  {}", log_entry);
        }
    }

    Ok(())
}

async fn run_loop(config: &Config, interval: &str) -> Result<()> {
    info!("🔄 Starting continuous orchestration loop with interval: {}", interval);

    // Override config interval if provided
    let mut config = config.clone();
    config.shodan.interval = interval.to_string();

    let mut orchestrator = Orchestrator::new(config).await
        .context("Failed to create orchestrator")?;

    orchestrator.start_orchestration().await
        .context("Orchestration loop failed")?;

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

    // Execute with Claude Code
    info!("🚀 Executing prompt with Claude Code...");
    match claude_code::execute_prompt(config, &prompt).await {
        Ok(output) => {
            info!("✅ Claude Code execution completed");
            info!("   Session ID: {}", output.session_id);
            info!("   Success: {}", output.success);
            info!("   Execution time: {:.2}s", output.execution_time_seconds);

            if !output.files_created.is_empty() {
                info!("   Files created: {}", output.files_created.len());
                for file in &output.files_created {
                    info!("     + {}", file.display());
                }
            }

            if !output.files_modified.is_empty() {
                info!("   Files modified: {}", output.files_modified.len());
                for file in &output.files_modified {
                    info!("     ~ {}", file.display());
                }
            }

            if let Some(git_changes) = &output.git_changes {
                if let Some(branch) = &git_changes.branch_created {
                    info!("   Branch created: {}", branch);
                }
                if !git_changes.commits.is_empty() {
                    info!("   Commits: {}", git_changes.commits.len());
                }
                if let Some(pr) = git_changes.pr_created {
                    info!("   PR created: #{}", pr);
                }
            }

            if let Some(error) = &output.error {
                warn!("   Error: {}", error);
            }

            if !output.output.is_empty() {
                info!("Claude Code output:");
                println!("---");
                println!("{}", output.output);
                println!("---");
            }
        }
        Err(e) => {
            warn!("❌ Claude Code execution failed: {}", e);
            return Err(e);
        }
    }

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

async fn monitor_pr(config: &Config, pr_number: u32, timeout_str: &str) -> Result<()> {
    info!("Starting PR monitoring for PR #{}", pr_number);

    // Parse timeout
    let timeout_seconds = config.parse_interval(timeout_str)
        .context("Failed to parse timeout duration")?;
    let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

    // Create PR monitor
    let mut monitor = github::PRMonitor::new(config.clone());

    // Start monitoring
    monitor.start_monitoring(pr_number).await?;
    info!("✅ Monitoring started for PR #{}", pr_number);

    // Wait for PR to become ready
    match monitor.wait_for_pr_ready(pr_number, timeout_duration).await {
        Ok(final_status) => {
            info!("🎉 PR #{} is ready for merge!", pr_number);
            info!("   Final status: ready={}", final_status.is_ready);
            info!("   Checks passed: {}/{}",
                  final_status.checks.iter().filter(|c| matches!(c.conclusion, Some(github::CheckConclusion::Success))).count(),
                  final_status.checks.len());

            if let Some(true) = final_status.merge_status.mergeable {
                info!("   ✅ PR is mergeable with no conflicts");
            }
        }
        Err(e) => {
            warn!("❌ PR monitoring failed or timed out: {}", e);

            // Get final status for reporting
            if let Ok(status) = monitor.check_pr_detailed_status(pr_number).await {
                warn!("Final status:");
                warn!("  Ready: {}", status.is_ready);
                warn!("  Blocking issues: {}", status.blocking_issues.len());
                for issue in &status.blocking_issues {
                    warn!("    - {}", issue);
                }
            }

            return Err(e);
        }
    }

    Ok(())
}

async fn check_pr(config: &Config, pr_number: u32, analyze_failures: bool) -> Result<()> {
    info!("Checking detailed status for PR #{}", pr_number);

    let monitor = github::PRMonitor::new(config.clone());
    let status = monitor.check_pr_detailed_status(pr_number).await?;

    // Display PR information
    info!("PR #{}: {}", status.pr.number, status.pr.title);
    info!("  Author: {}", status.pr.author);
    info!("  State: {}", status.pr.state);
    info!("  Branch: {} -> {}", status.pr.head_ref, status.pr.base_ref);
    info!("  URL: {}", status.pr.url);

    // Display readiness status
    if status.is_ready {
        info!("✅ PR is ready for merge");
    } else {
        warn!("❌ PR is NOT ready for merge");
    }

    // Display merge status
    info!("Merge Status:");
    info!("  Mergeable: {:?}", status.merge_status.mergeable);
    info!("  State: {}", status.merge_status.mergeable_state);
    info!("  Has conflicts: {}", status.merge_status.has_conflicts);
    info!("  Required checks passing: {}", status.merge_status.required_checks_passing);

    // Display CI checks
    info!("CI Checks ({}):", status.checks.len());
    for check in &status.checks {
        let status_icon = match (&check.status, &check.conclusion) {
            (github::CheckState::Completed, Some(github::CheckConclusion::Success)) => "✅",
            (github::CheckState::Completed, Some(github::CheckConclusion::Failure)) => "❌",
            (github::CheckState::Completed, Some(github::CheckConclusion::Cancelled)) => "🚫",
            (github::CheckState::InProgress, _) => "🔄",
            (github::CheckState::Queued | github::CheckState::Pending, _) => "⏳",
            _ => "❓",
        };

        info!("  {} {} ({:?})", status_icon, check.name, check.status);
        if let Some(conclusion) = &check.conclusion {
            info!("     Conclusion: {:?}", conclusion);
        }
    }

    // Display blocking issues
    if !status.blocking_issues.is_empty() {
        warn!("Blocking Issues:");
        for issue in &status.blocking_issues {
            warn!("  - {}", issue);
        }
    }

    // Perform failure analysis if requested
    if analyze_failures {
        info!("Performing failure analysis...");
        match monitor.analyze_pr_failures(pr_number).await {
            Ok(analysis) => {
                if !analysis.failed_checks.is_empty() {
                    warn!("Failed Checks Analysis:");
                    for check in &analysis.failed_checks {
                        warn!("  ❌ {}: {:?}", check.name, check.conclusion);
                    }
                }

                if !analysis.error_logs.is_empty() {
                    warn!("Error Logs:");
                    for (i, log) in analysis.error_logs.iter().enumerate().take(10) {
                        warn!("  {}: {}", i + 1, log);
                    }
                    if analysis.error_logs.len() > 10 {
                        warn!("  ... and {} more error lines", analysis.error_logs.len() - 10);
                    }
                }

                // Suggested fixes removed - actual error logs are provided instead

                if analysis.retry_recommended {
                    info!("🔄 Retry is recommended for this PR");
                }
            }
            Err(e) => {
                warn!("Failed to analyze failures: {}", e);
            }
        }
    }

    Ok(())
}