//! The plugin host that runs in the game process (`runtime.dll`): plugin
//! instances, their permissions and lifecycle, the services of the plugin
//! API, and, on Windows, the hooks and the overlay that connect them to the
//! game.
pub mod catalog;
pub mod config;
pub mod error;
pub mod excel_text;
pub mod game;
pub mod game_data;
pub mod host;
pub mod jobs;
pub mod manifest;
pub mod package;
pub mod panel;
pub mod sandbox;
pub mod storage;
pub mod text_case;
pub mod ui;
pub mod wasi;

#[cfg(windows)]
#[allow(unsafe_code)]
pub mod interop;

include!(concat!(env!("OUT_DIR"), "/release_key.rs"));
