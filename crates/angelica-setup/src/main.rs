//! `angelica-setup.exe` (`docs/architecture/installer.md`): installs
//! Angelica beside the game, and removes it with `--uninstall`. With
//! `--place <game> <installation>` or `--remove <game>` it is the copy with
//! administrator rights that only changes the game folder.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod install;
mod neighbors;
mod payload;
#[allow(unsafe_code)]
mod system;
mod words;

use std::path::PathBuf;

/// Exit codes of the copy with administrator rights.
pub mod exit {
    pub const OK: i32 = 0;
    pub const FAILED: i32 = 1;
    pub const OTHER_TOOL: i32 = 2;
    pub const IN_USE: i32 = 3;
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--place") if args.len() == 3 => {
            std::process::exit(place(&PathBuf::from(&args[1]), &PathBuf::from(&args[2])));
        }
        Some("--remove") if args.len() == 2 => {
            std::process::exit(code(&install::remove_from_game(&PathBuf::from(&args[1]))));
        }
        Some("--uninstall") => app::run(app::Mode::Uninstall),
        _ => app::run(app::Mode::Install),
    }
}

fn place(game: &std::path::Path, installation: &std::path::Path) -> i32 {
    let (Some(payload), Some(key)) = (payload::PAYLOAD, payload::release_key()) else {
        return exit::FAILED;
    };
    if payload.verify(&key).is_err() {
        return exit::FAILED;
    }
    code(&install::place(game, installation, payload.dinput8_dll))
}

fn code(result: &Result<(), install::PlaceError>) -> i32 {
    match result {
        Ok(()) => exit::OK,
        Err(install::PlaceError::OtherTool) => exit::OTHER_TOOL,
        Err(install::PlaceError::InUse) => exit::IN_USE,
        Err(install::PlaceError::Other(_)) => exit::FAILED,
    }
}
