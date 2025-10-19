use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command as TokioCommand};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::prompts::Prompt;

const DEFAULT_SYSTEM_PROMPT: &str = r#"# Shodan Automation Context

This session is running under Shodan automation with the following constraints:

## Safety Guidelines
- Only make incremental, safe improvements
- Do not modify core VR functionality without thorough understanding
- Focus on documentation, testing, and minor improvements
- Always test changes before committing
- Because this is automation, bias towards making decisions without user intervention.
- Keep changes as simple as possible.

## Project Context
- This is a VR port of System Shock 2 for Oculus Quest
- Written in Rust with OpenGL rendering
- Performance is critical for VR (90+ FPS)
- Follow existing code patterns and conventions

## Workflow
- Once you have decided on a work item, create a new branch with git
  - If there is pending work in a PR that you are working off of, use that latest branch
  - Otherwise, based your new branch on main
  - Use 'gt track' once the branch is created so graphite is aware of it
  - When the atom of work is complete:, make sure to update the issue, project description, docs, etc as well as part of the change.
  - make sure to update the issue, project description, docs, etc as well as part of the change.
  - Push a PR up with all of the changes - make sure the base is relative to the branch you worked off of
  - If you identify an issue or project that is outside the scope of the current work stream, avoid scope creep, but you may do one of the following:
      - Add a TODO item in the codebase (small tasks)
      - Open an issue against the codebase (medium task) - provide as much context as possible
      - Start a new file in projects to document the project (large task)

