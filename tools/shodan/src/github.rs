use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::git::PullRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestStatus {
    pub pr: PullRequest,
    pub checks: Vec<CheckStatus>,
    pub merge_status: MergeStatus,
    pub is_ready: bool,
    pub blocking_issues: Vec<String>,
    pub last_updated: std::time::SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckStatus {
    pub name: String,
    pub status: CheckState,
    pub conclusion: Option<CheckConclusion>,
    pub url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub details_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CheckState {
    Queued,
    InProgress,
    Completed,
    Waiting,
    Requested,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CheckConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
    Stale,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeStatus {
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub merge_state_status: String,
    pub has_conflicts: bool,
    pub required_checks_passing: bool,
}

#[derive(Debug, Clone)]
pub struct PRMonitor {
    config: Config,
    monitored_prs: HashMap<u32, PRMonitorState>,
}

#[derive(Debug, Clone)]
struct PRMonitorState {
    pr_number: u32,
    start_time: Instant,
    last_check: Instant,
    failure_count: u32,
    status_history: Vec<PullRequestStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAnalysis {
    pub pr_number: u32,
    pub failed_checks: Vec<CheckStatus>,
    pub error_logs: Vec<String>,
    pub retry_recommended: bool,
}

impl PRMonitor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            monitored_prs: HashMap::new(),
        }
    }

    /// Extract run ID from GitHub URL (e.g., https://github.com/owner/repo/actions/runs/18468560103/job/52616505217)
    fn extract_run_id_from_url(&self, url: &str) -> Option<String> {
        if let Some(runs_pos) = url.find("/actions/runs/") {
            let start = runs_pos + "/actions/runs/".len();
            if let Some(end) = url[start..].find('/') {
                return Some(url[start..start + end].to_string());
            }
        }
        None
    }

    /// Get repository owner and name from git remote configuration
    async fn get_repository_info(&self) -> Result<(String, String)> {
        // Use gh api to get current repository info
        let output = TokioCommand::new("gh")
            .args(["repo", "view", "--json", "owner,name"])
            .output()
            .await
            .context("Failed to execute gh repo view command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Failed to get repository info: {}", stderr));
        }

        let stdout = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in gh repo view output")?;

        let json: serde_json::Value = serde_json::from_str(&stdout)
            .context("Failed to parse repository info JSON")?;

        let owner = json["owner"]["login"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No owner found in repository info"))?
            .to_string();

        let name = json["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No name found in repository info"))?
            .to_string();

        debug!("Detected repository: {}/{}", owner, name);
        Ok((owner, name))
    }

    /// Start monitoring a specific PR
    pub async fn start_monitoring(&mut self, pr_number: u32) -> Result<()> {
        info!("Starting monitoring for PR #{}", pr_number);

        // Verify PR exists and is from repository owner
        let pr = crate::git::check_pr_status(pr_number).await?;

        let monitor_state = PRMonitorState {
            pr_number,
            start_time: Instant::now(),
            last_check: Instant::now(),
            failure_count: 0,
            status_history: Vec::new(),
        };

        self.monitored_prs.insert(pr_number, monitor_state);
        info!("Started monitoring PR #{}: {}", pr_number, pr.title);

        Ok(())
    }

    /// Check the status of a specific PR with detailed CI information
    pub async fn check_pr_detailed_status(&self, pr_number: u32) -> Result<PullRequestStatus> {
        debug!("Checking detailed status for PR #{}", pr_number);

        // Get basic PR information
        let pr = crate::git::check_pr_status(pr_number).await?;

        // Get CI/CD check status
        let checks = self.get_pr_checks(pr_number).await?;

        // Get merge status
        let merge_status = self.get_merge_status(pr_number).await?;

        // Determine if PR is ready
        let is_ready = self.assess_pr_readiness(&checks, &merge_status);

        // Identify blocking issues
        let blocking_issues = self.identify_blocking_issues(&checks, &merge_status);

        let status = PullRequestStatus {
            pr,
            checks,
            merge_status,
            is_ready,
            blocking_issues: blocking_issues.clone(),
            last_updated: std::time::SystemTime::now(),
        };

        debug!("PR #{} status: ready={}, blocking_issues={}",
               pr_number, is_ready, blocking_issues.len());

        Ok(status)
    }

    /// Get CI/CD check status for a PR
    async fn get_pr_checks(&self, pr_number: u32) -> Result<Vec<CheckStatus>> {
        debug!("Getting CI checks for PR #{}", pr_number);

        // Use gh pr checks without JSON - parse text output instead
        let output = TokioCommand::new("gh")
            .args(["pr", "checks", &pr_number.to_string()])
            .output()
            .await
            .context("Failed to execute gh pr checks command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("gh pr checks failed, trying alternative approach: {}", stderr);

            // Try to get status via GitHub API using gh api
            return self.get_pr_checks_via_api(pr_number).await;
        }

        let stdout = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in gh output")?;

        if stdout.trim().is_empty() {
            debug!("No checks output, trying API approach");
            return self.get_pr_checks_via_api(pr_number).await;
        }

        // Parse text output from gh pr checks
        let mut checks = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("Some checks") || line.starts_with("All checks") {
                continue;
            }

            // Parse lines like: "✓ Check Name"  or "✗ Check Name" or "- Check Name"
            let (status, conclusion, name) = if line.starts_with("✓") {
                (CheckState::Completed, Some(CheckConclusion::Success), line[2..].trim())
            } else if line.starts_with("✗") {
                (CheckState::Completed, Some(CheckConclusion::Failure), line[2..].trim())
            } else if line.starts_with("◯") {
                (CheckState::Pending, None, line[2..].trim())
            } else if line.starts_with("-") {
                (CheckState::InProgress, None, line[2..].trim())
            } else {
                continue;
            };

            let check = CheckStatus {
                name: name.to_string(),
                status,
                conclusion,
                url: None,
                started_at: None,
                completed_at: None,
                details_url: None,
            };
            checks.push(check);
        }

        debug!("Found {} CI checks for PR #{}", checks.len(), pr_number);
        Ok(checks)
    }

    /// Alternative method to get PR checks via GitHub API
    async fn get_pr_checks_via_api(&self, pr_number: u32) -> Result<Vec<CheckStatus>> {
        debug!("Getting PR checks via GitHub API for PR #{}", pr_number);

        let (owner, repo) = self.get_repository_info().await?;
        let output = TokioCommand::new("gh")
            .args(["api", &format!("repos/{}/{}/pulls/{}/commits", owner, repo, pr_number)])
            .output()
            .await
            .context("Failed to get PR commits via API")?;

        if !output.status.success() {
            debug!("GitHub API call failed, returning empty checks list");
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let commits: Vec<serde_json::Value> = serde_json::from_str(&stdout)
            .context("Failed to parse commits JSON")?;

        if commits.is_empty() {
            return Ok(Vec::new());
        }

        // Get the latest commit SHA
        let latest_commit = &commits[commits.len() - 1];
        let commit_sha = latest_commit["sha"].as_str()
            .ok_or_else(|| anyhow::anyhow!("No commit SHA found"))?;

        // Get check runs for the latest commit
        let output = TokioCommand::new("gh")
            .args(["api", &format!("repos/{}/{}/commits/{}/check-runs", owner, repo, commit_sha)])
            .output()
            .await
            .context("Failed to get check runs via API")?;

        if !output.status.success() {
            debug!("Failed to get check runs, returning empty list");
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let response: serde_json::Value = serde_json::from_str(&stdout)
            .context("Failed to parse check runs JSON")?;

        let mut checks = Vec::new();
        if let Some(check_runs) = response["check_runs"].as_array() {
            for run in check_runs {
                let name = run["name"].as_str().unwrap_or("Unknown").to_string();
                let status = parse_check_state(run["status"].as_str().unwrap_or("unknown"));
                let conclusion = run["conclusion"].as_str().map(parse_check_conclusion);

                let check = CheckStatus {
                    name,
                    status,
                    conclusion,
                    url: run["html_url"].as_str().map(|s| s.to_string()),
                    started_at: run["started_at"].as_str().map(|s| s.to_string()),
                    completed_at: run["completed_at"].as_str().map(|s| s.to_string()),
                    details_url: run["details_url"].as_str().map(|s| s.to_string()),
                };
                checks.push(check);
            }
        }

        debug!("Found {} check runs via API for PR #{}", checks.len(), pr_number);
        Ok(checks)
    }

    /// Get merge status for a PR
    async fn get_merge_status(&self, pr_number: u32) -> Result<MergeStatus> {
        debug!("Getting merge status for PR #{}", pr_number);

        let output = TokioCommand::new("gh")
            .args(["pr", "view", &pr_number.to_string(), "--json", "mergeable,mergeStateStatus"])
            .output()
            .await
            .context("Failed to execute gh pr view command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("gh pr view failed: {}", stderr));
        }

        let stdout = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in gh output")?;

        let merge_json: serde_json::Value = serde_json::from_str(&stdout)
            .context("Failed to parse gh pr view JSON output")?;

        let mergeable = merge_json["mergeable"].as_bool();
        let merge_state_status = merge_json["mergeStateStatus"].as_str().unwrap_or("unknown").to_string();
        let mergeable_state = merge_state_status.clone(); // Use mergeStateStatus for both

        let has_conflicts = merge_state_status == "dirty";
        let required_checks_passing = merge_state_status == "clean" || merge_state_status == "unstable";

        let merge_status = MergeStatus {
            mergeable,
            mergeable_state,
            merge_state_status,
            has_conflicts,
            required_checks_passing,
        };

        debug!("PR #{} merge status: mergeable={:?}, conflicts={}",
               pr_number, mergeable, has_conflicts);

        Ok(merge_status)
    }

    /// Assess if a PR is ready for merge
    fn assess_pr_readiness(&self, checks: &[CheckStatus], merge_status: &MergeStatus) -> bool {
        // Check if all required checks are passing
        let all_checks_passing = checks.iter().all(|check| {
            check.status == CheckState::Completed &&
            matches!(check.conclusion, Some(CheckConclusion::Success) | Some(CheckConclusion::Neutral) | Some(CheckConclusion::Skipped))
        });

        // Check merge status
        let merge_ready = merge_status.mergeable.unwrap_or(false)
            && !merge_status.has_conflicts
            && merge_status.required_checks_passing;

        all_checks_passing && merge_ready
    }

    /// Identify blocking issues preventing merge
    fn identify_blocking_issues(&self, checks: &[CheckStatus], merge_status: &MergeStatus) -> Vec<String> {
        let mut issues = Vec::new();

        // Check for failing CI checks
        for check in checks {
            if check.status == CheckState::Completed {
                if let Some(conclusion) = &check.conclusion {
                    match conclusion {
                        CheckConclusion::Failure => {
                            issues.push(format!("Check '{}' failed", check.name));
                        }
                        CheckConclusion::TimedOut => {
                            issues.push(format!("Check '{}' timed out", check.name));
                        }
                        CheckConclusion::Cancelled => {
                            issues.push(format!("Check '{}' was cancelled", check.name));
                        }
                        CheckConclusion::ActionRequired => {
                            issues.push(format!("Check '{}' requires action", check.name));
                        }
                        _ => {}
                    }
                }
            } else if matches!(check.status, CheckState::Waiting | CheckState::Pending | CheckState::Queued) {
                issues.push(format!("Check '{}' is still {:#?}", check.name, check.status));
            }
        }

        // Check merge conflicts
        if merge_status.has_conflicts {
            issues.push("PR has merge conflicts".to_string());
        }

        // Check mergeable status
        if let Some(false) = merge_status.mergeable {
            issues.push("PR is not mergeable".to_string());
        }

        // Check required status checks
        if !merge_status.required_checks_passing {
            issues.push("Required status checks not passing".to_string());
        }

        issues
    }

    /// Analyze failures and suggest fixes
    pub async fn analyze_pr_failures(&self, pr_number: u32) -> Result<FailureAnalysis> {
        info!("Analyzing failures for PR #{}", pr_number);

        let status = self.check_pr_detailed_status(pr_number).await?;

        let failed_checks: Vec<CheckStatus> = status.checks.iter()
            .filter(|check| {
                matches!(check.conclusion, Some(CheckConclusion::Failure) | Some(CheckConclusion::TimedOut))
            })
            .cloned()
            .collect();

        let mut error_logs = Vec::new();

        // Get detailed logs for failed checks
        for check in &failed_checks {
            if let Some(details_url) = &check.details_url {
                // Extract run ID from details URL (format: https://github.com/owner/repo/actions/runs/18468560103/job/52616505217)
                if let Some(run_id) = self.extract_run_id_from_url(details_url) {
                    debug!("Extracted run ID {} from check '{}'", run_id, check.name);

                    match self.get_logs_via_rest_api(&run_id).await {
                        Ok(logs) => {
                            if !logs.is_empty() {
                                // Save raw logs to a temporary file for LLM access
                                let temp_dir = std::env::temp_dir();
                                let log_file = temp_dir.join(format!("shodan_logs_pr{}_{}_run{}.txt", pr_number, check.name, run_id));

                                match std::fs::write(&log_file, logs.join("\n")) {
                                    Ok(_) => {
                                        error_logs.push(format!("✅ Retrieved detailed build logs for check '{}' (run ID: {})", check.name, run_id));
                                        error_logs.push(format!("📁 Raw logs saved to: {}", log_file.display()));
                                        error_logs.push(format!("📊 Log contains {} lines of build output including error details", logs.len()));
                                        error_logs.push("🔍 The raw logs contain the complete build failure information that Claude can analyze.".to_string());
                                        info!("Successfully saved {} log lines for check '{}' to {}", logs.len(), check.name, log_file.display());
                                    }
                                    Err(e) => {
                                        warn!("Failed to save logs to file: {}", e);
                                        // Fall back to including logs directly
                                        error_logs.push(format!("=== Logs for check: {} ===", check.name));
                                        error_logs.extend(logs);
                                        error_logs.push("=== End of logs ===".to_string());
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to get logs for check '{}': {}", check.name, e);
                            error_logs.push(format!("❌ Could not retrieve logs for check '{}': {}", check.name, e));
                        }
                    }
                } else {
                    debug!("Could not extract run ID from details URL: {}", details_url);
                }
            }

            // Skip generating generic suggested fixes since actual logs are available
        }

        // Determine if retry is recommended
        let retry_recommended = failed_checks.iter().any(|check| {
            matches!(check.conclusion, Some(CheckConclusion::TimedOut)) ||
            check.name.contains("flaky") ||
            check.name.contains("network")
        });

        let analysis = FailureAnalysis {
            pr_number,
            failed_checks,
            error_logs,
            retry_recommended,
        };

        info!("Failure analysis for PR #{}: {} failed checks",
              pr_number, analysis.failed_checks.len());

        Ok(analysis)
    }


    /// Get recent failed workflow runs matching the check name
    async fn get_recent_failed_runs(&self, check_name: &str) -> Result<Vec<String>> {
        debug!("Getting recent failed runs for check: {}", check_name);

        // First try with JSON format to get more details including workflow names
        let json_output = TokioCommand::new("gh")
            .args([
                "run", "list",
                "--json", "databaseId,name,workflowName,conclusion,headBranch",
                "--limit", "50"  // Get more runs since we'll filter for failures
            ])
            .output()
            .await;

        if let Ok(output) = json_output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Ok(runs) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                    for run in runs {
                        let run_name = run["name"].as_str().unwrap_or("");
                        let workflow_name = run["workflowName"].as_str().unwrap_or("");
                        let head_branch = run["headBranch"].as_str().unwrap_or("");
                        let conclusion = run["conclusion"].as_str().unwrap_or("");

                        // Only look at failed runs
                        if conclusion != "failure" {
                            continue;
                        }

                        // Check if this run matches our criteria
                        let matches = run_name.to_lowercase().contains(&check_name.to_lowercase()) ||
                                    workflow_name.to_lowercase().contains(&check_name.to_lowercase()) ||
                                    (check_name.to_lowercase().contains("build") &&
                                     (run_name.to_lowercase().contains("build") ||
                                      workflow_name.to_lowercase().contains("build")));

                        if matches {
                            if let Some(run_id) = run["databaseId"].as_u64() {
                                debug!("Found matching failed run: '{}' from workflow '{}' on branch '{}'",
                                       run_name, workflow_name, head_branch);

                                let mut logs = self.get_run_failure_logs(&run_id.to_string()).await?;
                                if !logs.is_empty() {
                                    // Prepend context about which workflow this is from
                                    logs.insert(0, format!("=== Failure from workflow: {} ===", workflow_name));
                                    logs.insert(1, format!("=== Job/Run name: {} ===", run_name));
                                    logs.insert(2, format!("=== Branch: {} ===", head_branch));
                                    logs.insert(3, "=== Error Details ===".to_string());
                                    return Ok(logs);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback to text-based approach
        let output = TokioCommand::new("gh")
            .args([
                "run", "list",
                "--status", "failure",
                "--limit", "10"
            ])
            .output()
            .await
            .context("Failed to get recent failed runs")?;

        if !output.status.success() {
            debug!("Failed to get recent runs: {}", String::from_utf8_lossy(&output.stderr));
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse text output to find matching runs
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("STATUS") {
                continue;
            }

            // Look for lines containing our check name
            if line.to_lowercase().contains(&check_name.to_lowercase()) ||
               check_name.to_lowercase().contains("build") && line.to_lowercase().contains("build") {

                // Extract run ID from the line (usually the last part)
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(run_id) = parts.last() {
                    if run_id.chars().all(|c| c.is_ascii_digit()) {
                        debug!("Found matching failed run ID: {}", run_id);
                        let mut logs = self.get_run_failure_logs(run_id).await?;
                        if !logs.is_empty() {
                            logs.insert(0, format!("=== Failure from run ID: {} ===", run_id));
                            return Ok(logs);
                        }
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get workflow runs for a specific commit
    async fn get_runs_for_commit(&self, commit_sha: &str, check_name: &str) -> Result<Vec<String>> {
        debug!("Getting runs for commit {} and check {}", commit_sha, check_name);

        let output = TokioCommand::new("gh")
            .args([
                "run", "list",
                "--commit", commit_sha,
                "--limit", "20"
            ])
            .output()
            .await
            .context("Failed to get runs for commit")?;

        if !output.status.success() {
            debug!("Failed to get runs for commit: {}", String::from_utf8_lossy(&output.stderr));
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse text output to find matching failed runs
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("STATUS") {
                continue;
            }

            // Look for failed runs that match our check
            if line.contains("failure") &&
               (line.to_lowercase().contains(&check_name.to_lowercase()) ||
                check_name.to_lowercase().contains("build") && line.to_lowercase().contains("build")) {

                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(run_id) = parts.last() {
                    if run_id.chars().all(|c| c.is_ascii_digit()) {
                        debug!("Found matching failed run ID for commit: {}", run_id);
                        return self.get_run_failure_logs(run_id).await;
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get PR status check details as fallback
    async fn get_pr_status_details(&self, pr_number: u32, check_name: &str) -> Result<Vec<String>> {
        debug!("Getting PR status details for check: {}", check_name);

        // Try to get more detailed status information
        let output = TokioCommand::new("gh")
            .args(["pr", "view", &pr_number.to_string()])
            .output()
            .await
            .context("Failed to get PR details")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut relevant_lines = Vec::new();
        let mut in_checks_section = false;

        for line in stdout.lines() {
            let line = line.trim();

            if line.to_lowercase().contains("check") || line.to_lowercase().contains("status") {
                in_checks_section = true;
            }

            if in_checks_section && (line.to_lowercase().contains(&check_name.to_lowercase()) ||
                                   line.contains("❌") || line.contains("✗") || line.contains("FAILED")) {
                relevant_lines.push(format!("PR Status: {}", line));
            }

            // Stop after checks section
            if in_checks_section && line.is_empty() {
                break;
            }
        }

        if relevant_lines.is_empty() {
            relevant_lines.push(format!("Check '{}' failed but detailed logs are not available through GitHub CLI", check_name));
            relevant_lines.push("This could be due to:".to_string());
            relevant_lines.push("- Build script failures (check build.rs files)".to_string());
            relevant_lines.push("- Missing system dependencies".to_string());
            relevant_lines.push("- Cross-compilation configuration issues".to_string());
            relevant_lines.push("- Environment variable configuration".to_string());
        }

        Ok(relevant_lines)
    }

    /// Get failure logs from a specific workflow run
    async fn get_run_failure_logs(&self, run_id: &str) -> Result<Vec<String>> {
        debug!("Getting failure logs for run ID: {}", run_id);

        // Try multiple approaches to get the logs

        // Approach 1: Use GitHub REST API to get logs (most reliable)
        if let Ok(api_logs) = self.get_logs_via_rest_api(run_id).await {
            if !api_logs.is_empty() {
                return Ok(api_logs);
            }
        }

        // Approach 2: Get failed logs specifically
        let failed_output = TokioCommand::new("gh")
            .args(["run", "view", run_id, "--log-failed"])
            .output()
            .await;

        if let Ok(output) = failed_output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    let parsed_logs = self.parse_failure_logs(&stdout);
                    if !parsed_logs.is_empty() {
                        return Ok(parsed_logs);
                    }
                }
            }
        }

        // Approach 3: Get all logs and filter for errors
        let all_output = TokioCommand::new("gh")
            .args(["run", "view", run_id, "--log"])
            .output()
            .await;

        if let Ok(output) = all_output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    let parsed_logs = self.parse_failure_logs(&stdout);
                    if !parsed_logs.is_empty() {
                        return Ok(parsed_logs);
                    }
                }
            }
        }

        // Approach 4: Try to download logs directly
        let download_output = TokioCommand::new("gh")
            .args(["run", "download", run_id, "--dir", "/tmp/shodan-logs"])
            .output()
            .await;

        if let Ok(output) = download_output {
            if output.status.success() {
                // Try to read downloaded log files
                if let Ok(downloaded_logs) = self.read_downloaded_logs("/tmp/shodan-logs").await {
                    if !downloaded_logs.is_empty() {
                        return Ok(downloaded_logs);
                    }
                }
            }
        }

        // Approach 5: Get basic run information as fallback
        let view_output = TokioCommand::new("gh")
            .args(["run", "view", run_id])
            .output()
            .await
            .context("Failed to get run view")?;

        if !view_output.status.success() {
            let stderr = String::from_utf8_lossy(&view_output.stderr);
            return Ok(vec![format!("Failed to get any logs for run {}: {}", run_id, stderr)]);
        }

        let stdout = String::from_utf8_lossy(&view_output.stdout);
        let basic_info = self.parse_basic_run_info(&stdout);

        Ok(basic_info)
    }

    /// Get logs via GitHub REST API using gh api command
    async fn get_logs_via_rest_api(&self, run_id: &str) -> Result<Vec<String>> {
        debug!("Getting logs via REST API for run ID: {}", run_id);

        let (owner, repo) = self.get_repository_info().await?;

        // Step 1: Get the workflow run details to find jobs
        let run_info_output = TokioCommand::new("gh")
            .args(["api", &format!("repos/{}/{}/actions/runs/{}", owner, repo, run_id)])
            .output()
            .await
            .context("Failed to get run info via API")?;

        if !run_info_output.status.success() {
            debug!("Failed to get run info: {}", String::from_utf8_lossy(&run_info_output.stderr));
            return Err(anyhow::anyhow!("Failed to get run info via API"));
        }

        let run_info_json: serde_json::Value = serde_json::from_slice(&run_info_output.stdout)
            .context("Failed to parse run info JSON")?;

        let workflow_name = run_info_json["name"].as_str().unwrap_or("Unknown");
        let conclusion = run_info_json["conclusion"].as_str().unwrap_or("unknown");

        // Step 2: Get jobs for this run
        let jobs_output = TokioCommand::new("gh")
            .args(["api", &format!("repos/{}/{}/actions/runs/{}/jobs", owner, repo, run_id)])
            .output()
            .await
            .context("Failed to get jobs via API")?;

        if !jobs_output.status.success() {
            debug!("Failed to get jobs: {}", String::from_utf8_lossy(&jobs_output.stderr));
            return Err(anyhow::anyhow!("Failed to get jobs via API"));
        }

        let jobs_json: serde_json::Value = serde_json::from_slice(&jobs_output.stdout)
            .context("Failed to parse jobs JSON")?;

        let mut all_logs = Vec::new();
        all_logs.push(format!("=== Workflow: {} (Conclusion: {}) ===", workflow_name, conclusion));

        // Step 3: Get logs for each failed job
        if let Some(jobs) = jobs_json["jobs"].as_array() {
            for job in jobs {
                let job_name = job["name"].as_str().unwrap_or("Unknown Job");
                let job_conclusion = job["conclusion"].as_str().unwrap_or("unknown");
                let job_id = job["id"].as_u64().unwrap_or(0);

                if job_conclusion == "failure" || job_conclusion == "cancelled" {
                    all_logs.push(format!("=== Job: {} (ID: {}, Conclusion: {}) ===", job_name, job_id, job_conclusion));

                    // Get logs for this specific job
                    let job_logs = self.get_job_logs_via_api(job_id).await?;
                    all_logs.extend(job_logs);
                    all_logs.push("".to_string()); // Add separator
                }
            }
        }

        // If we couldn't get logs via individual job API, try downloading the full ZIP
        if all_logs.len() <= 1 { // Only header, no actual logs
            debug!("No logs retrieved via job API, trying ZIP download for run {}", run_id);
            match self.download_and_extract_logs(run_id).await {
                Ok(zip_logs) => {
                    info!("Successfully retrieved {} log lines from ZIP download", zip_logs.len());
                    all_logs.push("=== Downloaded Raw Logs ===".to_string());
                    all_logs.extend(zip_logs);
                }
                Err(e) => {
                    warn!("Failed to download logs via ZIP: {}", e);
                    return Err(anyhow::anyhow!("No logs retrieved via job API or ZIP download"));
                }
            }
        }

        if all_logs.len() > 1 { // More than just the header
            Ok(all_logs)
        } else {
            Err(anyhow::anyhow!("No failed jobs found"))
        }
    }

    /// Get logs for a specific job via REST API
    async fn get_job_logs_via_api(&self, job_id: u64) -> Result<Vec<String>> {
        debug!("Getting job logs via REST API for job ID: {}", job_id);

        let (owner, repo) = self.get_repository_info().await?;

        // Use gh api to get logs - this will return the raw log content
        let logs_output = TokioCommand::new("gh")
            .args(["api", &format!("repos/{}/{}/actions/jobs/{}/logs", owner, repo, job_id)])
            .output()
            .await
            .context("Failed to get job logs via API")?;

        if !logs_output.status.success() {
            let stderr = String::from_utf8_lossy(&logs_output.stderr);
            debug!("Failed to get job logs: {}", stderr);
            return Ok(vec![format!("Failed to get logs for job {}: {}", job_id, stderr)]);
        }

        let log_content = String::from_utf8_lossy(&logs_output.stdout);

        // Parse the raw logs and extract error information
        let parsed_logs = self.parse_failure_logs(&log_content);

        if parsed_logs.is_empty() {
            // If no specific errors found, return a sample of the logs
            let lines: Vec<&str> = log_content.lines().collect();
            let total_lines = lines.len();

            if total_lines > 50 {
                let mut result = Vec::new();
                result.push(format!("Full log has {} lines. Showing last 50 lines:", total_lines));
                result.extend(lines.iter().skip(total_lines - 50).map(|s| s.to_string()));
                Ok(result)
            } else {
                Ok(lines.iter().map(|s| s.to_string()).collect())
            }
        } else {
            Ok(parsed_logs)
        }
    }

    /// Download raw logs as ZIP file and extract them
    async fn download_and_extract_logs(&self, run_id: &str) -> Result<Vec<String>> {
        debug!("Downloading and extracting raw logs for run ID: {}", run_id);

        let (owner, repo) = self.get_repository_info().await?;

        // Use gh api to download the logs zip file
        let logs_output = TokioCommand::new("gh")
            .args(["api", &format!("repos/{}/{}/actions/runs/{}/logs", owner, repo, run_id), "--paginate"])
            .output()
            .await
            .context("Failed to download logs via API")?;

        if !logs_output.status.success() {
            let stderr = String::from_utf8_lossy(&logs_output.stderr);
            debug!("Failed to download logs: {}", stderr);
            return Err(anyhow::anyhow!("Failed to download logs: {}", stderr));
        }

        // The logs endpoint returns a ZIP file as binary data
        let zip_data = logs_output.stdout;

        // Create a temporary directory for extracting logs
        let temp_dir = std::env::temp_dir().join(format!("shodan_logs_{}", run_id));
        std::fs::create_dir_all(&temp_dir)
            .context("Failed to create temporary directory for logs")?;

        // Write ZIP data to a temporary file
        let zip_path = temp_dir.join("logs.zip");
        std::fs::write(&zip_path, &zip_data)
            .context("Failed to write ZIP file")?;

        // Extract the ZIP file
        let extract_output = TokioCommand::new("unzip")
            .args(["-o", zip_path.to_str().unwrap(), "-d", temp_dir.to_str().unwrap()])
            .output()
            .await;

        match extract_output {
            Ok(output) if output.status.success() => {
                debug!("Successfully extracted logs to {:?}", temp_dir);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("unzip command failed: {}", stderr);
                // Try to fall back to reading the logs directly from the API response
                return self.parse_raw_logs_from_zip_data(&zip_data);
            }
            Err(e) => {
                warn!("unzip command not available: {}", e);
                // Try to fall back to reading the logs directly from the API response
                return self.parse_raw_logs_from_zip_data(&zip_data);
            }
        }

        // Read all .txt files in the extracted directory
        let mut all_logs = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "txt") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        all_logs.push(format!("=== {} ===", path.file_name().unwrap().to_string_lossy()));
                        all_logs.extend(content.lines().map(String::from));
                        all_logs.push(String::new()); // Add separator
                    }
                }
            }
        }

        // Clean up temporary directory
        std::fs::remove_dir_all(&temp_dir).ok();

        if all_logs.is_empty() {
            return Err(anyhow::anyhow!("No log files found in extracted ZIP"));
        }

        info!("Successfully extracted and parsed {} log lines from ZIP", all_logs.len());
        Ok(all_logs)
    }

    /// Parse raw logs directly from ZIP data (fallback when unzip is not available)
    fn parse_raw_logs_from_zip_data(&self, zip_data: &[u8]) -> Result<Vec<String>> {
        // This is a simple fallback - in a real implementation, you might want to use
        // a ZIP library like 'zip' crate to properly parse the ZIP file
        warn!("Falling back to basic ZIP data parsing");

        // Convert ZIP data to string and look for text content
        if let Ok(text_data) = String::from_utf8(zip_data.to_vec()) {
            let lines: Vec<String> = text_data.lines()
                .filter(|line| !line.is_empty() && line.len() > 10) // Filter out binary data
                .map(String::from)
                .collect();

            if !lines.is_empty() {
                return Ok(lines);
            }
        }

        Err(anyhow::anyhow!("Could not parse ZIP data as text"))
    }

    /// Parse failure logs from raw log output
    fn parse_failure_logs(&self, log_content: &str) -> Vec<String> {
        let mut error_lines = Vec::new();
        let mut in_error_section = false;
        let mut collecting_stacktrace = false;
        let mut collecting_cargo_output = false;

        for line in log_content.lines() {
            let line = line.trim();

            // Skip empty lines unless we're in an error section
            if line.is_empty() && !in_error_section {
                continue;
            }

            // Look for build system failures
            if line.contains("failed to run custom build command") {
                error_lines.push(line.to_string());
                in_error_section = true;
                collecting_cargo_output = true;
            }
            // Look for cargo compilation errors
            else if line.contains("Caused by:") {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for process failures
            else if line.contains("process didn't exit successfully:") {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for panic messages
            else if line.contains("thread '") && line.contains("panicked at") {
                error_lines.push(line.to_string());
                in_error_section = true;
                collecting_stacktrace = true;
            }
            // Look for unwrap/expect failures
            else if line.contains("called `Result::unwrap()` on an `Err` value:") {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for pkg-config errors
            else if line.contains("pkg-config") && (line.contains("error") || line.contains("not been configured")) {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for cross-compilation errors
            else if line.contains("cross-compilation") || (line.contains("TARGET_") && line.contains("not found")) {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for dependency resolution errors
            else if line.contains("couldn't resolve") || (line.contains("dependency") && line.contains("failed")) {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for standard error patterns
            else if line.contains("ERROR") || line.contains("FAILED") || line.contains("error:") {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for test failures
            else if line.contains("FAIL:") || line.contains("assertion failed") || line.contains("test result: FAILED") {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Look for build failures
            else if line.contains("Build failed") || line.contains("compilation failed") {
                error_lines.push(line.to_string());
                in_error_section = true;
            }
            // Collect stdout/stderr sections
            else if line.contains("--- stdout") || line.contains("--- stderr") {
                error_lines.push(line.to_string());
                in_error_section = true;
                collecting_cargo_output = true;
            }
            // Continue collecting if we're in an error section
            else if in_error_section {
                // Stop collecting at certain boundaries
                if line.starts_with("##") && !collecting_cargo_output {
                    in_error_section = false;
                    collecting_stacktrace = false;
                } else if !line.is_empty() {
                    // Include the line if it looks relevant
                    if collecting_stacktrace || collecting_cargo_output ||
                       line.contains("at ") ||  // stack trace lines
                       line.contains("cargo:") ||  // cargo build script output
                       line.contains("PKG_CONFIG") ||  // environment variable issues
                       line.contains("LIBAV") ||  // ffmpeg specific
                       line.contains("note:") ||  // compiler notes
                       line.contains("help:") ||  // compiler help
                       line.contains("-->") ||  // code location indicators
                       line.contains("exit status:") ||  // exit codes
                       line.starts_with("  ") {  // indented context lines
                        error_lines.push(line.to_string());
                    }
                }

                // Stop collecting cargo output after certain markers
                if collecting_cargo_output && (line.starts_with("##") || line.contains("=== End")) {
                    collecting_cargo_output = false;
                }

                // Limit output size but be more generous for build errors
                if error_lines.len() > 100 {
                    error_lines.push("... (truncated for brevity, see full logs for complete output)".to_string());
                    break;
                }
            }
        }

        error_lines
    }

    /// Read downloaded log files from a directory
    async fn read_downloaded_logs(&self, log_dir: &str) -> Result<Vec<String>> {
        use tokio::fs;

        let mut all_logs = Vec::new();

        // Try to read the log directory
        let mut entries = fs::read_dir(log_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name() {
                    if let Some(filename_str) = filename.to_str() {
                        if filename_str.ends_with(".txt") || filename_str.contains("log") {
                            match fs::read_to_string(&path).await {
                                Ok(content) => {
                                    all_logs.push(format!("=== {} ===", filename_str));
                                    all_logs.extend(self.parse_failure_logs(&content));
                                }
                                Err(e) => {
                                    debug!("Failed to read log file {}: {}", path.display(), e);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(all_logs)
    }

    /// Parse basic run information as fallback
    fn parse_basic_run_info(&self, run_view: &str) -> Vec<String> {
        let mut info = Vec::new();
        let mut in_jobs_section = false;

        for line in run_view.lines() {
            let line = line.trim();

            if line.contains("JOBS") || line.contains("Jobs") {
                in_jobs_section = true;
            }

            if in_jobs_section {
                if line.contains("✗") || line.contains("❌") || line.contains("FAILED") {
                    info.push(format!("Failed job: {}", line));
                }
                if line.contains("Conclusion:") {
                    info.push(line.to_string());
                }
            }

            // Look for error indicators
            if line.contains("error") || line.contains("failed") || line.contains("Error") {
                info.push(line.to_string());
            }
        }

        if info.is_empty() {
            info.push("Run failed but no detailed error information available".to_string());
            info.push("Try checking the GitHub Actions web interface for complete logs".to_string());
        }

        info
    }

    /// Generate fix suggestions based on check failures
    fn generate_fix_suggestions(&self, check_name: &str, check: &CheckStatus) -> Vec<String> {
        let mut suggestions = Vec::new();

        match check.conclusion {
            Some(CheckConclusion::Failure) => {
                if check_name.contains("test") {
                    suggestions.push("Review test failures and fix failing tests".to_string());
                    suggestions.push("Check for recent changes that might have broken tests".to_string());
                    suggestions.push("Run `cargo test` locally to reproduce the issue".to_string());
                } else if check_name.contains("build") || check_name.contains("compile") {
                    suggestions.push("Fix compilation errors - check the build logs for specific error messages".to_string());
                    suggestions.push("Common build issues to check:".to_string());
                    suggestions.push("  • Missing system dependencies (check build.rs files)".to_string());
                    suggestions.push("  • Cross-compilation configuration (Android NDK, pkg-config)".to_string());
                    suggestions.push("  • Environment variables (PKG_CONFIG_*, TARGET_*, LIBAV*, etc.)".to_string());
                    suggestions.push("  • Native library dependencies (ffmpeg, openssl, etc.)".to_string());

                    // Add specific suggestions based on common patterns
                    if check_name.to_lowercase().contains("android") {
                        suggestions.push("Android-specific build issues:".to_string());
                        suggestions.push("  • Check Android NDK setup and environment variables".to_string());
                        suggestions.push("  • Verify cross-compilation toolchain configuration".to_string());
                        suggestions.push("  • Check for missing aarch64-linux-android target".to_string());
                        suggestions.push("  • Review pkg-config cross-compilation settings".to_string());
                    }

                    suggestions.push("Try local reproduction:".to_string());
                    suggestions.push("  • Run `cargo check` to identify compilation issues".to_string());
                    suggestions.push("  • Run `cargo build` to test the build process".to_string());
                    if check_name.to_lowercase().contains("android") {
                        suggestions.push("  • Set up Android development environment locally".to_string());
                        suggestions.push("  • Test with `cargo apk build` if using Android target".to_string());
                    }
                } else if check_name.contains("lint") || check_name.contains("format") {
                    suggestions.push("Run cargo fmt to fix formatting issues".to_string());
                    suggestions.push("Run cargo clippy and fix linting warnings".to_string());
                    suggestions.push("Check for code style violations in the diff".to_string());
                } else if check_name.contains("security") {
                    suggestions.push("Review security scan results and address vulnerabilities".to_string());
                    suggestions.push("Check for unsafe code patterns or dependency vulnerabilities".to_string());
                } else {
                    suggestions.push(format!("Investigate and fix issues in '{}'", check_name));
                    suggestions.push("Check the workflow logs for specific error messages".to_string());
                    suggestions.push("Look for patterns like 'error:', 'failed:', 'panic:', or 'unwrap()'".to_string());
                }
            }
            Some(CheckConclusion::TimedOut) => {
                suggestions.push("Check for performance issues or infinite loops".to_string());
                suggestions.push("Consider splitting large tests into smaller chunks".to_string());
                suggestions.push("Retry the check as it may have been a temporary issue".to_string());
                if check_name.contains("build") {
                    suggestions.push("Build timeouts can be caused by:".to_string());
                    suggestions.push("  • Large dependency downloads".to_string());
                    suggestions.push("  • Slow native library compilation".to_string());
                    suggestions.push("  • Insufficient build resources".to_string());
                }
            }
            Some(CheckConclusion::Cancelled) => {
                suggestions.push("Check why the workflow was cancelled".to_string());
                suggestions.push("Retry the workflow if it was cancelled due to resource constraints".to_string());
                suggestions.push("Look for workflow configuration issues or dependency conflicts".to_string());
            }
            _ => {}
        }

        suggestions
    }

    /// Wait for PR to become ready with timeout and periodic checks
    pub async fn wait_for_pr_ready(&mut self, pr_number: u32, timeout: Duration) -> Result<PullRequestStatus> {
        info!("Waiting for PR #{} to become ready (timeout: {:?})", pr_number, timeout);

        let start_time = Instant::now();
        let check_interval = Duration::from_secs(
            self.config.parse_check_interval()
                .unwrap_or(300) // Default to 5 minutes
        );

        while start_time.elapsed() < timeout {
            let status = self.check_pr_detailed_status(pr_number).await?;

            // Update monitoring state
            if let Some(monitor_state) = self.monitored_prs.get_mut(&pr_number) {
                monitor_state.last_check = Instant::now();
                monitor_state.status_history.push(status.clone());

                // Keep only last 10 status updates
                if monitor_state.status_history.len() > 10 {
                    monitor_state.status_history.remove(0);
                }
            }

            if status.is_ready {
                info!("✅ PR #{} is ready for merge!", pr_number);
                return Ok(status);
            }

            if !status.blocking_issues.is_empty() {
                info!("PR #{} not ready. Blocking issues:", pr_number);
                for issue in &status.blocking_issues {
                    info!("  - {}", issue);
                }
            }

            // Check if we should analyze failures
            let has_failures = status.checks.iter().any(|check| {
                matches!(check.conclusion, Some(CheckConclusion::Failure))
            });

            if has_failures {
                warn!("PR #{} has failing checks, analyzing...", pr_number);
                if let Ok(analysis) = self.analyze_pr_failures(pr_number).await {
                    if !analysis.error_logs.is_empty() {
                        info!("Error logs available for analysis of PR #{}", pr_number);
                    }
                }
            }

            info!("Waiting {} seconds before next check...", check_interval.as_secs());
            sleep(check_interval).await;
        }

        warn!("Timeout waiting for PR #{} to become ready", pr_number);
        Err(anyhow::anyhow!("Timeout waiting for PR to become ready"))
    }
}

/// Parse GitHub check state string to enum
fn parse_check_state(state_str: &str) -> CheckState {
    match state_str.to_lowercase().as_str() {
        "queued" => CheckState::Queued,
        "in_progress" => CheckState::InProgress,
        "completed" => CheckState::Completed,
        "waiting" => CheckState::Waiting,
        "requested" => CheckState::Requested,
        "pending" => CheckState::Pending,
        _ => CheckState::Pending,
    }
}

/// Parse GitHub check conclusion string to enum
fn parse_check_conclusion(conclusion_str: &str) -> CheckConclusion {
    match conclusion_str.to_lowercase().as_str() {
        "success" => CheckConclusion::Success,
        "failure" => CheckConclusion::Failure,
        "neutral" => CheckConclusion::Neutral,
        "cancelled" => CheckConclusion::Cancelled,
        "timed_out" => CheckConclusion::TimedOut,
        "action_required" => CheckConclusion::ActionRequired,
        "stale" => CheckConclusion::Stale,
        "skipped" => CheckConclusion::Skipped,
        _ => CheckConclusion::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_check_state() {
        assert_eq!(parse_check_state("completed"), CheckState::Completed);
        assert_eq!(parse_check_state("in_progress"), CheckState::InProgress);
        assert_eq!(parse_check_state("queued"), CheckState::Queued);
        assert_eq!(parse_check_state("unknown"), CheckState::Pending);
    }

    #[test]
    fn test_parse_check_conclusion() {
        assert_eq!(parse_check_conclusion("success"), CheckConclusion::Success);
        assert_eq!(parse_check_conclusion("failure"), CheckConclusion::Failure);
        assert_eq!(parse_check_conclusion("timed_out"), CheckConclusion::TimedOut);
        assert_eq!(parse_check_conclusion("unknown"), CheckConclusion::Neutral);
    }

    #[test]
    fn test_assess_pr_readiness() {
        let config = crate::config::Config::default();
        let monitor = PRMonitor::new(config);

        let passing_checks = vec![
            CheckStatus {
                name: "test".to_string(),
                status: CheckState::Completed,
                conclusion: Some(CheckConclusion::Success),
                url: None,
                started_at: None,
                completed_at: None,
                details_url: None,
            }
        ];

        let merge_status = MergeStatus {
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            merge_state_status: "clean".to_string(),
            has_conflicts: false,
            required_checks_passing: true,
        };

        assert!(monitor.assess_pr_readiness(&passing_checks, &merge_status));
    }
}