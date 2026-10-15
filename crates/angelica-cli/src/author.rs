//! Plugins from their authors, outside the official catalog: the author's
//! key, signed plugin files (`docs/formats/plugin-file-v1.md`), publishing
//! them behind a link that stays the same across versions, and installing
//! and updating them.
use crate::Output;
use clap::Args;
use ed25519_dalek::SigningKey;
use runtime::{
    author,
    installer::{self, Installation},
    manifest::Manifest,
    package::hex,
};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
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
pub struct PublishArgs {
    /// The plugin's folder, with `angelica.toml`.
    #[arg(default_value = ".")]
    folder: PathBuf,
    /// The component built for `wasm32-wasip2`.
    #[arg(long)]
    component: PathBuf,
    /// The GitHub repository (`owner/name`) whose release `<id>` holds the
    /// file; publishing uses the `gh` command and its sign-in.
    #[arg(long)]
    github: String,
    /// The author key, created when missing; by default
    /// `%APPDATA%/Angelica/author.key`.
    #[arg(long)]
    key: Option<PathBuf>,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// The plugin file (`.angelica`), or an `https` link to it.
    source: String,
    /// The installation folder `angelica.json` names.
    #[arg(long)]
    installation: PathBuf,
    #[command(flatten)]
    reporting: Output,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
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
    report(
        &args.reporting,
        "plugin-pack",
        (|| {
            let packed = pack_plugin(&args.folder, &args.component, args.key.as_ref())?;
            let output = args.output.clone().unwrap_or_else(|| {
                PathBuf::from(format!("{}-{}.angelica", packed.id, packed.version))
            });
            installer::write(&output, &packed.file)?;
            Ok(serde_json::json!({
                "plugin": packed.id,
                "version": packed.version,
                "file": output,
                "key": packed.key,
                "key_created": packed.key_created,
            }))
        })(),
    )
}

pub fn publish(args: &PublishArgs) -> ExitCode {
    report(&args.reporting, "plugin-publish", publish_plugin(args))
}

pub fn install(args: &InstallArgs) -> ExitCode {
    report(
        &args.reporting,
        "plugin-install",
        (|| {
            let installation = Installation::open(&args.installation)?;
            if args.source.contains("://") && !args.source.starts_with("https://") {
                return Err(format!(
                    "{}: plugins install only from https links",
                    args.source
                ));
            }
            let (file, url) = if args.source.starts_with("https://") {
                (installer::fetch(&args.source)?, Some(args.source.as_str()))
            } else {
                let path = Path::new(&args.source);
                (read_limited(path, author::MAX_FILE)?, None)
            };
            let installed = installer::install(&installation, file, url)?;
            serde_json::to_value(installed).map_err(|e| e.to_string())
        })(),
    )
}

pub fn update(args: &UpdateArgs) -> ExitCode {
    report(
        &args.reporting,
        "plugin-update",
        (|| {
            let installation = Installation::open(&args.installation)?;
            let outcomes = installer::update(&installation, installer::fetch);
            let mut report = serde_json::Map::new();
            for (id, outcome) in outcomes {
                report.insert(
                    id,
                    serde_json::to_value(outcome).map_err(|e| e.to_string())?,
                );
            }
            Ok(serde_json::Value::Object(report))
        })(),
    )
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
        return Ok((installer::read_key(path)?, false));
    }
    Ok((installer::create_key(path)?, true))
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

/// A plugin signed into a file.
struct Packed {
    id: String,
    version: String,
    name: String,
    description: String,
    file: Vec<u8>,
    key: PathBuf,
    key_created: bool,
}

