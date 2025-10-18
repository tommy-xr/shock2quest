use std::fmt;
use std::io;
use std::process::ExitStatus;

/// Comprehensive error types for Shodan operations
#[derive(Debug)]
pub enum ShodanError {
    /// Configuration related errors
    Config {
        source: anyhow::Error,
        context: String,
    },

    /// Git operation failures
    Git {
        operation: String,
        exit_code: Option<i32>,
        stderr: String,
    },

    /// GitHub API/CLI failures
    GitHub {
        operation: String,
        exit_code: Option<i32>,
        stderr: String,
        retry_possible: bool,
    },

    /// Network connectivity issues
    Network {
        operation: String,
        source: Option<anyhow::Error>,
        retry_after: Option<std::time::Duration>,
    },

    /// Claude Code execution failures
    ClaudeCode {
        session_id: String,
        phase: String,
        source: anyhow::Error,
        timeout: bool,
    },

    /// Prompt related errors
    Prompt { prompt_file: String, issue: String },

    /// PR monitoring failures
    PrMonitoring {
        pr_number: u32,
        issue: String,
        recoverable: bool,
    },

    /// I/O operation failures
    Io {
        operation: String,
        source: io::Error,
    },

    /// Orchestration cycle failures
    Orchestration {
        cycle_id: String,
        phase: String,
        source: anyhow::Error,
        should_retry: bool,
    },

    /// Timeout errors
    Timeout {
        operation: String,
        duration: std::time::Duration,
    },

    /// Validation failures
    Validation { item: String, reason: String },
}

impl fmt::Display for ShodanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShodanError::Config { context, .. } => {
                write!(f, "Configuration error: {}", context)
            }
            ShodanError::Git {
                operation,
                exit_code,
                stderr,
            } => {
                write!(f, "Git operation '{}' failed", operation)?;
                if let Some(code) = exit_code {
                    write!(f, " (exit code: {})", code)?;
                }
                if !stderr.is_empty() {
                    write!(f, ": {}", stderr)?;
                }
                Ok(())
            }
            ShodanError::GitHub {
                operation,
                retry_possible,
                ..
            } => {
                write!(f, "GitHub operation '{}' failed", operation)?;
                if *retry_possible {
                    write!(f, " (retry possible)")?;
                }
                Ok(())
            }
            ShodanError::Network {
                operation,
                retry_after,
                ..
            } => {
                write!(f, "Network error during '{}'", operation)?;
                if let Some(duration) = retry_after {
                    write!(f, " (retry after {:?})", duration)?;
                }
                Ok(())
            }
            ShodanError::ClaudeCode {
                session_id,
                phase,
                timeout,
                ..
            } => {
                if *timeout {
                    write!(
                        f,
                        "Claude Code session '{}' timed out during phase '{}'",
                        session_id, phase
                    )
                } else {
                    write!(
                        f,
                        "Claude Code session '{}' failed during phase '{}'",
                        session_id, phase
                    )
                }
            }
            ShodanError::Prompt { prompt_file, issue } => {
                write!(f, "Prompt error in '{}': {}", prompt_file, issue)
            }
            ShodanError::PrMonitoring {
                pr_number,
                issue,
                recoverable,
            } => {
                write!(f, "PR #{} monitoring failed: {}", pr_number, issue)?;
                if *recoverable {
                    write!(f, " (recoverable)")?;
                }
                Ok(())
            }
            ShodanError::Io { operation, source } => {
                write!(f, "I/O error during '{}': {}", operation, source)
            }
            ShodanError::Orchestration {
                cycle_id,
                phase,
                should_retry,
                ..
            } => {
                write!(
                    f,
                    "Orchestration cycle '{}' failed during phase '{}'",
                    cycle_id, phase
                )?;
                if *should_retry {
                    write!(f, " (will retry)")?;
                }
                Ok(())
            }
            ShodanError::Timeout {
                operation,
                duration,
            } => {
                write!(
                    f,
                    "Operation '{}' timed out after {:?}",
                    operation, duration
                )
            }
            ShodanError::Validation { item, reason } => {
                write!(f, "Validation failed for '{}': {}", item, reason)
            }
        }
    }
}

