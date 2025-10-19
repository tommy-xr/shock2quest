use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::prompts::Prompt;

/// Enumeration of supported automation agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude Code",
            AgentKind::Codex => "Codex Code",
        }
    }
}

impl Default for AgentKind {
    fn default() -> Self {
        AgentKind::Claude
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "claude" => Ok(AgentKind::Claude),
            "codex" => Ok(AgentKind::Codex),
            other => Err(anyhow::anyhow!("Unsupported agent '{}'", other)),
        }
    }
}

/// Output returned by an automation agent after executing a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    pub success: bool,
    pub session_id: String,
    pub output: String,
    pub error: Option<String>,
    pub execution_time_seconds: f64,
    pub files_created: Vec<std::path::PathBuf>,
    pub files_modified: Vec<std::path::PathBuf>,
    pub git_changes: Option<GitChanges>,
}

/// Representation of git changes emitted by an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitChanges {
    pub branch_created: Option<String>,
    pub commits: Vec<String>,
    pub pr_created: Option<u32>,
}

/// Current status of an automation session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Starting,
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

#[async_trait]
pub trait AutomationAgent: Send {
    fn display_name(&self) -> &'static str;

    fn process_identifier(&self) -> &'static str;

    async fn start_session(&mut self, prompt: &Prompt) -> Result<String>;

    async fn wait_for_completion(&mut self, session_id: &str) -> Result<AgentOutput>;

    fn cleanup_completed_sessions(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_kind_case_insensitive() {
        assert_eq!(AgentKind::Claude, "CLAUDE".parse().unwrap());
        assert_eq!(AgentKind::Codex, "codex".parse().unwrap());
    }

    #[test]
    fn formats_agent_kind() {
        assert_eq!(AgentKind::Claude.to_string(), "claude");
        assert_eq!(AgentKind::Codex.to_string(), "codex");
    }
}