fn pack_plugin(folder: &Path, component: &Path, key: Option<&PathBuf>) -> Result<Packed, String> {
    let manifest_bytes = read_limited(&folder.join("angelica.toml"), author::MAX_MANIFEST)?;
    let manifest = Manifest::parse(
        std::str::from_utf8(&manifest_bytes).map_err(|e| format!("angelica.toml: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let component = read_limited(component, author::MAX_COMPONENT)?;
    installer::compile(&component)?;
    let key_path = key_path(key)?;
    let (key, key_created) = author_key(&key_path)?;
    Ok(Packed {
        id: manifest.id().to_owned(),
        version: manifest.plugin.version.clone(),
        name: manifest.plugin.name.clone(),
        description: manifest.plugin.description.clone(),
        file: author::sign(&key, &manifest_bytes, &component)?,
        key: key_path,
        key_created,
    })
}

/// The link players install `id` from: the file `<id>.angelica` of the
/// release `<id>` in `repository`, which every version replaces.
fn github_link(repository: &str, id: &str) -> String {
    format!("https://github.com/{repository}/releases/download/{id}/{id}.angelica")
}

fn publish_plugin(args: &PublishArgs) -> Result<serde_json::Value, String> {
    let repository = args.github.trim();
    let valid = repository.split('/').count() == 2
        && repository.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        });
    if !valid {
        return Err(format!(
            "{repository} is not a GitHub repository owner/name"
        ));
    }
    let packed = pack_plugin(&args.folder, &args.component, args.key.as_ref())?;
    let link = github_link(repository, &packed.id);
    // Players update from the link: it must move to a newer version of the
    // same author. The release is read through gh, never through the link,
    // whose answer GitHub caches: a "not found" asked before the upload
    // would hide the file from players for a while.
    let exists = gh(&["release", "view", &packed.id, "--repo", repository])?.is_some();
    let current = if exists {
        gh(&[
            "release",
            "download",
            &packed.id,
            "--repo",
            repository,
            "--pattern",
            &format!("{}.angelica", packed.id),
            "--output",
            "-",
        ])?
    } else {
        None
    };
    if let Some(current) = current {
        let current = author::open(&current)?;
        if author::open(&packed.file)?.author != current.author {
            return Err(format!(
                "{link} holds a file signed with another author key than {}",
                packed.key.display()
            ));
        }
        let current =
            // The published version may declare permissions this Angelica
            // removed.
            Manifest::parse_earlier(
                std::str::from_utf8(&current.manifest).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        if !installer::newer(&packed.version, &current.plugin.version)? {
            return Err(format!(
                "{link} already has version {}; raise plugin.version",
                current.plugin.version
            ));
        }
    }
    let folder = std::env::temp_dir().join(format!("angelica-publish-{}", std::process::id()));
    let file = folder.join(format!("{}.angelica", packed.id));
    installer::write(&file, &packed.file)?;
    let published = upload(repository, &packed, &file, exists);
    let _ = std::fs::remove_dir_all(&folder);
    published?;
    Ok(serde_json::json!({
        "plugin": packed.id,
        "version": packed.version,
        "link": link,
        "key": packed.key,
        "key_created": packed.key_created,
    }))
}

/// Runs gh; its standard output when it succeeds, `None` when it fails.
fn gh(arguments: &[&str]) -> Result<Option<Vec<u8>>, String> {
    let output = Command::new("gh")
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("gh: {e}; install the GitHub command and sign in"))?;
    Ok(output.status.success().then_some(output.stdout))
}

/// Creates the release `<id>` when missing and replaces its file.
fn upload(repository: &str, packed: &Packed, file: &Path, exists: bool) -> Result<(), String> {
    if !exists
        && gh(&[
            "release",
            "create",
            &packed.id,
            "--repo",
            repository,
            "--title",
            &packed.name,
            "--notes",
            &packed.description,
        ])?
        .is_none()
    {
        return Err(format!(
            "gh could not create the release {} in {repository}",
            packed.id
        ));
    }
    let file = file.to_str().ok_or("the temporary folder is not Unicode")?;
    if gh(&[
        "release",
        "upload",
        &packed.id,
        file,
        "--clobber",
        "--repo",
        repository,
    ])?
    .is_none()
    {
        return Err(format!("gh could not upload {file}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_github_link_stays_the_same_across_versions() {
        assert_eq!(
            super::github_link("example/timers", "timers"),
            "https://github.com/example/timers/releases/download/timers/timers.angelica"
        );
    }
}
