//! `angelica anatomia`: convert a game map v2 to Anatomia 1 and the Lens
//! catalog, and resolve a description against the game's executable.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use angelica_anatomia::{Anatomia, BuildRecord, Catalog, Kind};
use angelica_pe::{Image, Layout};
use clap::{Args, Subcommand};
use serde_json::{Map as JsonObject, Value as Json, json};

use crate::Output;
use crate::verify::sha256_hex;

const EXECUTABLE: &str = "ffxiv_dx11.exe";
const VERSION_FILE: &str = "ffxivgame.ver";

#[derive(Debug, Subcommand)]
pub enum AnatomiaCommand {
    /// Convert a game map of format v2 to Anatomia 1 and the Lens catalog.
    Convert(ConvertArgs),
    /// Resolve every entry of a description against the game's executable,
    /// and check the Lens catalog and the schemas against the build.
    Verify(VerifyArgs),
}

#[derive(Debug, Args)]
pub struct ConvertArgs {
    /// The map directory (the one with `map.toml`).
    #[arg(long)]
    map: PathBuf,
    /// The directory to write the description to; empty or new.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// The description directory (the one with `anatomia.toml`).
    #[arg(long)]
    anatomia: PathBuf,
    /// The game's `game` folder, with `ffxiv_dx11.exe` and `ffxivgame.ver`.
    #[arg(long)]
    game: PathBuf,
    /// Write the build record to `builds/<game version>.toml`.
    #[arg(long)]
    write: bool,
    /// A field with a constant offset that was checked in the game for this
    /// build; repeatable. The build's existing record keeps confirming the
    /// fields it confirmed, while their offsets stay the same.
    #[arg(long, value_name = "FIELD")]
    confirm: Vec<String>,
    #[command(flatten)]
    output: Output,
}

pub fn run(command: &AnatomiaCommand) -> ExitCode {
    match command {
        AnatomiaCommand::Convert(args) => match convert(args) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        AnatomiaCommand::Verify(args) => match verify(args) {
            Ok(verified) => {
                if args.output.json {
                    println!("{}", report_json(&verified));
                } else {
                    print!("{}", report_text(&verified));
                }
                if verified.complete() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                if args.output.json {
                    println!("{}", json!({"error": {"message": error}}));
                } else {
                    eprintln!("error: {error}");
                }
                ExitCode::FAILURE
            }
        },
    }
}

fn convert(args: &ConvertArgs) -> Result<String, String> {
    if args.out.exists()
        && std::fs::read_dir(&args.out)
            .map_err(|error| format!("{}: {error}", args.out.display()))?
            .next()
            .is_some()
    {
        return Err(format!("{} is not empty", args.out.display()));
    }
    let converted = angelica_anatomia::convert_map(&args.map)?;
    for (path, text) in &converted.files {
        let target = args.out.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&target, text).map_err(|error| format!("{}: {error}", target.display()))?;
    }
    let mut text = format!(
        "wrote {} files to {}\n",
        converted.files.len(),
        args.out.display()
    );
    for (old, new) in &converted.renamed {
        let _ = writeln!(text, "  {old} is now {new}, with the old name as an alias");
    }
    Ok(text)
}

type Report = BTreeMap<String, Result<(), String>>;

struct Verified {
    record: BuildRecord,
    broken: BTreeMap<String, String>,
    catalog: Report,
    sheets: Report,
    windows: Report,
    written: Option<PathBuf>,
}

impl Verified {
    fn complete(&self) -> bool {
        self.record.resolution.absent().next().is_none()
            && self.broken.is_empty()
            && [&self.catalog, &self.sheets, &self.windows]
                .iter()
                .all(|report| report.values().all(Result::is_ok))
    }
}

