//! `angelica release`: what the release workflows do with the release key
//! (`docs/development/releases.md`). The secret comes from the environment
//! variable `ANGELICA_RELEASE_SIGNING_KEY`, never from a file in the
//! repository, and must match `ANGELICA_RELEASE_PUBLIC_KEY`, the key the
//! release's programs are built with.
use clap::{Args, Subcommand};
use ed25519_dalek::SigningKey;
use std::{collections::BTreeMap, path::PathBuf, process::ExitCode};

use crate::Output;
use angelica_update::{Channel, Feed, File, Product};

pub const SECRET: &str = "ANGELICA_RELEASE_SIGNING_KEY";
pub const PUBLIC: &str = "ANGELICA_RELEASE_PUBLIC_KEY";

#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    /// Create a release key and print its public half. Keep the file offline.
    Key(KeyArgs),
    /// Write `<file>.sig`, the release signature of each file.
    Sign(SignArgs),
    /// Write a signed update feed for files of a release.
    Feed(FeedArgs),
    /// Sign a game map into one release file, `map.pack`.
    Map(MapArgs),
}

#[derive(Debug, Args)]
pub struct MapArgs {
    /// The game map folder, with `map.toml`.
    #[arg(long)]
    map: PathBuf,
    /// Where to write the pack.
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    reporting: Output,
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
pub struct SignArgs {
    /// The files to sign.
    #[arg(required = true)]
    files: Vec<PathBuf>,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct FeedArgs {
    /// `angelica` or `map`.
    #[arg(long)]
    product: String,
    /// `stable` or `nightly`.
    #[arg(long)]
    channel: String,
    /// The release's version.
    #[arg(long)]
    version: String,
    /// When the release was built, RFC 3339 in UTC.
    #[arg(long)]
    published: String,
    /// A file the release installs, from the folder of the assets; repeatable.
    #[arg(long = "file", required = true)]
    files: Vec<PathBuf>,
    /// Where to write the feed; its signature goes beside it as `.sig`.
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    reporting: Output,
}

pub fn run(command: &ReleaseCommand) -> ExitCode {
    let (result, reporting) = match command {
        ReleaseCommand::Key(args) => (key(args), &args.reporting),
        ReleaseCommand::Sign(args) => (sign(args), &args.reporting),
        ReleaseCommand::Feed(args) => (feed(args), &args.reporting),
        ReleaseCommand::Map(args) => (map(args), &args.reporting),
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
                    serde_json::json!({"error": {"code": "release", "message": error}})
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
    Ok(serde_json::json!({
        "key": args.output,
        "public_key": runtime::package::hex(public.as_bytes()),
    }))
}

/// The release key from the environment, checked against the public key the
/// release is built with.
fn release_key() -> Result<SigningKey, String> {
    let secret = std::env::var(SECRET).map_err(|_| format!("{SECRET} is not set"))?;
    let key = SigningKey::from_bytes(
        &hex32(secret.trim()).ok_or(format!("{SECRET} is not 64 hexadecimal digits"))?,
    );
    let public = std::env::var(PUBLIC).map_err(|_| format!("{PUBLIC} is not set"))?;
    if runtime::package::hex(key.verifying_key().as_bytes()) != public.trim().to_ascii_lowercase() {
        return Err(format!("{SECRET} is not the secret half of {PUBLIC}"));
    }
    Ok(key)
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn sign(args: &SignArgs) -> Result<serde_json::Value, String> {
    let key = release_key()?;
    let mut signed = Vec::new();
    for path in &args.files {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let signature = ed25519_dalek::Signer::sign(&key, &bytes).to_bytes();
        let mut target = path.clone().into_os_string();
        target.push(".sig");
        std::fs::write(&target, signature).map_err(|e| format!("{}: {e}", path.display()))?;
        signed.push(PathBuf::from(target));
    }
    Ok(serde_json::json!({ "signed": signed }))
}

fn feed(args: &FeedArgs) -> Result<serde_json::Value, String> {
    let key = release_key()?;
    let product = match args.product.as_str() {
        "angelica" => Product::Angelica,
        "map" => Product::Map,
        other => return Err(format!("unknown product {other}")),
    };
    let channel = match args.channel.as_str() {
        "stable" => Channel::Stable,
        "nightly" => Channel::Nightly,
        other => return Err(format!("unknown channel {other}")),
    };
    let version: semver::Version = args.version.parse().map_err(|e| format!("version: {e}"))?;
    let mut files = BTreeMap::new();
    for path in &args.files {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| angelica_update::plain_name(name))
            .ok_or_else(|| format!("{}: not a plain file name", path.display()))?;
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        files.insert(name.to_owned(), File::of(name, &bytes));
    }
    let feed = Feed {
        format: 1,
        product,
        channel,
        version,
        published: args.published.clone(),
        files,
    };
    let (json, signature) = feed.sign(&key);
    // The feed must read back as a reader reads it.
    Feed::verify(
        &json,
        &signature,
        key.verifying_key().as_bytes(),
        product,
        channel,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(&args.output, &json).map_err(|e| format!("{}: {e}", args.output.display()))?;
    let mut sig = args.output.clone().into_os_string();
    sig.push(".sig");
    std::fs::write(&sig, signature).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "feed": args.output, "version": feed.version.to_string() }))
}

/// The map's files with their signed file list, packed: the host unpacks it
/// into its map folder and loads it as any map package.
fn map(args: &MapArgs) -> Result<serde_json::Value, String> {
    let key = release_key()?;
    let files = crate::dev::map_files(&args.map)?;
    let staging = std::env::temp_dir().join(format!("angelica-map-{}", std::process::id()));
    runtime::installer::write_package(&staging, &files, &key)?;
    let mut packed = files.clone();
    for name in ["files.json", "files.json.sig"] {
        packed.insert(
            name.to_owned(),
            std::fs::read(staging.join(name)).map_err(|e| format!("{name}: {e}"))?,
        );
    }
    let _ = std::fs::remove_dir_all(&staging);
    let bytes = angelica_update::pack(&packed).map_err(|e| e.to_string())?;
    std::fs::write(&args.output, &bytes).map_err(|e| format!("{}: {e}", args.output.display()))?;
    Ok(serde_json::json!({ "pack": args.output, "files": packed.len() }))
}