## Summarization & Continuous Improvement
Once the workstream is complete, append a journal entry to .notes/journal.md, containing:
- A single sentence describing the work done.
- A single sentence for continuous improvement - a piece of data that you learned that would've been useful, a suggestion for prompt improvement, or a tool that could've assisted.
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeInput {
    pub prompt: String,
    pub context: Option<String>,
    pub working_directory: Option<PathBuf>,
    pub timeout_seconds: Option<u64>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeOutput {
    pub success: bool,
    pub session_id: String,
    pub output: String,
    pub error: Option<String>,
    pub execution_time_seconds: f64,
    pub files_created: Vec<PathBuf>,
    pub files_modified: Vec<PathBuf>,
    pub git_changes: Option<GitChanges>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitChanges {
    pub branch_created: Option<String>,
    pub commits: Vec<String>,
    pub pr_created: Option<u32>,
}

#[derive(Debug)]
pub struct ClaudeCodeSession {
    pub id: String,
    pub start_time: Instant,
    pub prompt: Prompt,
    pub status: SessionStatus,
    pub process: Option<Child>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Starting,
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

pub struct ClaudeCodeManager {
    config: Config,
    active_sessions: Vec<ClaudeCodeSession>,
}

impl ClaudeCodeManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            active_sessions: Vec::new(),
        }
    }

    /// Start a new Claude Code session with a prompt
    pub async fn start_session(&mut self, prompt: &Prompt) -> Result<String> {
        let session_id = generate_session_id();
        info!("Starting Claude Code session: {}", session_id);

        // Prepare the input for Claude Code
        let claude_input = self.prepare_input(prompt, &session_id).await?;

        // Get timeout from config
        let timeout_duration = Duration::from_secs(
            self.config
                .parse_session_time()
                .context("Failed to parse session timeout")?,
        );

        let session = ClaudeCodeSession {
            id: session_id.clone(),
            start_time: Instant::now(),
            prompt: prompt.clone(),
            status: SessionStatus::Starting,
            process: None,
            working_directory: std::env::current_dir()?,
            timeout: timeout_duration,
        };

        // Execute Claude Code
        let process = self.execute_claude_code(&claude_input).await?;

        let mut session = session;
        session.process = Some(process);
        session.status = SessionStatus::Running;

        self.active_sessions.push(session);
        info!("Claude Code session {} started successfully", session_id);

        Ok(session_id)
    }

    /// Prepare the JSON input for Claude Code
    async fn prepare_input(&self, prompt: &Prompt, session_id: &str) -> Result<ClaudeCodeInput> {
        let formatted_prompt = crate::prompts::format_prompt_for_execution(prompt);

        // Add context about the repository and current state
        let context = self.generate_context().await?;

        let input = ClaudeCodeInput {
            prompt: formatted_prompt,
            context: Some(context),
            working_directory: Some(std::env::current_dir()?),
            timeout_seconds: Some(self.config.parse_session_time()?),
            session_id: Some(session_id.to_string()),
        };

        debug!("Prepared Claude Code input for session: {}", session_id);
        Ok(input)
    }

    /// Generate context information for Claude Code
    async fn generate_context(&self) -> Result<String> {
        let mut context = self.load_system_prompt().await;

        if !context.ends_with('\n') {
            context.push('\n');
        }

        // Add current repository state
        if let Ok(repo_state) = crate::git::get_repository_state().await {
            context.push_str(&format!("## Current Repository State\n"));
            context.push_str(&format!(
                "- Branch: {}\n",
                repo_state.git_status.current_branch
            ));
            context.push_str(&format!("- Clean: {}\n", repo_state.git_status.is_clean));
            context.push_str(&format!("- Open PRs: {}\n", repo_state.open_prs.len()));
            context.push_str("\n");
        }

        Ok(context)
    }

    async fn load_system_prompt(&self) -> String {
        let mut candidates = Vec::new();

        if let Some(root) = find_repo_root() {
            candidates.push(root.join(".shodan").join("system_prompt.md"));
        }

        if let Ok(current_dir) = std::env::current_dir() {
            candidates.push(current_dir.join(".shodan").join("system_prompt.md"));
            candidates.push(current_dir.join("system_prompt.md"));
        }

        let mut seen = HashSet::new();
        for candidate in candidates
            .into_iter()
            .filter(|path| seen.insert(path.clone()))
        {
            match fs::read_to_string(&candidate).await {
                Ok(content) => {
                    if content.trim().is_empty() {
                        warn!(
                            "System prompt file at {} is empty; using default fallback",
                            candidate.display()
                        );
                    } else {
                        debug!("Loaded system prompt from {}", candidate.display());
                        return content;
                    }
                }
                Err(err) => {
                    if err.kind() != ErrorKind::NotFound {
                        warn!(
                            "Failed to read system prompt from {}: {}",
                            candidate.display(),
                            err
                        );
                    } else {
                        debug!(
                            "System prompt not found at {}; continuing to next candidate",
                            candidate.display()
                        );
                    }
                }
            }
        }

        debug!("Using default embedded system prompt");
        DEFAULT_SYSTEM_PROMPT.to_string()
    }

    /// Execute Claude Code as a subprocess
    async fn execute_claude_code(&self, input: &ClaudeCodeInput) -> Result<Child> {
        debug!("Executing Claude Code with text input and JSON output");
        let output_format = if self.config.shodan.show_claude_output {
            "text"
        } else {
            "json"
        };
        info!(
            "Claude Code command: claude --print --output-format={} --permission-mode={}",
            output_format, self.config.shodan.permission_mode
        );

        // Prepare text input (Claude Code expects text, not JSON when using --print)
        let input_text = format!(
            "{}\n\n{}",
            input.context.as_deref().unwrap_or(""),
            input.prompt
        );

        // Start Claude Code process with configurable permission mode
        let permission_arg = format!("--permission-mode={}", self.config.shodan.permission_mode);
        let mut args = vec!["--print", &permission_arg];

        // Use JSON output for parsing, or text output for visibility
        if !self.config.shodan.show_claude_output {
            args.push("--output-format=json");
        }

        let mut process = TokioCommand::new("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start Claude Code process. Make sure 'claude' command is available in PATH.")?;

        // Send input to Claude Code
        if let Some(mut stdin) = process.stdin.take() {
            stdin
                .write_all(input_text.as_bytes())
                .await
                .context("Failed to write input to Claude Code")?;
            stdin.shutdown().await.context("Failed to close stdin")?;
        }

        debug!("Claude Code process started successfully");
        Ok(process)
    }

    /// Check the status of a running session
    pub async fn check_session(&mut self, session_id: &str) -> Result<SessionStatus> {
        let session = self.find_session_mut(session_id)?;

        // Check if session has timed out
        if session.start_time.elapsed() > session.timeout {
            session.status = SessionStatus::TimedOut;
            self.terminate_session(session_id).await?;
            return Ok(SessionStatus::TimedOut);
        }

        // Check process status
        if let Some(process) = &mut session.process {
            match process.try_wait() {
                Ok(Some(exit_status)) => {
                    if exit_status.success() {
                        session.status = SessionStatus::Completed;
                        info!("Claude Code session {} completed successfully", session_id);
                    } else {
                        session.status = SessionStatus::Failed;
                        warn!(
                            "Claude Code session {} failed with exit code: {:?}",
                            session_id,
                            exit_status.code()
                        );
                    }
                }
                Ok(None) => {
                    // Still running
                    session.status = SessionStatus::Running;
                }
                Err(e) => {
                    error!("Error checking Claude Code process: {}", e);
                    session.status = SessionStatus::Failed;
                }
            }
        }

        Ok(session.status.clone())
    }

    /// Wait for a session to complete and get the output
    pub async fn wait_for_completion(&mut self, session_id: &str) -> Result<ClaudeCodeOutput> {
        let (timeout_duration, start_time) = {
            let session = self.find_session_mut(session_id)?;
            (session.timeout, session.start_time)
        };

        info!(
            "Waiting for Claude Code session {} to complete (timeout: {:?})",
            session_id, timeout_duration
        );

        // Extract process from session
        let process = {
            let session = self.find_session_mut(session_id)?;
            session.process.take()
        };

        // Wait for process with timeout
        let output = if let Some(mut process) = process {
            match timeout(timeout_duration, async {
                // Collect output
                let stdout = if let Some(stdout) = process.stdout.take() {
                    let mut reader = BufReader::new(stdout);
                    let mut output = String::new();
                    let mut line = String::new();
                    while reader.read_line(&mut line).await? > 0 {
                        // Stream output in real-time
                        print!("{}", line);
                        use std::io::Write;
                        std::io::stdout().flush().unwrap();

                        output.push_str(&line);
                        line.clear();
                    }
                    output
                } else {
                    String::new()
                };

                let stderr = if let Some(stderr) = process.stderr.take() {
                    let mut reader = BufReader::new(stderr);
                    let mut output = String::new();
                    let mut line = String::new();
                    while reader.read_line(&mut line).await? > 0 {
                        // Stream stderr in real-time with prefix
                        eprint!("[Claude stderr] {}", line);
                        use std::io::Write;
                        std::io::stderr().flush().unwrap();

                        output.push_str(&line);
                        line.clear();
                    }
                    output
                } else {
                    String::new()
                };

                // Wait for process to complete
                let exit_status = process.wait().await?;

                Ok::<(String, String, bool), anyhow::Error>((stdout, stderr, exit_status.success()))
            })
            .await
            {
                Ok(result) => result?,
                Err(_) => {
                    // Timeout occurred
                    let session = self.find_session_mut(session_id)?;
                    session.status = SessionStatus::TimedOut;
                    process.kill().await.ok();
                    return Ok(ClaudeCodeOutput {
                        success: false,
                        session_id: session_id.to_string(),
                        output: String::new(),
                        error: Some("Session timed out".to_string()),
                        execution_time_seconds: timeout_duration.as_secs_f64(),
                        files_created: Vec::new(),
                        files_modified: Vec::new(),
                        git_changes: None,
                    });
                }
            }
        } else {
            return Err(anyhow::anyhow!(
                "No process found for session: {}",
                session_id
            ));
        };

        let execution_time = start_time.elapsed();
        let success = output.2;

        // Parse Claude Code JSON output
        let claude_output = if success && !output.0.is_empty() {
            self.parse_claude_output(&output.0, session_id, execution_time)
                .await?
        } else {
            ClaudeCodeOutput {
                success: false,
                session_id: session_id.to_string(),
                output: output.0,
                error: if output.1.is_empty() {
                    None
                } else {
                    Some(output.1)
                },
                execution_time_seconds: execution_time.as_secs_f64(),
                files_created: Vec::new(),
                files_modified: Vec::new(),
                git_changes: None,
            }
        };

        // Update session status
        let session = self.find_session_mut(session_id)?;
        session.status = if success {
            SessionStatus::Completed
        } else {
            SessionStatus::Failed
        };

        info!(
            "Claude Code session {} finished in {:.2}s (success: {})",
            session_id,
            execution_time.as_secs_f64(),
            success
        );

        Ok(claude_output)
    }

    /// Parse Claude Code JSON output
    async fn parse_claude_output(
        &self,
        output: &str,
        session_id: &str,
        execution_time: Duration,
    ) -> Result<ClaudeCodeOutput> {
        // Try to parse as JSON first
        if let Ok(parsed) = serde_json::from_str::<ClaudeCodeOutput>(output) {
            return Ok(parsed);
        }

        // If JSON parsing fails, create a basic output structure
        warn!(
            "Failed to parse Claude Code output as JSON for session: {}",
            session_id
        );

        Ok(ClaudeCodeOutput {
            success: true, // Assume success if we got output
            session_id: session_id.to_string(),
            output: output.to_string(),
            error: None,
            execution_time_seconds: execution_time.as_secs_f64(),
            files_created: Vec::new(),
            files_modified: Vec::new(),
            git_changes: None,
        })
    }

    /// Terminate a running session
    pub async fn terminate_session(&mut self, session_id: &str) -> Result<()> {
        let session = self.find_session_mut(session_id)?;

        if let Some(mut process) = session.process.take() {
            info!("Terminating Claude Code session: {}", session_id);
            process
                .kill()
                .await
                .context("Failed to kill Claude Code process")?;
        }

        session.status = SessionStatus::Cancelled;
        Ok(())
    }

    /// Get all active sessions
    pub fn get_active_sessions(&self) -> Vec<&ClaudeCodeSession> {
        self.active_sessions.iter().collect()
    }

    /// Clean up completed sessions
    pub fn cleanup_completed_sessions(&mut self) {
        self.active_sessions.retain(|session| {
            !matches!(
                session.status,
                SessionStatus::Completed
                    | SessionStatus::Failed
                    | SessionStatus::TimedOut
                    | SessionStatus::Cancelled
            )
        });
    }

    /// Find a session by ID
    fn find_session_mut(&mut self, session_id: &str) -> Result<&mut ClaudeCodeSession> {
        self.active_sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))
    }
}

/// Generate a unique session ID
fn generate_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("shodan-{}-{:04x}", timestamp, rand::random::<u16>())
}

/// Execute a prompt with Claude Code (convenience function)
pub async fn execute_prompt(config: &Config, prompt: &Prompt) -> Result<ClaudeCodeOutput> {
    let mut manager = ClaudeCodeManager::new(config.clone());
    let session_id = manager.start_session(prompt).await?;
    manager.wait_for_completion(&session_id).await
}

fn find_repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::PromptMetadata;

    #[test]
    fn test_generate_session_id() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();

        assert!(id1.starts_with("shodan-"));
        assert!(id2.starts_with("shodan-"));
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_prepare_input() {
        let config = crate::config::Config::default();
        let manager = ClaudeCodeManager::new(config);

        let prompt = Prompt {
            name: "test.md".to_string(),
            file_path: PathBuf::from("test.md"),
            content: "Test prompt content".to_string(),
            weight: 1,
            metadata: PromptMetadata::default(),
        };

        let input = manager
            .prepare_input(&prompt, "test-session")
            .await
            .unwrap();

        assert!(input.prompt.contains("Test prompt content"));
        assert!(input.context.is_some());
        assert_eq!(input.session_id, Some("test-session".to_string()));
    }
}
