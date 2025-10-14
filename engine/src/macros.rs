#![macro_use]

// Macro from: https://github.com/bwasty/learn-opengl-rs

/// Get offset to struct member, similar to `offset_of` in C/C++
macro_rules! offset_of {
    ($ty:ty, $field:ident) => {
        std::mem::offset_of!($ty, $field)
    };
}


/// Enhanced profile macro with scope and level awareness
///
/// Usage:
/// ```
/// profile!(scope: "physics", level: debug, "collision_detection", {
///     // expensive computation
/// });
/// ```
#[macro_export]
macro_rules! profile {
    // New scope-aware version
    (scope: $scope:expr, level: $level:ident, $description:expr, $block:expr) => {{
        let log_config = $crate::logging::get_log_config();
        if log_config.should_log($scope, tracing::Level::$level) {
            let start = std::time::Instant::now();
            let result = $block;
            let duration = start.elapsed();
            tracing::event!(tracing::Level::$level, scope = $scope, duration = ?duration, "{}", $description);
            result
        } else {
            $block
        }
    }};

    // Backwards compatibility - old macro interface, defaults to "performance" scope and DEBUG level
    ($description:expr, $block:expr) => {{
        let log_config = $crate::logging::get_log_config();
        if log_config.should_log("performance", tracing::Level::DEBUG) {
            let start = std::time::Instant::now();
            let result = $block;
            let duration = start.elapsed();
            tracing::event!(tracing::Level::DEBUG, scope = "performance", duration = ?duration, "{}", $description);
            result
        } else {
            $block
        }
    }};
}
