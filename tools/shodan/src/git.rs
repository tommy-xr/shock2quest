use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::{retry_operation, RetryConfig, ShodanError, ShodanResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub state: String,
    pub url: String,
    pub head_ref: String,
    pub base_ref: String,
    pub author: String,
}

#[derive(Debug, Clone)]
pub struct RepositoryInfo {
    pub owner: String,
    pub name: String,
    pub full_name: String,
}

#[derive(Debug, Clone)]
pub struct GitStatus {
    pub current_branch: String,
    pub is_clean: bool,
    pub has_uncommitted_changes: bool,
    pub has_untracked_files: bool,
    pub ahead_of_upstream: u32,
    pub behind_upstream: u32,
}

#[derive(Debug, Clone)]
pub struct RepositoryState {
    pub git_status: GitStatus,
    pub open_prs: Vec<PullRequest>,
    pub active_claude_sessions: Vec<String>,
}

/// Helper function for robust git command execution
async fn execute_git_command(args: &[&str], operation: &str) -> ShodanResult<String> {
    debug!("Executing git command: {}", args.join(" "));

    let output = TokioCommand::new("git")
        .args(args)
        .output()
        .await
        .map_err(|e| ShodanError::from_io_error(&format!("git {}", args.join(" ")), e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ShodanError::from_git_failure(
            operation,
            output.status,
            &stderr,
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the current Git branch name with retry logic
pub async fn get_current_branch() -> Result<String> {
    debug!("Getting current Git branch");

    let retry_config = RetryConfig {
        max_attempts: 2,
        initial_delay: std::time::Duration::from_secs(1),
        ..Default::default()
    };

    let result = retry_operation("get_current_branch", &retry_config, || async {
        execute_git_command(&["rev-parse", "--abbrev-ref", "HEAD"], "get current branch").await
    })
    .await;

    match result {
        Ok(branch) => {
            debug!("Current branch: {}", branch);
            Ok(branch)
        }
        Err(e) => {
            warn!("Failed to get current branch: {}", e);
            Err(anyhow::anyhow!("Git operation failed: {}", e))
        }
    }
}

/// Checkout the main branch
pub async fn checkout_main(config: &Config) -> Result<()> {
    let main_branch = &config.shodan.main_branch;
    info!("Checking out main branch: {}", main_branch);

    let output = TokioCommand::new("git")
        .args(["checkout", main_branch])
        .output()
        .await
        .context("Failed to execute git checkout command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Git checkout failed: {}", stderr));
    }

    info!("Successfully checked out {}", main_branch);
    Ok(())
}

/// Run the sync command (typically "gt sync")
pub async fn run_gt_sync(config: &Config) -> Result<()> {
    let sync_command = &config.shodan.sync_command;
    info!("Running sync command: {}", sync_command);

    // Parse the command into parts
    let parts: Vec<&str> = sync_command.split_whitespace().collect();
    if parts.is_empty() {
        return Err(anyhow::anyhow!("Empty sync command"));
    }

    let (cmd, args) = parts.split_first().unwrap();

    let output = TokioCommand::new(cmd)
        .args(args)
        .output()
        .await
        .with_context(|| format!("Failed to execute sync command: {}", sync_command))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        warn!("Sync command output: {}", stdout);
        return Err(anyhow::anyhow!("Sync command failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    info!("Sync completed successfully: {}", stdout.trim());
    Ok(())
}

/// Get repository information using GitHub CLI
pub async fn get_repository_info() -> Result<RepositoryInfo> {
    debug!("Getting repository information via gh CLI");

    let output = TokioCommand::new("gh")
        .args(["repo", "view", "--json", "owner,name,nameWithOwner"])
        .output()
        .await
        .context("Failed to execute gh repo view command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("gh repo view failed: {}", stderr));
    }

    let stdout = String::from_utf8(output.stdout).context("Invalid UTF-8 in gh output")?;

    let repo_json: serde_json::Value =
        serde_json::from_str(&stdout).context("Failed to parse gh repo view JSON output")?;

    let repo_info = RepositoryInfo {
        owner: repo_json["owner"]["login"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        name: repo_json["name"].as_str().unwrap_or("").to_string(),
        full_name: repo_json["nameWithOwner"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    };

    debug!(
        "Repository: {} (owner: {})",
        repo_info.full_name, repo_info.owner
    );
    Ok(repo_info)
}

/// Get list of open pull requests using GitHub CLI, filtered by repository owner for security
pub async fn get_open_prs() -> Result<Vec<PullRequest>> {
    debug!("Getting open pull requests via gh CLI");

    // First get repository info to determine the owner
    let repo_info = get_repository_info().await?;

    let output = TokioCommand::new("gh")
        .args(["pr", "list", "--json", "number,title,state,url,headRefName,baseRefName,author"])
        .output()
        .await
        .context("Failed to execute gh pr list command. Make sure GitHub CLI (gh) is installed and authenticated.")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("gh pr list failed: {}", stderr));
    }

    let stdout = String::from_utf8(output.stdout).context("Invalid UTF-8 in gh output")?;

    // Parse JSON response
    let gh_prs: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).context("Failed to parse gh pr list JSON output")?;

    let mut prs = Vec::new();
    let mut filtered_count = 0;

    for pr_json in gh_prs {
        let author = pr_json["author"]["login"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Security filter: Only include PRs from repository owner
        if author != repo_info.owner {
            filtered_count += 1;
            debug!(
                "Filtering out PR #{} from non-owner author: {}",
                pr_json["number"].as_u64().unwrap_or(0),
                author
            );
            continue;
        }

        let pr = PullRequest {
            number: pr_json["number"].as_u64().unwrap_or(0) as u32,
            title: pr_json["title"].as_str().unwrap_or("").to_string(),
            state: pr_json["state"].as_str().unwrap_or("").to_string(),
            url: pr_json["url"].as_str().unwrap_or("").to_string(),
            head_ref: pr_json["headRefName"].as_str().unwrap_or("").to_string(),
            base_ref: pr_json["baseRefName"].as_str().unwrap_or("").to_string(),
            author,
        };
        prs.push(pr);
    }

    if filtered_count > 0 {
        info!(
            "Security: Filtered out {} PRs from non-owner authors",
            filtered_count
        );
    }
    debug!(
        "Found {} owner PRs (repo owner: {})",
        prs.len(),
        repo_info.owner
    );
    Ok(prs)
}

/// Check the status of a specific pull request with security validation
pub async fn check_pr_status(pr_number: u32) -> Result<PullRequest> {
    debug!("Checking status of PR #{}", pr_number);

    let output = TokioCommand::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "number,title,state,url,headRefName,baseRefName,author",
        ])
        .output()
        .await
        .context("Failed to execute gh pr view command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("gh pr view failed: {}", stderr));
    }

    let stdout = String::from_utf8(output.stdout).context("Invalid UTF-8 in gh output")?;

    let pr_json: serde_json::Value =
        serde_json::from_str(&stdout).context("Failed to parse gh pr view JSON output")?;

    let author = pr_json["author"]["login"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Security check: Verify this PR is from repository owner
    let repo_info = get_repository_info().await?;
    if author != repo_info.owner {
        return Err(anyhow::anyhow!(
            "Security: PR #{} is from non-owner author '{}' (repo owner: '{}')",
            pr_number,
            author,
            repo_info.owner
        ));
    }

    let pr = PullRequest {
        number: pr_json["number"].as_u64().unwrap_or(0) as u32,
        title: pr_json["title"].as_str().unwrap_or("").to_string(),
        state: pr_json["state"].as_str().unwrap_or("").to_string(),
        url: pr_json["url"].as_str().unwrap_or("").to_string(),
        head_ref: pr_json["headRefName"].as_str().unwrap_or("").to_string(),
        base_ref: pr_json["baseRefName"].as_str().unwrap_or("").to_string(),
        author,
    };

    debug!(
        "PR #{} status: {} (owner: {})",
        pr_number, pr.state, pr.author
    );
    Ok(pr)
}

/// Detect active Claude Code sessions by checking for running processes
pub async fn detect_active_claude_code_sessions() -> Result<Vec<String>> {
    debug!("Detecting active Claude Code sessions");

    // Look for claude processes
    let output = TokioCommand::new("pgrep")
        .args(["-f", "claude"])
        .output()
        .await;

    let sessions = match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| format!("PID: {}", line.trim()))
                .collect()
        }
        _ => {
            // pgrep failed or no processes found
            Vec::new()
        }
    };

    if sessions.is_empty() {
        debug!("No active Claude Code sessions detected");
    } else {
        debug!("Found {} active Claude Code sessions", sessions.len());
    }

    Ok(sessions)
}

