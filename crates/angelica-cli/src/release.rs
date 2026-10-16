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
    /// Review a catalog entry's pull request: whether it merges by itself.
    Review(ReviewArgs),
    /// Write the catalog's signed index from the registry and the built files.
    Catalog(CatalogArgs),
}

#[derive(Debug, Args)]
pub struct ReviewArgs {
    /// The entry in the pull request, `plugins/<id>.toml`.
    #[arg(long)]
    entry: PathBuf,
    /// The plugin file of the entry's tag, from the plugin's repository.
    #[arg(long)]
    author_file: PathBuf,
    /// The entry the catalog has now; absent for a new plugin.
    #[arg(long, requires = "previous_file")]
    previous_entry: Option<PathBuf>,
    /// The plugin file the catalog publishes now.
    #[arg(long, requires = "previous_entry")]
    previous_file: Option<PathBuf>,
    /// The released game map's folder, whose domains decide whether a new
    /// plugin merges by itself; without it, a new plugin waits.
    #[arg(long)]
    map: Option<PathBuf>,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct CatalogArgs {
    /// The registry checkout, with `plugins/` and `withdrawn.toml`.
    #[arg(long)]
    registry: PathBuf,
    /// The folder of the plugin files the catalog built, `<id>.angelica`.
    #[arg(long)]
    files: PathBuf,
    /// When the index was built, RFC 3339 in UTC.
    #[arg(long)]
    published: String,
    /// Where to write `catalog.json`; its signature goes beside it as `.sig`.
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    reporting: Output,
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
        ReleaseCommand::Review(args) => (review(args), &args.reporting),
        ReleaseCommand::Catalog(args) => (catalog(args), &args.reporting),
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

fn read_entry(path: &std::path::Path) -> Result<crate::catalog::Entry, String> {
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("{}: not plugins/<id>.toml", path.display()))?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::catalog::Entry::parse(&text, id).map_err(|e| format!("{}: {e}", path.display()))
}

fn review(args: &ReviewArgs) -> Result<serde_json::Value, String> {
    let entry = read_entry(&args.entry)?;
    let author_file = std::fs::read(&args.author_file)
        .map_err(|e| format!("{}: {e}", args.author_file.display()))?;
    let previous = match (&args.previous_entry, &args.previous_file) {
        (Some(entry), Some(file)) => {
            let before = read_entry(entry)?;
            let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
            let (_, manifest) = crate::catalog::open_earlier(&bytes)?;
            Some((before, manifest))
        }
        _ => None,
    };
    let domains = match &args.map {
        Some(folder) => Some(
            angelica_map::Map::load(folder)
                .map_err(|e| format!("{}: {e}", folder.display()))?
                .objects()
                .domains
                .clone(),
        ),
        None => None,
    };
    let review = crate::catalog::review(
        &entry,
        &author_file,
        previous.as_ref().map(|(entry, manifest)| (entry, manifest)),
        domains.as_ref(),
    )?;
    serde_json::to_value(review).map_err(|e| e.to_string())
}

fn catalog(args: &CatalogArgs) -> Result<serde_json::Value, String> {
    let key = release_key()?;
    let release = key.verifying_key().to_bytes();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(args.registry.join("plugins"))
        .map_err(|e| format!("plugins: {e}"))?
        .filter_map(Result::ok)
        .map(|item| item.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let mut plugins = Vec::new();
    for path in paths {
        let entry = read_entry(&path)?;
        let file = args.files.join(format!("{}.angelica", entry.id));
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        plugins.push(crate::catalog::listed(&entry, &bytes, &release)?);
    }
    let withdrawn = match std::fs::read_to_string(args.registry.join("withdrawn.toml")) {
        Ok(text) => {
            let withdrawn: crate::catalog::Withdrawn =
                toml::from_str(&text).map_err(|e| format!("withdrawn.toml: {e}"))?;
            if withdrawn.format != 1 {
                return Err("withdrawn.toml: unsupported format".into());
            }
            withdrawn.withdrawn
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("withdrawn.toml: {e}")),
    };
    let index = serde_json::json!({
        "format": 1,
        "published": args.published,
        "plugins": plugins,
        "withdrawn": withdrawn,
    });
    let mut json = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    json.push(b'\n');
    let signature = ed25519_dalek::Signer::sign(&key, &json).to_bytes();
    std::fs::write(&args.output, &json).map_err(|e| format!("{}: {e}", args.output.display()))?;
    let mut sig = args.output.clone().into_os_string();
    sig.push(".sig");
    std::fs::write(&sig, signature).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "catalog": args.output, "plugins": plugins.len() }))
}
