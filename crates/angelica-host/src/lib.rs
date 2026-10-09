//! The plugin host that runs in the game process (`runtime.dll`): plugin
//! instances, their permissions and lifecycle, the services of the plugin
//! API, and, on Windows, the hooks and the overlay that connect them to the
//! game.
// The MSVC linker reports the import library it writes for `runtime.dll`
// on every build, in the system's language.
#![allow(linker_messages)]
pub mod author;
pub mod catalog;
pub mod chat;
pub mod commands;
pub mod config;
pub mod error;
pub mod events;
pub mod excel_text;
pub mod file_formats;
pub mod file_replacements;
pub mod game;
pub mod game_data;
pub mod host;
pub mod http;
pub mod icons;
pub mod installer;
pub mod jobs;
pub mod manifest;
#[cfg(test)]
mod mutations;
pub mod package;
pub mod panel;
pub mod sandbox;
pub mod sheets;
pub mod state;
pub mod storage;
pub mod text_case;
pub mod ui;
pub mod wasi;

#[cfg(windows)]
#[allow(unsafe_code)]
pub mod interop;

include!(concat!(env!("OUT_DIR"), "/release_key.rs"));
