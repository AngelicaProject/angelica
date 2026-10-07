use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use angelica_map::{BuildRecord, Map, MapError};
use angelica_pe::{Image, ImageError, Layout};
use clap::Args;
use serde_json::{Map as JsonObject, Value as Json, json};
use sha2::{Digest, Sha256};

use crate::Output;

const EXECUTABLE: &str = "ffxiv_dx11.exe";
const VERSION_FILE: &str = "ffxivgame.ver";

#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// The map directory (the one with `map.toml`).
    #[arg(long)]
    map: PathBuf,
    /// The game's `game` folder, with `ffxiv_dx11.exe` and `ffxivgame.ver`.
    #[arg(long)]
    game: PathBuf,
    /// Write the build record to `builds/<game version>.toml` in the map.
    #[arg(long)]
    write: bool,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, thiserror::Error)]
enum VerifyError {
    #[error(transparent)]
    Map(#[from] MapError),
    #[error("{path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("{path}: {source}")]
    Image { path: PathBuf, source: ImageError },
    #[error("{path}: `{text}` is not a game version")]
    Version { path: PathBuf, text: String },
}

impl VerifyError {
    fn to_json(&self) -> Json {
        let (code, path, entry) = match self {
            Self::Map(error) => (error.code(), error.path(), error.entry()),
            Self::Io { path, .. } => ("io", Some(path.as_path()), None),
            Self::Image { path, .. } => ("image", Some(path.as_path()), None),
            Self::Version { path, .. } => ("game-version", Some(path.as_path()), None),
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

pub fn run(args: &VerifyArgs) -> ExitCode {
    match verify(args) {
        Ok((record, written)) => {
            if args.output.json {
                println!("{}", report_json(&record, written.as_deref()));
            } else {
                print!("{}", report_text(&record, written.as_deref()));
            }
            if record.resolution.absent().next().is_none() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
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

fn verify(args: &VerifyArgs) -> Result<(BuildRecord, Option<PathBuf>), VerifyError> {
    let map = Map::load(&args.map)?;

    let executable_path = args.game.join(EXECUTABLE);
    let executable = read(&executable_path)?;
    let version_path = args.game.join(VERSION_FILE);
    let version_text = String::from_utf8_lossy(&read(&version_path)?)
        .trim()
        .to_owned();
    if version_text.is_empty()
        || !version_text
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    {
        return Err(VerifyError::Version {
            path: version_path,
            text: version_text,
        });
    }

    let image = Image::parse(&executable, Layout::File).map_err(|source| VerifyError::Image {
        path: executable_path,
        source,
    })?;
    let record = BuildRecord {
        game_version: version_text,
        exe_sha256: sha256_hex(&executable),
        entries_sha256: map.entries_sha256().to_owned(),
        resolution: map.resolve(&image, image.image_base()),
    };

    let written = if args.write {
        let path = args
            .map
            .join("builds")
            .join(format!("{}.toml", record.game_version));
        write_atomically(&path, record.to_toml().as_bytes())?;
        Some(path)
    } else {
        None
    };
    Ok((record, written))
}

fn report_json(record: &BuildRecord, written: Option<&Path>) -> Json {
    let present = record
        .resolution
        .present()
        .map(|(name, value)| (name.to_owned(), json!(value.get())))
        .collect::<JsonObject<_, _>>();
    let absent = record
        .resolution
        .absent()
        .map(|(name, absence)| (name.to_owned(), json!(absence.to_string())))
        .collect::<JsonObject<_, _>>();
    json!({
        "game_version": record.game_version,
        "exe_sha256": record.exe_sha256,
        "entries_sha256": record.entries_sha256,
        "present": present,
        "absent": absent,
        "constant_offsets": record.resolution.constant_offsets(),
        "written": written.map(|path| path.display().to_string()),
    })
}

fn report_text(record: &BuildRecord, written: Option<&Path>) -> String {
    let present = record.resolution.present().count();
    let absent = record.resolution.absent().collect::<Vec<_>>();
    let mut text = format!(
        "game {}: {present} present, {} absent\n",
        record.game_version,
        absent.len()
    );
    for (name, absence) in absent {
        let _ = writeln!(text, "  absent {name}: {absence}");
    }
    for name in record.resolution.constant_offsets() {
        let _ = writeln!(text, "  constant offset {name}");
    }
    if let Some(path) = written {
        let _ = writeln!(text, "wrote {}", path.display());
    }
    text
}

fn read(path: &Path) -> Result<Vec<u8>, VerifyError> {
    std::fs::read(path).map_err(|error| io_error(path, &error))
}

/// Writes a sibling temporary file and renames it over the destination.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), VerifyError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(|error| io_error(&temporary, &error))?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        io_error(path, &error)
    })
}

fn io_error(path: &Path, error: &std::io::Error) -> VerifyError {
    VerifyError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
