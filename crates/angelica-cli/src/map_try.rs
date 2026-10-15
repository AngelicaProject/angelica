//! `angelica map try`: a map checked as `map check` does, put into an
//! installation as dev mode's local map, which Angelica uses instead of the
//! signed map while dev mode is on.
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use serde_json::{Value as Json, json};

use crate::Output;

#[derive(Debug, Args)]
pub struct TryArgs {
    /// The map directory (the one with `map.toml`).
    #[arg(long, required_unless_present = "clear")]
    map: Option<PathBuf>,
    /// The installation folder (`%LOCALAPPDATA%\Angelica` for an installed
    /// Angelica).
    #[arg(long)]
    installation: PathBuf,
    /// Remove the local map: Angelica goes back to the signed map.
    #[arg(long, conflicts_with = "map")]
    clear: bool,
    #[command(flatten)]
    output: Output,
}

pub fn run(args: &TryArgs) -> ExitCode {
    match try_map(args) {
        Ok(report) => {
            if args.output.json {
                println!("{report}");
            } else {
                print!("{}", text(&report));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if args.output.json {
                println!("{error}");
            } else {
                eprintln!(
                    "error: {}",
                    error["error"]["message"].as_str().unwrap_or_default()
                );
            }
            ExitCode::FAILURE
        }
    }
}

fn failure(code: &str, message: &str) -> Json {
    json!({"error": {"code": code, "message": message, "path": null, "entry": null}})
}

fn try_map(args: &TryArgs) -> Result<Json, Json> {
    let dev = args.installation.join(runtime::dev::FOLDER);
    let local = dev.join(runtime::dev::MAP);
    let dev_mode = runtime::dev::read(&dev);
    let Some(map) = &args.map else {
        if local.exists() {
            std::fs::remove_dir_all(&local)
                .map_err(|e| failure("io", &format!("{}: {e}", local.display())))?;
        }
        return Ok(json!({"local_map": null, "dev_mode": dev_mode}));
    };
    let checked = crate::check::check(map).map_err(|error| error.to_json())?;
    let files = crate::dev::map_files(map).map_err(|message| failure("io", &message))?;
    runtime::installer::write_unsigned(&local, &files)
        .map_err(|message| failure("io", &message))?;
    Ok(json!({
        "local_map": local.display().to_string(),
        "entries": checked.entries,
        "builds": checked.builds.iter().map(|record| record.game_version.clone()).collect::<Vec<_>>(),
        "dev_mode": dev_mode,
    }))
}

fn text(report: &Json) -> String {
    let mut text = match report["local_map"].as_str() {
        Some(path) => format!("local map with {} entries in {path}\n", report["entries"]),
        None => "local map removed; Angelica uses the signed map\n".to_owned(),
    };
    if report["dev_mode"] == false && !report["local_map"].is_null() {
        text.push_str(
            "dev mode is off, so Angelica keeps the signed map: angelica dev mode on --installation <folder>\n",
        );
    }
    text
}
