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
use sha2::{Digest, Sha256};
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

fn read_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "{}: a key is 64 hexadecimal digits",
            path.display()
        ));
    }
    let mut secret = [0u8; 32];
    for (index, byte) in secret.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(SigningKey::from_bytes(&secret))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let temporary = path.with_extension("tmp-angelica");
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes `files` and a signed `files.json` into `folder`, replacing it.
fn package(
    folder: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    key: &SigningKey,
) -> Result<(), String> {
    if folder.exists() {
        std::fs::remove_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let mut hashes = serde_json::Map::new();
    for (name, bytes) in files {
        if !runtime::package::valid_relative_path(name) {
            return Err(format!("{name} is not a valid package path"));
        }
        hashes.insert(
            name.clone(),
            runtime::package::hex(&Sha256::digest(bytes)).into(),
        );
        write(&folder.join(name), bytes)?;
    }
    let list = serde_json::to_vec(&serde_json::json!({"format": 1, "files": hashes}))
        .map_err(|e| e.to_string())?;
    write(&folder.join("files.json.sig"), &key.sign(&list).to_bytes())?;
    write(&folder.join("files.json"), &list)
}

fn install(args: &InstallArgs) -> Result<serde_json::Value, String> {
    let key = read_key(&args.key)?;
    let mut report = serde_json::Map::new();
    report.insert(
        "public_key".into(),
        runtime::package::hex(key.verifying_key().as_bytes()).into(),
    );
    if let Some(path) = &args.runtime {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // A running game holds the installed runtime open; the proxy loads a
        // staged one at the next start.
        let folder = args.installation.join("staged");
        write(&folder.join("runtime.dll"), &bytes)?;
        write(
            &folder.join("runtime.dll.sig"),
            &key.sign(&bytes).to_bytes(),
        )?;
        report.insert("runtime".into(), folder.display().to_string().into());
    }
    if let Some(map) = &args.map {
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
        for folder in ["sheets", "state"] {
            add_folder(&map.join(folder), folder, &mut files)?;
        }
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