impl std::error::Error for ShodanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ShodanError::Config { source, .. } => Some(source.as_ref()),
            ShodanError::Network {
                source: Some(source),
                ..
            } => Some(source.as_ref()),
            ShodanError::ClaudeCode { source, .. } => Some(source.as_ref()),
            ShodanError::Io { source, .. } => Some(source),
            ShodanError::Orchestration { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl ShodanError {
    /// Check if this error indicates a temporary condition that could be retried
    pub fn is_retryable(&self) -> bool {
        match self {
            ShodanError::GitHub { retry_possible, .. } => *retry_possible,
            ShodanError::Network { .. } => true,
            ShodanError::PrMonitoring { recoverable, .. } => *recoverable,
            ShodanError::Orchestration { should_retry, .. } => *should_retry,
            ShodanError::Timeout { .. } => true,
            ShodanError::Io { source, .. } => {
                // Some I/O errors are retryable (e.g., temporary file locks)
                matches!(
                    source.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::TimedOut
                )
            }
            _ => false,
        }
    }

    /// Get suggested retry delay for retryable errors
    pub fn retry_delay(&self) -> Option<std::time::Duration> {
        match self {
            ShodanError::GitHub { .. } => Some(std::time::Duration::from_secs(30)),
            ShodanError::Network { retry_after, .. } => {
                retry_after.or(Some(std::time::Duration::from_secs(60)))
            }
            ShodanError::PrMonitoring { .. } => Some(std::time::Duration::from_secs(300)), // 5 minutes
            ShodanError::Timeout { .. } => Some(std::time::Duration::from_secs(120)),
            _ => None,
        }
    }

    /// Convert from git command failure
    pub fn from_git_failure(operation: &str, status: ExitStatus, stderr: &str) -> Self {
        ShodanError::Git {
            operation: operation.to_string(),
            exit_code: status.code(),
            stderr: stderr.to_string(),
        }
    }

    /// Convert from GitHub CLI failure
    pub fn from_github_failure(operation: &str, status: ExitStatus, stderr: &str) -> Self {
        let retry_possible = stderr.contains("rate limit")
            || stderr.contains("timeout")
            || stderr.contains("network")
            || stderr.contains("connection")
            || status.code().map_or(false, |c| c == 22); // HTTP 404/422 might be temporary

        ShodanError::GitHub {
            operation: operation.to_string(),
            exit_code: status.code(),
            stderr: stderr.to_string(),
            retry_possible,
        }
    }

    /// Convert from I/O error with context
    pub fn from_io_error(operation: &str, error: io::Error) -> Self {
        ShodanError::Io {
            operation: operation.to_string(),
            source: error,
        }
    }
}

/// Result type for Shodan operations
pub type ShodanResult<T> = Result<T, ShodanError>;

/// Retry configuration for error recovery
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay: std::time::Duration,
    pub max_delay: std::time::Duration,
    pub exponential_backoff: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: std::time::Duration::from_secs(5),
            max_delay: std::time::Duration::from_secs(300),
            exponential_backoff: true,
        }
    }
}

impl RetryConfig {
    /// Calculate delay for a given attempt number (0-based)
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        if !self.exponential_backoff {
            return self.initial_delay;
        }

        let delay = self.initial_delay.as_secs() * 2_u64.pow(attempt);
        let delay = std::cmp::min(delay, self.max_delay.as_secs());
        std::time::Duration::from_secs(delay)
    }
}

/// Wrapper for retry logic
pub async fn retry_operation<T, F, Fut>(
    operation_name: &str,
    config: &RetryConfig,
    operation: F,
) -> ShodanResult<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ShodanResult<T>>,
{
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) => {
                if !error.is_retryable() || attempt == config.max_attempts - 1 {
                    return Err(error);
                }

                tracing::warn!(
                    operation = operation_name,
                    attempt = attempt + 1,
                    max_attempts = config.max_attempts,
                    error = %error,
                    "Operation failed, will retry"
                );

                let delay = error
                    .retry_delay()
                    .unwrap_or_else(|| config.delay_for_attempt(attempt));

                tokio::time::sleep(delay).await;
                last_error = Some(error);
            }
        }
    }

    // This should never be reached due to the loop logic above, but just in case
    Err(last_error.unwrap_or_else(|| ShodanError::Validation {
        item: "retry_operation".to_string(),
        reason: "Unexpected retry loop exit".to_string(),
    }))
}
