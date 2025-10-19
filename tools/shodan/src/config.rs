use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::agent::AgentKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub shodan: ShodanConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShodanConfig {
    // Scheduling
    pub interval: String,
    pub max_session_time: String,
    #[serde(default)]
    pub default_agent: AgentKind,

    // Git settings
    pub main_branch: String,
    pub sync_command: String,

    // GitHub settings
    pub check_interval: String,
    pub max_ci_wait_time: String,

    // Prompt settings
    pub prompt_dir: String,
    pub prompt_weights: HashMap<String, u32>,

    // Claude Code settings
    pub permission_mode: String,
    pub show_claude_output: bool,
}

impl Default for ShodanConfig {
    fn default() -> Self {
        let mut prompt_weights = HashMap::new();
        prompt_weights.insert("iterate-on-projects.md".to_string(), 3);
        prompt_weights.insert("iterate-on-issues.md".to_string(), 2);
        prompt_weights.insert("check-pr-state.md".to_string(), 1);
        prompt_weights.insert("improve-documentation.md".to_string(), 2);
        prompt_weights.insert("optimize-performance.md".to_string(), 2);

        Self {
            interval: "1h".to_string(),
            max_session_time: "4h".to_string(),
            default_agent: AgentKind::Claude,
            main_branch: "main".to_string(),
            sync_command: "gt sync".to_string(),
            check_interval: "5m".to_string(),
            max_ci_wait_time: "30m".to_string(),
            prompt_dir: "prompts".to_string(),
            prompt_weights,
            permission_mode: "bypassPermissions".to_string(),
            show_claude_output: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shodan: ShodanConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from file or use defaults, with environment variable overrides
    pub async fn load(config_path: Option<&Path>) -> Result<Self> {
        let mut config = if let Some(path) = config_path {
            Self::load_from_file(path).await?
        } else {
            // Try to load from default locations
            let default_paths = [
                ".shodan/shodan.toml",
                "shodan.toml",
                "tools/shodan/shodan.toml",
                ".shodan.toml",
            ];

            let mut found_config = None;
            for path in &default_paths {
                let path = Path::new(path);
                if path.exists() {
                    found_config = Some(Self::load_from_file(path).await?);
                    break;
                }
            }

            found_config.unwrap_or_else(|| Self::default())
        };

        // Apply environment variable overrides
        config.apply_env_overrides()?;
        Ok(config)
    }

    /// Apply environment variable overrides to configuration
    fn apply_env_overrides(&mut self) -> Result<()> {
        // Scheduling overrides
        if let Ok(val) = std::env::var("SHODAN_INTERVAL") {
            self.shodan.interval = val;
        }
        if let Ok(val) = std::env::var("SHODAN_MAX_SESSION_TIME") {
            self.shodan.max_session_time = val;
        }
        if let Ok(val) = std::env::var("SHODAN_AGENT") {
            self.shodan.default_agent = val
                .parse()
                .with_context(|| format!("Invalid SHODAN_AGENT value: {}", val))?;
        }

        // Git overrides
        if let Ok(val) = std::env::var("SHODAN_MAIN_BRANCH") {
            self.shodan.main_branch = val;
        }
        if let Ok(val) = std::env::var("SHODAN_SYNC_COMMAND") {
            self.shodan.sync_command = val;
        }

        // GitHub overrides
        if let Ok(val) = std::env::var("SHODAN_CHECK_INTERVAL") {
            self.shodan.check_interval = val;
        }
        if let Ok(val) = std::env::var("SHODAN_MAX_CI_WAIT_TIME") {
            self.shodan.max_ci_wait_time = val;
        }

        // Prompt overrides
        if let Ok(val) = std::env::var("SHODAN_PROMPT_DIR") {
            self.shodan.prompt_dir = val;
        }

        // Claude Code overrides
        if let Ok(val) = std::env::var("SHODAN_PERMISSION_MODE") {
            self.shodan.permission_mode = val;
        }
        if let Ok(val) = std::env::var("SHODAN_SHOW_CLAUDE_OUTPUT") {
            self.shodan.show_claude_output = val.parse().with_context(|| {
                format!(
                    "Invalid boolean value for SHODAN_SHOW_CLAUDE_OUTPUT: {}",
                    val
                )
            })?;
        }

        // Prompt weights overrides (format: "file1=weight1,file2=weight2")
        if let Ok(val) = std::env::var("SHODAN_PROMPT_WEIGHTS") {
            let weights: Result<HashMap<String, u32>, _> = val
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|pair| {
                    let parts: Vec<&str> = pair.split('=').collect();
                    if parts.len() != 2 {
                        return Err(anyhow::anyhow!("Invalid prompt weight format: {}", pair));
                    }
                    let weight = parts[1]
                        .parse::<u32>()
                        .with_context(|| format!("Invalid weight value: {}", parts[1]))?;
                    Ok((parts[0].to_string(), weight))
                })
                .collect();

            match weights {
                Ok(weights) => {
                    // Replace existing weights with environment values
                    self.shodan.prompt_weights = weights;
                }
                Err(e) => {
                    tracing::warn!("Invalid SHODAN_PROMPT_WEIGHTS format: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Save configuration to file
    pub async fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        fs::write(path, content)
            .await
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Get the absolute path to the prompts directory
    pub fn prompts_dir(&self) -> PathBuf {
        let prompt_dir = &self.shodan.prompt_dir;

        // If it's already absolute, use as-is
        if Path::new(prompt_dir).is_absolute() {
            PathBuf::from(prompt_dir)
        } else {
            // Try current directory first, then .shodan/
            let current_dir_path = PathBuf::from(prompt_dir);
            if current_dir_path.exists() {
                current_dir_path
            } else {
                // Fallback to .shodan/ relative path
                PathBuf::from(".shodan").join(prompt_dir)
            }
        }
    }

    /// Parse interval string to duration in seconds
    pub fn parse_interval(&self, interval_str: &str) -> Result<u64> {
        parse_duration(interval_str)
    }

    /// Parse session time string to duration in seconds
    pub fn parse_session_time(&self) -> Result<u64> {
        parse_duration(&self.shodan.max_session_time)
    }

    /// Parse check interval to duration in seconds
    pub fn parse_check_interval(&self) -> Result<u64> {
        parse_duration(&self.shodan.check_interval)
    }

    /// Parse CI wait time to duration in seconds
    pub fn parse_ci_wait_time(&self) -> Result<u64> {
        parse_duration(&self.shodan.max_ci_wait_time)
    }

    /// Parse the orchestration interval from config
    pub fn parse_orchestration_interval(&self) -> Result<u64> {
        parse_duration(&self.shodan.interval)
    }

    /// Get the prompt directory path
    pub fn prompt_dir(&self) -> PathBuf {
        PathBuf::from(&self.shodan.prompt_dir)
    }
}

/// Parse duration strings like "1h", "30m", "45s" into seconds
fn parse_duration(duration_str: &str) -> Result<u64> {
    let duration_str = duration_str.trim();

    if duration_str.is_empty() {
        return Err(anyhow::anyhow!("Empty duration string"));
    }

    let (number_part, unit_part) = if let Some(pos) = duration_str.find(|c: char| c.is_alphabetic())
    {
        (&duration_str[..pos], &duration_str[pos..])
    } else {
        // If no unit, assume seconds
        (duration_str, "s")
    };

    let number: u64 = number_part
        .parse()
        .with_context(|| format!("Invalid number in duration: {}", number_part))?;

    let multiplier = match unit_part.to_lowercase().as_str() {
        "s" | "sec" | "second" | "seconds" => 1,
        "m" | "min" | "minute" | "minutes" => 60,
        "h" | "hr" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86400,
        _ => return Err(anyhow::anyhow!("Unknown duration unit: {}", unit_part)),
    };

    Ok(number * multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("1d").unwrap(), 86400);
        assert_eq!(parse_duration("60").unwrap(), 60); // No unit defaults to seconds
    }

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.shodan.interval, "1h");
        assert_eq!(config.shodan.main_branch, "main");
        assert!(config
            .shodan
            .prompt_weights
            .contains_key("iterate-on-projects.md"));
    }
}
