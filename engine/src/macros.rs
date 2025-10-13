#![macro_use]

// Macro from: https://github.com/bwasty/learn-opengl-rs

/// Get offset to struct member, similar to `offset_of` in C/C++
macro_rules! offset_of {
    ($ty:ty, $field:ident) => {
        std::mem::offset_of!($ty, $field)
    };
}


#[macro_export]
macro_rules! profile {
    ($description:expr, $block:expr) => {{
        let start = std::time::Instant::now();
        let result = $block;
        let duration = start.elapsed();
        println!("[{}]: Time elapsed: {:?}", $description, duration);
        result
    }};
}
