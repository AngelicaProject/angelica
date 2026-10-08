//! The dinput8 proxy and fail-closed verification of the native runtime.
pub mod trust;

#[cfg(windows)]
mod windows;

include!(concat!(env!("OUT_DIR"), "/release_key.rs"));
