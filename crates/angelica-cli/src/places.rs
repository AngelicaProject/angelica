//! `angelica where`: the game folders on this computer and the Angelica
//! installation each one loads, for the other commands' `--game` and
//! `--installation`.
use crate::Output;
use clap::Args;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug, Args)]
pub struct WhereArgs {
    /// The game's folder, or its `game` folder, when it is not where a
    /// launcher put it.
    #[arg(long)]
    game: Option<PathBuf>,
    #[command(flatten)]
    output: Output,
}

pub fn run(args: &WhereArgs) -> ExitCode {
    let result = match &args.game {
        Some(folder) => angelica_games::resolve(folder)
            .map(|game| vec![game])
            .ok_or_else(|| format!("{}: no game in this folder", folder.display())),
        None => Some(angelica_games::find())
            .filter(|games| !games.is_empty())
            .ok_or_else(|| "no game found where launchers put it; pass --game <folder>".to_owned()),
    };
    match result {
        Ok(games) => {
            let report = serde_json::json!({
                "games": games.iter().map(|game| describe(game)).collect::<Vec<_>>()
            });
            if args.output.json {
                println!("{report}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).unwrap_or_default()
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if args.output.json {
                println!(
                    "{}",
                    serde_json::json!({"error": {"code": "where", "message": error}})
                );
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

/// A game folder, and what its `angelica.json` names
/// (`docs/formats/host-installation-v1.md`).
fn describe(game: &Path) -> serde_json::Value {
    let installation = std::fs::read(game.join("angelica.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|config| config.get("installation_dir")?.as_str().map(PathBuf::from));
    let Some(installation) = installation else {
        return serde_json::json!({
            "game": game,
            "installation": null,
            "note": "Angelica is not installed for this game",
        });
    };
    let json = |path: &Path| {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    };
    let version = json(&installation.join("current.json"))
        .and_then(|current| current.get("version")?.as_str().map(str::to_owned));
    let host = json(&installation.join("host.json"));
    let folder = |field: &str, default: &str| {
        host.as_ref()
            .and_then(|host| host.get(field)?.as_str().map(PathBuf::from))
            .unwrap_or_else(|| installation.join(default))
    };
    let map = folder("map_directory", "map");
    let map_format = std::fs::read_to_string(map.join("map.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("format")?.as_integer());
    let plugins = folder("plugin_directory", "plugins");
    let mut installed: Vec<String> = std::fs::read_dir(&plugins)
        .map(|listing| {
            listing
                .filter_map(Result::ok)
                .filter(|item| item.path().is_dir())
                .map(|item| item.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    installed.sort();
    let dev_mode = json(&installation.join("dev/mode.json"))
        .is_some_and(|mode| mode == serde_json::json!({"format": 1, "on": true}));
    serde_json::json!({
        "game": game,
        "installation": installation,
        "angelica": version,
        "map": {"folder": map, "format": map_format},
        "plugins": {"folder": plugins, "installed": installed},
        "dev_mode": dev_mode,
        "local_map": installation.join("dev/map/files.json").is_file(),
    })
}
