//! The plugin host that runs in the game process (`runtime.dll`): plugin
//! instances, their permissions and lifecycle, the services of the plugin
//! API, and, on Windows, the hooks and the overlay that connect them to the
//! game.
// The MSVC linker reports the import library it writes for `runtime.dll`
// on every build, in the system's language.
#![allow(linker_messages)]
pub mod author;
pub mod catalog;
pub mod catalog_index;
pub mod chat;
pub mod clipboard;
pub mod commands;
pub mod config;
pub mod dev;
pub mod draw;
pub use angelica_look::{fonts, theme};
pub use angelica_query::error;
pub mod excel_text;
pub mod file_formats;
pub mod file_replacements;

pub mod game;
pub mod game_data;
pub mod gpu;
pub mod host;
pub mod http;
pub mod icons;
pub mod installer;
pub mod intercepts;
pub mod jobs;
pub mod key;
pub mod language;
pub mod lens;
pub mod links;
pub mod look;
pub mod manifest;
pub mod memory;
pub mod menu;
pub mod menus;
pub mod messages;
#[cfg(test)]
use angelica_query::mutations;
pub mod notifications;
pub mod package;
pub mod panel;
pub(crate) mod parts;
pub(crate) mod places;
pub mod plugin_installer;
pub mod sandbox;
pub mod self_update;
pub mod sheets;
pub mod sockets;
pub mod sound;
pub mod storage;
pub mod text_case;

pub mod view;
pub mod wasi;

#[cfg(windows)]
#[allow(unsafe_code)]
pub mod interop;

include!(concat!(env!("OUT_DIR"), "/release_key.rs"));