/// Check for uncommitted changes in the repository
pub async fn check_uncommitted_changes() -> Result<GitStatus> {
    debug!("Checking Git repository status");

    // Get current branch
    let current_branch = get_current_branch().await?;

    // Get git status
    let status_output = TokioCommand::new("git")
        .args(["status", "--porcelain"])
        .output()
        .await
        .context("Failed to execute git status command")?;

    if !status_output.status.success() {
        let stderr = String::from_utf8_lossy(&status_output.stderr);
        return Err(anyhow::anyhow!("Git status failed: {}", stderr));
    }

    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    let has_changes = !status_stdout.trim().is_empty();

    // Count untracked files (lines starting with "??")
    let untracked_count = status_stdout
        .lines()
        .filter(|line| line.starts_with("??"))
        .count();
    let has_untracked_files = untracked_count > 0;

    // Count uncommitted changes (any other lines)
    let uncommitted_count = status_stdout
        .lines()
        .filter(|line| !line.starts_with("??") && !line.trim().is_empty())
        .count();
    let has_uncommitted_changes = uncommitted_count > 0;

    // Get upstream comparison (ahead/behind)
    let (ahead, behind) = get_upstream_comparison().await.unwrap_or((0, 0));

    let git_status = GitStatus {
        current_branch,
        is_clean: !has_changes,
        has_uncommitted_changes,
        has_untracked_files,
        ahead_of_upstream: ahead,
        behind_upstream: behind,
    };

    debug!("Git status: {:?}", git_status);
    Ok(git_status)
}

