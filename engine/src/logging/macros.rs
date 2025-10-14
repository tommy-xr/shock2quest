// Profile macro is defined in macros.rs - this file contains additional logging utilities

/// Convenience macro for scoped logging at different levels
#[macro_export]
macro_rules! scoped_log {
    ($level:ident, $scope:expr, $($arg:tt)*) => {
        let log_config = $crate::logging::get_log_config();
        if log_config.should_log($scope, $crate::logging::Level::$level) {
            tracing::$level!(scope = $scope, $($arg)*);
        }
    };
}

// Convenience macros for common scopes
#[macro_export]
macro_rules! physics_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "physics", $($arg)*);
    };
}

#[macro_export]
macro_rules! audio_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "audio", $($arg)*);
    };
}

#[macro_export]
macro_rules! render_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "render", $($arg)*);
    };
}

#[macro_export]
macro_rules! game_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "game", $($arg)*);
    };
}

#[macro_export]
macro_rules! mission_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "mission", $($arg)*);
    };
}

#[macro_export]
macro_rules! script_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "script", $($arg)*);
    };
}

#[macro_export]
macro_rules! assets_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "assets", $($arg)*);
    };
}

#[macro_export]
macro_rules! input_log {
    ($level:ident, $($arg:tt)*) => {
        $crate::scoped_log!($level, "input", $($arg)*);
    };
}