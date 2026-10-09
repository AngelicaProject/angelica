//! Plugins from their authors, outside the official catalog: the author's
//! key, signed plugin files (`docs/formats/plugin-file-v1.md`), and their
//! installation, which compiles the component on the player's computer and
//! signs the package with the installation's key.
use crate::{
    Output,
    dev::{package, read_key, write},
};
use clap::Args;
use ed25519_dalek::SigningKey;
use runtime::{
    author::{self, Pin},
    manifest::Manifest,
    package::hex,
};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug, Args)]
pub struct KeyArgs {
    /// The author key; by default `%APPDATA%/Angelica/author.key`.
    #[arg(long)]
    key: Option<PathBuf>,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct PackArgs {
    /// The plugin's folder, with `angelica.toml`.
    #[arg(default_value = ".")]
    folder: PathBuf,
    /// The component built for `wasm32-wasip2`.
    #[arg(long)]
    component: PathBuf,
    /// The author key, created when missing; by default
    /// `%APPDATA%/Angelica/author.key`.
    #[arg(long)]
    key: Option<PathBuf>,
    /// The plugin file to write; by default `<id>-<version>.angelica` in the
    /// current folder.
    #[arg(long)]
    output: Option<PathBuf>,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// The plugin file (`.angelica`).
    file: PathBuf,
    /// The installation folder `angelica.json` names.
    #[arg(long)]
    installation: PathBuf,
    #[command(flatten)]
    reporting: Output,
}

pub fn key(args: &KeyArgs) -> ExitCode {
    report(
        &args.reporting,
        "author-key",
        (|| {
            let path = key_path(args.key.as_ref())?;
            let (key, created) = author_key(&path)?;
            Ok(serde_json::json!({
                "key": path,
                "created": created,
                "public_key": hex(key.verifying_key().as_bytes()),
            }))
        })(),
    )
}

pub fn pack(args: &PackArgs) -> ExitCode {
    report(&args.reporting, "plugin-pack", pack_file(args))
}

pub fn install(args: &InstallArgs) -> ExitCode {
    report(&args.reporting, "plugin-install", install_file(args))
}

fn report(reporting: &Output, code: &str, result: Result<serde_json::Value, String>) -> ExitCode {
    match result {
        Ok(value) => {
            if reporting.json {
                println!("{value}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if reporting.json {
                println!(
                    "{}",
                    serde_json::json!({"error": {"code": code, "message": error}})
                );
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn key_path(key: Option<&PathBuf>) -> Result<PathBuf, String> {
    if let Some(key) = key {
        return Ok(key.clone());
    }
    std::env::var_os("APPDATA")
        .map(|folder| PathBuf::from(folder).join("Angelica").join("author.key"))
        .ok_or_else(|| "APPDATA is not set; name the author key with --key".into())
}

/// The key at `path`, created when there is none; whether it was created.
fn author_key(path: &Path) -> Result<(SigningKey, bool), String> {
    if path.exists() {
        return Ok((read_key(path)?, false));
    }
    Ok((new_key(path)?, true))
}

/// Creates a key at `path`, which must not exist.
fn new_key(path: &Path) -> Result<SigningKey, String> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // A key is never overwritten.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, hex(&secret).as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(SigningKey::from_bytes(&secret))
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} is larger than {limit} bytes", path.display()));
    }
    Ok(bytes)
}

/// Compiles a component for this runtime, which also checks it.
fn compile(component: &[u8]) -> Result<Vec<u8>, String> {
    let sandbox = runtime::sandbox::Sandbox::new()?;
    wasmtime::component::Component::new(&sandbox.engine, component)
        .and_then(|component| component.serialize())
        .map_err(|e| format!("the component does not compile: {e}"))
}

fn pack_file(args: &PackArgs) -> Result<serde_json::Value, String> {
    let manifest_bytes = read_limited(&args.folder.join("angelica.toml"), author::MAX_MANIFEST)?;
    let manifest = Manifest::parse(
        std::str::from_utf8(&manifest_bytes).map_err(|e| format!("angelica.toml: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let component = read_limited(&args.component, author::MAX_COMPONENT)?;
    compile(&component)?;
    let key_path = key_path(args.key.as_ref())?;
    let (key, created) = author_key(&key_path)?;
    let file = author::sign(&key, &manifest_bytes, &component)?;
    let output = args.output.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}-{}.angelica",
            manifest.id(),
            manifest.plugin.version
        ))
    });
    write(&output, &file)?;
    Ok(serde_json::json!({
        "plugin": manifest.id(),
        "version": manifest.plugin.version,
        "file": output,
        "key": key_path,
        "key_created": created,
    }))
}

fn install_file(args: &InstallArgs) -> Result<serde_json::Value, String> {
    let bytes = read_limited(&args.file, author::MAX_FILE)?;
    let file = author::open(&bytes)?;
    let manifest = Manifest::parse(
        std::str::from_utf8(&file.manifest).map_err(|e| format!("angelica.toml: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let id = manifest.id();
    let config = runtime::config::Config::load(&args.installation)?;
    let keys = config
        .keys_directory
        .clone()
        .unwrap_or_else(|| args.installation.join("keys"));
    let folder = config.plugin_directory.join(id);
    let pin = Pin::read(&keys, id)?;
    match &pin {
        Some(pin) if !pin.names(&file.author) => {
            return Err(format!(
                "this file of {id} is signed by another author than the installed {id}; not installed"
            ));
        }
        None if folder.exists() => {
            return Err(format!(
                "{id} is already installed from the Angelica catalog; not installed"
            ));
        }
        _ => {}
    }
    let compiled = compile(&file.component)?;
    let installation = installation_key(&keys)?;
    if pin.is_none() {
        let pin = Pin::new(id, &file.author);
        write(
            &keys.join("authors").join(format!("{id}.json")),
            &serde_json::to_vec_pretty(&pin).map_err(|e| e.to_string())?,
        )?;
    }
    let files = BTreeMap::from([
        ("plugin.angelica".to_owned(), bytes),
        ("plugin.cwasm".to_owned(), compiled),
    ]);
    package(&folder, &files, &installation)?;
    Ok(serde_json::json!({
        "plugin": id,
        "version": manifest.plugin.version,
        "folder": folder,
        "new_author": pin.is_none(),
    }))
}

/// The installation's key, created with its public half when missing.
fn installation_key(keys: &Path) -> Result<SigningKey, String> {
    let path = keys.join("installation.key");
    if path.exists() {
        return read_key(&path);
    }
    let key = new_key(&path)?;
    write(
        &keys.join("installation.pub"),
        hex(key.verifying_key().as_bytes()).as_bytes(),
    )?;
    Ok(key)
}
