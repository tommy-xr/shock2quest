pub mod config;
pub mod macros;

pub use config::{LogConfig, init_logging};
pub use tracing::{Level, debug, error, info, trace, warn};

use once_cell::sync::Lazy;
use std::sync::OnceLock;

static LOG_CONFIG: OnceLock<LogConfig> = OnceLock::new();
static DEFAULT_CONFIG: Lazy<LogConfig> = Lazy::new(LogConfig::default);

pub fn get_log_config() -> &'static LogConfig {
    LOG_CONFIG.get().unwrap_or(&DEFAULT_CONFIG)
}

pub(crate) fn set_log_config(config: LogConfig) {
    LOG_CONFIG.set(config).ok();
}