fn verify(args: &VerifyArgs) -> Result<Verified, String> {
    let anatomia = Anatomia::load(&args.anatomia).map_err(|error| error.to_string())?;
    let catalog = Catalog::load(&args.anatomia).map_err(|error| error.to_string())?;

    let read =
        |path: &Path| std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()));
    let executable_path = args.game.join(EXECUTABLE);
    let executable = read(&executable_path)?;
    let version_path = args.game.join(VERSION_FILE);
    let version = String::from_utf8_lossy(&read(&version_path)?)
        .trim()
        .to_owned();
    if version.is_empty()
        || !version
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    {
        return Err(format!(
            "{}: `{version}` is not a game version",
            version_path.display()
        ));
    }
    let image = Image::parse(&executable, Layout::File)
        .map_err(|error| format!("{}: {error}", executable_path.display()))?;
    let exe_sha256 = sha256_hex(&executable);
    let record_path = args.anatomia.join("builds").join(format!("{version}.toml"));
    let confirmed = confirmed_offsets(&anatomia, &record_path, &exe_sha256, &args.confirm)?;
    let record = BuildRecord {
        game_version: version,
        exe_sha256,
        anatomia_sha256: anatomia.sha256().to_owned(),
        resolution: anatomia.resolve(&image, image.image_base(), &confirmed),
    };

    let catalog_report = catalog
        .check(&anatomia, &record.resolution)
        .into_iter()
        .map(|((kind, name), result)| (format!("{kind} {name}"), result))
        .collect();

    let mut sheets = Report::new();
    let mut windows = Report::new();
    if !anatomia.sheets.is_empty() || !anatomia.windows.is_empty() {
        let data = runtime::game_data::GameData::new(args.game.parent());
        for (path, text) in &anatomia.windows {
            let result = angelica_map::parse_window_schema(path, text).and_then(|schema| {
                let layout = data.read(&schema.layout).map_err(|error| error.message)?;
                let fingerprint = angelica_map::window_layout_fingerprint(&layout);
                schema.resolve(&fingerprint).map(drop)
            });
            windows.insert(path.clone(), result);
        }
        for (path, text) in &anatomia.sheets {
            let result = angelica_map::parse_sheet_schema(path, text).and_then(|schema| {
                let info = data.sheet(&schema.sheet).map_err(|error| error.message)?;
                schema.resolve(&info.layout()).map(drop)
            });
            sheets.insert(path.clone(), result);
        }
    }

    let written = if args.write {
        if let Some(parent) = record_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&record_path, record.to_toml())
            .map_err(|error| format!("{}: {error}", record_path.display()))?;
        Some(record_path)
    } else {
        None
    };
    Ok(Verified {
        record,
        broken: anatomia.broken().clone(),
        catalog: catalog_report,
        sheets,
        windows,
        written,
    })
}

/// The constant offsets confirmed for the executable: the ones `confirm`
/// names, and the ones the build's record confirms with the offset the
/// description still gives.
fn confirmed_offsets(
    anatomia: &Anatomia,
    record_path: &Path,
    executable: &str,
    confirm: &[String],
) -> Result<BTreeSet<String>, String> {
    let constant = |name: &str| match anatomia.get(name).map(|entry| &entry.kind) {
        Some(Kind::Field { at, .. }) => *at,
        _ => None,
    };
    let mut confirmed = BTreeSet::new();
    for name in confirm {
        if constant(name).is_none() {
            return Err(format!("`{name}` is not a field with a constant offset"));
        }
        confirmed.insert(anatomia.canonical(name).to_owned());
    }
    let text = match std::fs::read_to_string(record_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(confirmed),
        Err(error) => return Err(format!("{}: {error}", record_path.display())),
    };
    let recorded = BuildRecord::confirmed_offsets(&text, executable)
        .map_err(|message| format!("{}: {message}", record_path.display()))?;
    confirmed.extend(
        recorded
            .into_iter()
            .filter(|(name, offset)| constant(name) == Some(*offset))
            .map(|(name, _)| name),
    );
    Ok(confirmed)
}

fn report_json(verified: &Verified) -> Json {
    let record = &verified.record;
    let report = |report: &Report| {
        report
            .iter()
            .map(|(name, result)| {
                (
                    name.clone(),
                    json!({"available": result.is_ok(), "reason": result.as_ref().err()}),
                )
            })
            .collect::<JsonObject<_, _>>()
    };
    json!({
        "game_version": record.game_version,
        "exe_sha256": record.exe_sha256,
        "anatomia_sha256": record.anatomia_sha256,
        "present": record.resolution.present().map(|(name, value)| (name.to_owned(), json!(value.get()))).collect::<JsonObject<_, _>>(),
        "absent": record.resolution.absent().map(|(name, absence)| (name.to_owned(), json!(absence.0))).collect::<JsonObject<_, _>>(),
        "confirmed": record.resolution.confirmed(),
        "unusable": verified.broken,
        "catalog": report(&verified.catalog),
        "sheets": report(&verified.sheets),
        "windows": report(&verified.windows),
        "written": verified.written.as_ref().map(|path| path.display().to_string()),
    })
}

fn report_text(verified: &Verified) -> String {
    let record = &verified.record;
    let absent: Vec<_> = record.resolution.absent().collect();
    let mut text = format!(
        "game {}: {} present, {} absent\n",
        record.game_version,
        record.resolution.present().count(),
        absent.len()
    );
    for (name, absence) in absent {
        let _ = writeln!(text, "  absent {name}: {absence}");
    }
    for name in record.resolution.confirmed() {
        let _ = writeln!(text, "  confirmed constant offset {name}");
    }
    for (name, reason) in &verified.broken {
        if record.resolution.get(name).is_none() {
            let _ = writeln!(text, "  unusable {name}: {reason}");
        }
    }
    for (kind, report) in [
        ("catalog", &verified.catalog),
        ("sheets", &verified.sheets),
        ("windows", &verified.windows),
    ] {
        if report.is_empty() {
            continue;
        }
        let missing = report.values().filter(|result| result.is_err()).count();
        let _ = writeln!(
            text,
            "{kind}: {} usable, {missing} not",
            report.len() - missing
        );
        for (name, result) in report {
            if let Err(reason) = result {
                let _ = writeln!(text, "  {name}: {reason}");
            }
        }
    }
    if let Some(path) = &verified.written {
        let _ = writeln!(text, "wrote {}", path.display());
    }
    text
}
