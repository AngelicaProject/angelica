use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use angelica_map::{BuildRecord, Map, MapError};
use clap::Args;
use serde_json::{Value as Json, json};

use crate::Output;

const NOTICES: &str = "THIRD_PARTY_NOTICES.md";

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// The map directory (the one with `map.toml`).
    #[arg(long)]
    map: PathBuf,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CheckError {
    #[error(transparent)]
    Map(#[from] MapError),
    #[error("{path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("{path}: {message}")]
    Record { path: PathBuf, message: String },
    #[error("{path}: an entry has an `origin`, and the map has no {NOTICES}")]
    Notices { path: PathBuf },
    #[error("{} have no `whose`: say whether each reaches the player's objects (`you`) or others' (`others`)", .missing.join(", "))]
    Whose { missing: Vec<String> },
}

impl CheckError {
    pub(crate) fn to_json(&self) -> Json {
        let (code, path, entry) = match self {
            Self::Map(error) => (error.code(), error.path(), error.entry()),
            Self::Io { path, .. } => ("io", Some(path.as_path()), None),
            Self::Record { path, .. } => ("build-record", Some(path.as_path()), None),
            Self::Notices { path } => ("notices", Some(path.as_path()), None),
            Self::Whose { .. } => ("whose", None, None),
        };
        json!({
            "error": {
                "code": code,
                "message": self.to_string(),
                "path": path.map(|path| path.display().to_string()),
                "entry": entry,
            }
        })
    }
}

/// What `check` found: the number of entries and each build record, in
/// the order of their game versions.
pub(crate) struct Checked {
    pub(crate) entries: usize,
    pub(crate) builds: Vec<BuildRecord>,
}

pub fn run(args: &CheckArgs) -> ExitCode {
    match check(&args.map) {
        Ok(checked) => {
            if args.output.json {
                println!("{}", report_json(&checked));
            } else {
                print!("{}", report_text(&checked));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if args.output.json {
                println!("{}", error.to_json());
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn check(directory: &Path) -> Result<Checked, CheckError> {
    let map = Map::load(directory)?;
    let missing = map.objects().without_whose();
    if !missing.is_empty() {
        return Err(CheckError::Whose { missing });
    }
    let notices = directory.join(NOTICES);
    if map.entries().any(|entry| entry.origin.is_some()) && !notices.is_file() {
        return Err(CheckError::Notices { path: notices });
    }
    let mut builds = Vec::new();
    for path in record_paths(&directory.join("builds"))? {
        let text = std::fs::read_to_string(&path).map_err(|error| io_error(&path, &error))?;
        let record_error = |message| CheckError::Record {
            path: path.clone(),
            message,
        };
        let record = BuildRecord::check(&text, &map).map_err(record_error)?;
        let name = format!("{}.toml", record.game_version);
        if path.file_name().and_then(|file| file.to_str()) != Some(name.as_str()) {
            return Err(record_error(format!(
                "the record of game version {} is named {name}",
                record.game_version
            )));
        }
        builds.push(record);
    }
    Ok(Checked {
        entries: map.entries().count(),
        builds,
    })
}

/// Every file of `builds/`, sorted; none when the map has no `builds/`.
/// Anything there but a `.toml` file is refused, so no record is skipped.
fn record_paths(builds: &Path) -> Result<Vec<PathBuf>, CheckError> {
    let listing = match std::fs::read_dir(builds) {
        Ok(listing) => listing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(builds, &error)),
    };
    let mut paths = Vec::new();
    for item in listing {
        let item = item.map_err(|error| io_error(builds, &error))?;
        let path = item.path();
        let kind = item.file_type().map_err(|error| io_error(&path, &error))?;
        if !kind.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("toml")
        {
            return Err(CheckError::Record {
                path,
                message: "builds/ holds only build records, `<game version>.toml`".into(),
            });
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

fn report_json(checked: &Checked) -> Json {
    let builds = checked
        .builds
        .iter()
        .map(|record| {
            json!({
                "game_version": record.game_version,
                "exe_sha256": record.exe_sha256,
                "present": record.resolution.present().count(),
                "absent": record.resolution.absent().count(),
                "constant_offsets": record.resolution.constant_offsets(),
            })
        })
        .collect::<Vec<_>>();
    json!({"entries": checked.entries, "builds": builds})
}

fn report_text(checked: &Checked) -> String {
    let mut text = format!(
        "map: {} entries, {} build records\n",
        checked.entries,
        checked.builds.len()
    );
    for record in &checked.builds {
        let _ = writeln!(
            text,
            "  game {}: {} present, {} absent, {} confirmed constant offsets",
            record.game_version,
            record.resolution.present().count(),
            record.resolution.absent().count(),
            record.resolution.constant_offsets().len()
        );
    }
    text
}

fn io_error(path: &Path, error: &std::io::Error) -> CheckError {
    CheckError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    }
}
