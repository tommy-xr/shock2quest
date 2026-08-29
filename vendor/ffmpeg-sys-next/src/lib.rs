#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::approx_constant)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::redundant_static_lifetimes)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::ptr_offset_with_cast)]
#![allow(unpredictable_function_pointer_comparisons)]
#![allow(unnecessary_transmutes)]
// bindgen emits declarations for the libc string/memory routines pulled in by
// ffmpeg's headers (memcpy, memset, strlen, ...) typed with `libc::c_ulong`
// rather than `usize`. Newer rustc flags that as a mismatched redeclaration of
// a runtime symbol the standard library owns; the types are layout-identical,
// so allow it. `unknown_lints` keeps older toolchains (which do not know the
// lint name) from failing on the allow itself under `-D warnings`.
#![allow(unknown_lints)]
#![allow(suspicious_runtime_symbol_definitions)]

extern crate libc;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[macro_use]
mod avutil;
pub use avutil::*;
