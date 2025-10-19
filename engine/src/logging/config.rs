use std::collections::HashMap;
use tracing::Level;

#[derive(Debug, Clone)]
pub struct LogConfig {
    global_level: Level,
    scope_levels: HashMap<String, Level>,
}

impl LogConfig {
    pub fn new() -> Self {
        Self {
            global_level: Level::WARN,
            scope_levels: HashMap::new(),
        }
    }

    pub fn from_env(env_var_name: &str) -> Self {
        let mut config = Self::new();

        if let Ok(log_config) = std::env::var(env_var_name) {
            config.parse_config_string(&log_config);
        }

        config
    }

    fn parse_config_string(&mut self, config_str: &str) {
        let parts: Vec<&str> = config_str.split(',').collect();

        for part in parts {
            let part = part.trim();

            if part.contains('=') {
                // Scope-specific level: scope=level
                let scope_parts: Vec<&str> = part.splitn(2, '=').collect();
                if scope_parts.len() == 2 {
                    let scope = scope_parts[0].trim();
                    if let Ok(level) = parse_level(scope_parts[1].trim()) {
                        self.scope_levels.insert(scope.to_string(), level);
                    }
                }
            } else {
                // Global level
                if let Ok(level) = parse_level(part) {
                    self.global_level = level;
                }
            }
        }
    }

    pub fn should_log(&self, scope: &str, level: Level) -> bool {
        let target_level = self.scope_levels.get(scope).unwrap_or(&self.global_level);
        level <= *target_level
    }

    pub fn set_global_level(&mut self, level: Level) {
        self.global_level = level;
    }

    pub fn set_scope_level(&mut self, scope: String, level: Level) {
        self.scope_levels.insert(scope, level);
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_level(level_str: &str) -> Result<Level, ()> {
    match level_str.to_lowercase().as_str() {
        "error" => Ok(Level::ERROR),
        "warn" => Ok(Level::WARN),
        "info" => Ok(Level::INFO),
        "debug" => Ok(Level::DEBUG),
        "trace" => Ok(Level::TRACE),
        _ => Err(()),
    }
}

/// Initialize logging with the specified environment variable name
/// This allows different games/runtimes to use their own environment variables
/// Example: init_logging("SHOCK2_LOG") or init_logging("THIEF_LOG")
pub fn init_logging(env_var_name: &str) -> LogConfig {
    // Initialize the tracing subscriber if not already initialized
    use tracing_subscriber;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let config = LogConfig::from_env(env_var_name);
    super::set_log_config(config.clone());
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_global_level() {
        let mut config = LogConfig::new();
        config.parse_config_string("debug");
        assert_eq!(config.global_level, Level::DEBUG);
    }

    #[test]
    fn test_parse_scope_levels() {
        let mut config = LogConfig::new();
        config.parse_config_string("warn,physics=debug,render=trace");

        assert_eq!(config.global_level, Level::WARN);
        assert_eq!(config.scope_levels.get("physics"), Some(&Level::DEBUG));
        assert_eq!(config.scope_levels.get("render"), Some(&Level::TRACE));
    }

    #[test]
    fn test_should_log() {
        let mut config = LogConfig::new();
        config.global_level = Level::WARN;
        config
            .scope_levels
            .insert("physics".to_string(), Level::DEBUG);

        // Global level filtering
        assert!(config.should_log("unknown", Level::ERROR));
        assert!(config.should_log("unknown", Level::WARN));
        assert!(!config.should_log("unknown", Level::INFO));

        // Scope-specific level filtering
        assert!(config.should_log("physics", Level::ERROR));
        assert!(config.should_log("physics", Level::DEBUG));
        assert!(!config.should_log("physics", Level::TRACE));
    }
}
