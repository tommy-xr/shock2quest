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
    pub suggested_fixes: Vec<String>,
    pub retry_recommended: bool,
}

impl PRMonitor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            monitored_prs: HashMap::new(),
        }
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

        let output = TokioCommand::new("gh")
            .args(["api", &format!("repos/:owner/:repo/pulls/{}/commits", pr_number)])
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
            .args(["api", &format!("repos/:owner/:repo/commits/{}/check-runs", commit_sha)])
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
        let mut suggested_fixes = Vec::new();

        // Get detailed logs for failed checks
        for check in &failed_checks {
            if let Some(_details_url) = &check.details_url {
                // Try to get more detailed error information
                if let Ok(logs) = self.get_check_logs(pr_number, &check.name).await {
                    error_logs.extend(logs);
                }
            }

            // Generate suggested fixes based on check name and failure type
            let fixes = self.generate_fix_suggestions(&check.name, check);
            suggested_fixes.extend(fixes);
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
            suggested_fixes,
            retry_recommended,
        };

        info!("Failure analysis for PR #{}: {} failed checks, {} suggestions",
              pr_number, analysis.failed_checks.len(), analysis.suggested_fixes.len());

        Ok(analysis)
    }

    /// Get detailed logs for a specific check
    async fn get_check_logs(&self, pr_number: u32, check_name: &str) -> Result<Vec<String>> {
        debug!("Getting logs for check '{}' on PR #{}", check_name, pr_number);

        // First, get the PR's head commit SHA
        let pr_output = TokioCommand::new("gh")
            .args(["pr", "view", &pr_number.to_string(), "--json", "headRefOid"])
            .output()
            .await
            .context("Failed to get PR head commit")?;

        if !pr_output.status.success() {
            return Ok(vec!["Could not get PR information".to_string()]);
        }

        let pr_stdout = String::from_utf8_lossy(&pr_output.stdout);
        let pr_data: serde_json::Value = serde_json::from_str(&pr_stdout)
            .context("Failed to parse PR JSON")?;

        let head_sha = match pr_data["headRefOid"].as_str() {
            Some(sha) => sha,
            None => return Ok(vec!["No head commit SHA found".to_string()]),
        };

        // Get workflow runs for this commit
        let runs_output = TokioCommand::new("gh")
            .args([
                "run", "list",
                "--commit", head_sha,
                "--json", "databaseId,name,status,conclusion,workflowName",
                "--limit", "20"
            ])
            .output()
            .await
            .context("Failed to get workflow runs")?;

        if !runs_output.status.success() {
            return Ok(vec!["Could not get workflow runs".to_string()]);
        }

        let runs_stdout = String::from_utf8_lossy(&runs_output.stdout);
        let runs: Vec<serde_json::Value> = serde_json::from_str(&runs_stdout)
            .context("Failed to parse workflow runs JSON")?;

        // Find the run that matches our check name
        for run in runs {
            let run_name = run["name"].as_str().unwrap_or("");
            let workflow_name = run["workflowName"].as_str().unwrap_or("");

            // Match by check name or workflow name
            if run_name.contains(check_name) || workflow_name.contains(check_name) || check_name.contains(run_name) {
                if run["conclusion"].as_str() == Some("failure") {
                    if let Some(run_id) = run["databaseId"].as_u64() {
                        return self.get_run_failure_logs(&run_id.to_string()).await;
                    }
                }
            }
        }

        Ok(vec![format!("No matching failed workflow run found for check: {}", check_name)])
    }

    /// Get failure logs from a specific workflow run
    async fn get_run_failure_logs(&self, run_id: &str) -> Result<Vec<String>> {
        debug!("Getting failure logs for run ID: {}", run_id);

        // Get the run logs
        let output = TokioCommand::new("gh")
            .args(["run", "view", run_id, "--log-failed"])
            .output()
            .await
            .context("Failed to get run logs")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(vec![format!("Failed to get logs: {}", stderr)]);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse the logs to extract meaningful error messages
        let mut error_lines = Vec::new();
        let mut in_error_section = false;

        for line in stdout.lines() {
            let line = line.trim();

            // Look for common error patterns
            if line.contains("ERROR") || line.contains("FAILED") || line.contains("error:") {
                error_lines.push(line.to_string());
                in_error_section = true;
            } else if line.contains("FAIL:") || line.contains("assertion failed") {
                error_lines.push(line.to_string());
                in_error_section = true;
            } else if line.contains("Build failed") || line.contains("compilation failed") {
                error_lines.push(line.to_string());
                in_error_section = true;
            } else if in_error_section && !line.is_empty() && !line.starts_with("##") {
                // Continue collecting context lines after an error
                error_lines.push(line.to_string());
                if error_lines.len() > 50 {
                    break; // Limit output size
                }
            } else if line.starts_with("##") {
                // New section, stop collecting
                in_error_section = false;
            }
        }

        if error_lines.is_empty() {
            error_lines.push("No specific error messages found in logs".to_string());
        }

        debug!("Extracted {} error lines from run logs", error_lines.len());
        Ok(error_lines)
    }

    /// Generate fix suggestions based on check failures
    fn generate_fix_suggestions(&self, check_name: &str, check: &CheckStatus) -> Vec<String> {
        let mut suggestions = Vec::new();

        match check.conclusion {
            Some(CheckConclusion::Failure) => {
                if check_name.contains("test") {
                    suggestions.push("Review test failures and fix failing tests".to_string());
                    suggestions.push("Check for recent changes that might have broken tests".to_string());
                } else if check_name.contains("build") || check_name.contains("compile") {
                    suggestions.push("Fix compilation errors".to_string());
                    suggestions.push("Check for missing dependencies or build configuration issues".to_string());
                } else if check_name.contains("lint") || check_name.contains("format") {
                    suggestions.push("Run cargo fmt to fix formatting issues".to_string());
                    suggestions.push("Run cargo clippy and fix linting warnings".to_string());
                } else if check_name.contains("security") {
                    suggestions.push("Review security scan results and address vulnerabilities".to_string());
                } else {
                    suggestions.push(format!("Investigate and fix issues in '{}'", check_name));
                }
            }
            Some(CheckConclusion::TimedOut) => {
                suggestions.push("Check for performance issues or infinite loops".to_string());
                suggestions.push("Consider splitting large tests into smaller chunks".to_string());
                suggestions.push("Retry the check as it may have been a temporary issue".to_string());
            }
            Some(CheckConclusion::Cancelled) => {
                suggestions.push("Check why the workflow was cancelled".to_string());
                suggestions.push("Retry the workflow if it was cancelled due to resource constraints".to_string());
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
                    if !analysis.suggested_fixes.is_empty() {
                        info!("Suggested fixes for PR #{}:", pr_number);
                        for fix in &analysis.suggested_fixes {
                            info!("  - {}", fix);
                        }
                    }
                }
            }

            info!("Waiting {} seconds before next check...", check_interval.as_secs());
            sleep(check_interval).await;
        }

        warn!("Timeout waiting for PR #{} to become ready", pr_number);
        Err(anyhow::anyhow!("Timeout waiting for PR to become ready"))
    }

    /// Get monitoring statistics
    pub fn get_monitoring_stats(&self) -> HashMap<u32, MonitoringStats> {
        let mut stats = HashMap::new();

        for (pr_number, state) in &self.monitored_prs {
            let stat = MonitoringStats {
                pr_number: *pr_number,
                monitoring_duration: state.start_time.elapsed(),
                last_check_age: state.last_check.elapsed(),
                status_updates: state.status_history.len(),
                failure_count: state.failure_count,
            };
            stats.insert(*pr_number, stat);
        }

        stats
    }
}

#[derive(Debug, Clone)]
pub struct MonitoringStats {
    pub pr_number: u32,
    pub monitoring_duration: Duration,
    pub last_check_age: Duration,
    pub status_updates: usize,
    pub failure_count: u32,
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