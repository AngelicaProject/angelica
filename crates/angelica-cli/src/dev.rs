//! A signed local installation for development: a development key, and the
//! runtime, map, and plugin packages signed with it, in the layout of
//! `docs/formats/host-installation-v1.md`.
//!
//! The proxy and the runtime must be built with the key's public half in
//! `ANGELICA_RELEASE_PUBLIC_KEY`; packages signed with any other key are
//! refused.
use crate::Output;
use clap::{Args, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use runtime::installer::{read_key, write, write_package as package};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Create a development signing key and print its public half.
    Key(KeyArgs),
    /// Sign a runtime, a map, and plugins into an installation folder.
    Install(InstallArgs),
}

#[derive(Debug, Args)]
pub struct KeyArgs {
    /// Where to write the key (64 hexadecimal digits of the secret).
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// The development key from `angelica dev key`.
    #[arg(long)]
    key: PathBuf,
    /// The installation folder `angelica.json` names.
    #[arg(long)]
    installation: PathBuf,
    /// `runtime.dll` built with this key's public half.
    #[arg(long)]
    runtime: Option<PathBuf>,
    /// `plugin-installer.exe` from the same build, placed beside the runtime.
    #[arg(long)]
    installer: Option<PathBuf>,
    /// The game map folder, with `map.toml`.
    #[arg(long)]
    map: Option<PathBuf>,
    /// A plugin as `<folder with angelica.toml>=<component .wasm>`; repeatable.
    #[arg(long = "plugin")]
    plugins: Vec<String>,
    #[command(flatten)]
    reporting: Output,
}

pub fn run(command: &DevCommand) -> ExitCode {
    let (result, reporting) = match command {
        DevCommand::Key(args) => (key(args), &args.reporting),
        DevCommand::Install(args) => (install(args), &args.reporting),
    };
    match result {
        Ok(report) => {
            if reporting.json {
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
            if reporting.json {
                println!(
                    "{}",
                    serde_json::json!({"error": {"code": "dev", "message": error}})
                );
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn key(args: &KeyArgs) -> Result<serde_json::Value, String> {
    if args.output.exists() {
        return Err(format!(
            "{} exists; a key is never overwritten",
            args.output.display()
        ));
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
    std::fs::write(&args.output, runtime::package::hex(&secret)).map_err(|e| e.to_string())?;
    let public = SigningKey::from_bytes(&secret).verifying_key();
    Ok(
        serde_json::json!({"key": args.output, "public_key": runtime::package::hex(public.as_bytes())}),
    )
}

fn install(args: &InstallArgs) -> Result<serde_json::Value, String> {
    let key = read_key(&args.key)?;
    let mut report = serde_json::Map::new();
    report.insert(
        "public_key".into(),
        runtime::package::hex(key.verifying_key().as_bytes()).into(),
    );
    if let Some(path) = &args.runtime {
        // A running game holds its runtime open: each development install
        // is a new version folder, which `current.json` names for the next
        // start. The plugin installer goes beside the runtime.
        let version = format!(
            "dev-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs()
        );
        let folder = args.installation.join("versions").join(&version);
        let mut programs = vec![("runtime.dll", path)];
        if let Some(installer) = &args.installer {
            programs.push(("plugin-installer.exe", installer));
        }
        for (name, path) in programs {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            write(&folder.join(name), &bytes)?;
            write(
                &folder.join(format!("{name}.sig")),
                &key.sign(&bytes).to_bytes(),
            )?;
        }
        let current = args.installation.join("current.json");
        let next = args.installation.join("current.json.new");
        write(
            &next,
            serde_json::json!({"format": 1, "version": version})
                .to_string()
                .as_bytes(),
        )?;
        std::fs::rename(&next, &current).map_err(|e| format!("{}: {e}", current.display()))?;
        report.insert("runtime".into(), folder.display().to_string().into());
    } else if args.installer.is_some() {
        return Err("--installer goes beside a --runtime".into());
    }
    if let Some(map) = &args.map {
        let files = map_files(map)?;
        package(&args.installation.join("map"), &files, &key)?;
        report.insert("map_files".into(), files.len().into());
    }
    let mut plugins = Vec::new();
    for plugin in &args.plugins {
        let (source, component) = plugin.split_once('=').ok_or_else(|| {
            format!("{plugin}: expected <folder with angelica.toml>=<component .wasm>")
        })?;
        let manifest_text = std::fs::read(Path::new(source).join("angelica.toml"))
            .map_err(|e| format!("{source}: {e}"))?;
        let manifest = runtime::manifest::Manifest::parse(
            std::str::from_utf8(&manifest_text).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let wasm = std::fs::read(component).map_err(|e| format!("{component}: {e}"))?;
        let sandbox = runtime::sandbox::Sandbox::new()?;
        let compiled = wasmtime::component::Component::new(&sandbox.engine, &wasm)
            .and_then(|component| component.serialize())
            .map_err(|e| format!("{component}: {e}"))?;
        let files = BTreeMap::from([
            ("angelica.toml".to_owned(), manifest_text),
            ("plugin.wasm".to_owned(), wasm),
            ("plugin.cwasm".to_owned(), compiled),
            // Marks the plugin as in development in the menu.
            ("development".to_owned(), b"angelica dev install\n".to_vec()),
        ]);
        package(
            &args.installation.join("plugins").join(manifest.id()),
            &files,
            &key,
        )?;
        plugins.push(manifest.id().to_owned());
    }
    report.insert("plugins".into(), plugins.into());
    Ok(serde_json::Value::Object(report))
}

/// The files of the map in `map` that a map package holds, by their paths
/// relative to the map.
///
/// # Errors
/// A missing `map.toml`, `entries`, or `builds`, or an unreadable file.
pub fn map_files(map: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    files.insert(
        "map.toml".to_owned(),
        std::fs::read(map.join("map.toml")).map_err(|e| e.to_string())?,
    );
    for folder in ["entries", "builds"] {
        let mut names: Vec<_> = std::fs::read_dir(map.join(folder))
            .map_err(|e| format!("{folder}: {e}"))?
            .filter_map(Result::ok)
            .map(|item| item.file_name().to_string_lossy().into_owned())
            // The map reader takes exactly these names.
            .filter(|name| Path::new(name).extension().is_some_and(|e| e == "toml"))
            .collect();
        names.sort();
        for name in names {
            files.insert(
                format!("{folder}/{name}"),
                std::fs::read(map.join(folder).join(&name)).map_err(|e| e.to_string())?,
            );
        }
    }
    for folder in ["sheets", "types", "enums", "windows"] {
        add_folder(&map.join(folder), folder, &mut files)?;
    }
    for file in ["domains.toml", "roots.toml", "events.toml"] {
        let path = map.join(file);
        if path.exists() {
            files.insert(
                file.to_owned(),
                std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            );
        }
    }
    Ok(files)
}

/// Adds every `.toml` file under `folder`, by its path relative to the map,
/// to `files`.
fn add_folder(
    folder: &Path,
    prefix: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    if !folder.exists() {
        return Ok(());
    }
    let listing = std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for item in listing.filter_map(Result::ok) {
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            add_folder(&path, &format!("{prefix}/{name}"), files)?;
        } else if path.extension().is_some_and(|e| e == "toml") {
            files.insert(
                format!("{prefix}/{name}"),
                std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            );
        }
    }
    Ok(())
}