/// Get ahead/behind count compared to upstream
async fn get_upstream_comparison() -> Result<(u32, u32)> {
    let output = TokioCommand::new("git")
        .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
            if parts.len() == 2 {
                let ahead = parts[0].parse().unwrap_or(0);
                let behind = parts[1].parse().unwrap_or(0);
                Ok((ahead, behind))
            } else {
                Ok((0, 0))
            }
        }
        _ => Ok((0, 0)), // No upstream or other error
    }
}

/// Ensure working directory is clean and safe for automation
pub async fn ensure_clean_working_directory(config: &Config) -> Result<()> {
    info!("Ensuring clean working directory");

    let git_status = check_uncommitted_changes().await?;

    if git_status.has_uncommitted_changes {
        return Err(anyhow::anyhow!(
            "Repository has uncommitted changes. Please commit or stash them before running Shodan."
        ));
    }

    if git_status.has_untracked_files {
        warn!("Repository has untracked files, but continuing anyway");
    }

    // Check for active Claude Code sessions
    let active_sessions = detect_active_claude_code_sessions().await?;
    if !active_sessions.is_empty() {
        return Err(anyhow::anyhow!(
            "Active Claude Code sessions detected: {:?}. Please close them before running Shodan.",
            active_sessions
        ));
    }

    // Ensure we're on the correct branch
    if git_status.current_branch != config.shodan.main_branch {
        warn!(
            "Currently on branch '{}', switching to main branch '{}'",
            git_status.current_branch, config.shodan.main_branch
        );
        checkout_main(config).await?;
    }

    // Run sync to get latest state
    run_gt_sync(config).await?;

    info!("Working directory is clean and ready");
    Ok(())
}

/// Get complete repository state
pub async fn get_repository_state() -> Result<RepositoryState> {
    debug!("Getting complete repository state");

    let git_status = check_uncommitted_changes().await?;
    let open_prs = get_open_prs().await?;
    let active_claude_sessions = detect_active_claude_code_sessions().await?;

    let state = RepositoryState {
        git_status,
        open_prs,
        active_claude_sessions,
    };

    debug!("Repository state retrieved successfully");
    Ok(state)
}
