#![macro_use]

// Macro from: https://github.com/bwasty/learn-opengl-rs

/// Get offset to struct member, similar to `offset_of` in C/C++
/// From https://stackoverflow.com/questions/40310483/how-to-get-pointer-offset-in-bytes/40310851#40310851
macro_rules! offset_of {
    ($ty:ty, $field:ident) => {
        //  Undefined Behavior: dereferences a null pointer.
        //  Undefined Behavior: accesses field outside of valid memory area.
        unsafe { &(*(0 as *const $ty)).$field as *const _ as usize }
    };
}

use std::time::Instant;

#[macro_export]
macro_rules! profile {
    ($description:expr, $block:expr) => {{
        let start = Instant::now();
        let result = $block;
        let duration = start.elapsed();
        println!("[{}]: Time elapsed: {:?}", $description, duration);
        result
    }};
}
