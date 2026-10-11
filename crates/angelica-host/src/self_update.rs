//! Angelica updates itself (`docs/development/releases.md#host-updates`):
//! the feed of the player's channel names the newest release; a newer one
//! is downloaded, checked against the feed and the release key, written
//! into its own version folder, and named in `current.json`, which the proxy
//! reads at the next game start (`docs/architecture/loading.md#updates`).
use angelica_update::{Channel, Feed, Product};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Where Angelica's releases are published.
pub const REPOSITORY: &str = "AngelicaProject/angelica";
/// The files a release installs into its version folder; each comes with its
/// release signature.
pub const FILES: [&str; 2] = ["runtime.dll", "plugin-installer.exe"];
/// When the first check runs after the game starts, and how often after.
pub const FIRST_CHECK: Duration = Duration::from_secs(20);
pub const CHECK_EVERY: Duration = Duration::from_hours(6);

/// What the last check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Built without a release channel: development builds never update.
    Development,
    NotChecked,
    Checking,
    UpToDate,
    /// A newer version is installed and starts with the game.
    Ready(String),
    Failed(String),
}

/// Reads a URL.
pub trait Fetch: Send + Sync {
    /// The body of `url`, at most `limit` bytes.
    ///
    /// # Errors
    /// Any failure, in words.
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, String>;
}

/// What an update needs to know about the running Angelica.
#[derive(Debug, Clone)]
pub struct Setup {
    pub installation: PathBuf,
    pub repository: String,
    pub key: [u8; 32],
    pub running: semver::Version,
}

/// The channel this build was released on, from `ANGELICA_RELEASE_CHANNEL`
/// at build time; none for a development build, which never updates.
#[must_use]
pub fn build_channel() -> Option<Channel> {
    match option_env!("ANGELICA_RELEASE_CHANNEL") {
        Some("stable") => Some(Channel::Stable),
        Some("nightly") => Some(Channel::Nightly),
        _ => None,
    }
}

/// The state the menu shows, shared with the update thread.
#[derive(Default)]
pub struct SelfUpdate {
    status: Mutex<Option<Status>>,
    requested: AtomicBool,
}

impl SelfUpdate {
    #[must_use]
    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(Status::NotChecked)
    }

    pub fn set(&self, status: Status) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = Some(status);
    }

    /// Checks now instead of at the next interval.
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

/// Reads release files from GitHub over the host's HTTPS client.
pub struct Http(crate::http::Client);

impl Http {
    #[must_use]
    pub fn new() -> Self {
        Self(crate::http::Client::new(
            vec!["github.com".into(), "*.githubusercontent.com".into()],
            "angelica",
            env!("CARGO_PKG_VERSION"),
        ))
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for Http {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
        use std::io::Read as _;
        let request = crate::http::Request {
            url: url.to_owned(),
            headers: Vec::new(),
        };
        let (head, reader) = self
            .0
            .send(&request, Duration::from_mins(5), limit, || false)
            .map_err(|e| format!("{url}: {}", e.message))?;
        if head.status != 200 {
            return Err(format!("{url}: the server answered {}", head.status));
        }
        let mut body = Vec::new();
        reader
            .take(limit + 1)
            .read_to_end(&mut body)
            .map_err(|e| format!("{url}: {e}"))?;
        if body.len() as u64 > limit {
            return Err(format!("{url} is larger than {limit} bytes"));
        }
        Ok(body)
    }
}

/// Checks the feeds of `channel` and installs a newer version.
///
/// # Errors
/// What failed, in words: a feed, a download, a check, or writing.
pub fn update(setup: &Setup, channel: Channel, fetch: &dyn Fetch) -> Result<Status, String> {
    let channels: &[Channel] = match channel {
        Channel::Stable => &[Channel::Stable],
        Channel::Nightly => &[Channel::Stable, Channel::Nightly],
    };
    let mut feeds = Vec::new();
    let mut failures = Vec::new();
    for &channel in channels {
        match read_feed(setup, channel, fetch) {
            Ok(feed) => feeds.push(feed),
            Err(error) => failures.push(error),
        }
    }
    if feeds.is_empty() {
        return Err(failures.join("; "));
    }
    let Some(feed) = angelica_update::newer(&setup.running, &feeds) else {
        return Ok(Status::UpToDate);
    };
    let version = feed.version.to_string();
    if current(&setup.installation).as_deref() == Some(version.as_str()) {
        return Ok(Status::Ready(version));
    }
    install(setup, feed, fetch)?;
    Ok(Status::Ready(version))
}

fn read_feed(setup: &Setup, channel: Channel, fetch: &dyn Fetch) -> Result<Feed, String> {
    let url = angelica_update::feed_url(&setup.repository, Product::Angelica, channel);
    let json = fetch.get(&url, angelica_update::MAX_FEED as u64)?;
    let signature = fetch.get(&format!("{url}.sig"), 64)?;
    Feed::verify(&json, &signature, &setup.key, Product::Angelica, channel)
        .map_err(|e| format!("{}: {e}", channel.name()))
}

/// The version `current.json` names.
#[must_use]
pub fn current(installation: &Path) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Current {
        format: u32,
        version: String,
    }
    let bytes = std::fs::read(installation.join("current.json")).ok()?;
    let current: Current = serde_json::from_slice(&bytes).ok()?;
    (current.format == 1).then_some(current.version)
}

fn install(setup: &Setup, feed: &Feed, fetch: &dyn Fetch) -> Result<(), String> {
    let version = feed.version.to_string();
    let versions = setup.installation.join("versions");
    let folder = versions.join(&version);
    let partial = versions.join(format!(".{version}.partial"));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    let key = ed25519_dalek::VerifyingKey::from_bytes(&setup.key).map_err(|e| e.to_string())?;
    for name in FILES {
        let file = feed
            .files
            .get(name)
            .ok_or_else(|| format!("the release has no {name}"))?;
        let url = angelica_update::asset_url(&setup.repository, feed, file);
        let bytes = fetch.get(&url, file.size)?;
        file.check(&bytes).map_err(|e| e.to_string())?;
        let signature = fetch.get(&format!("{url}.sig"), 64)?;
        let parsed = ed25519_dalek::Signature::from_slice(&signature)
            .map_err(|_| format!("{name}.sig is not a signature"))?;
        key.verify_strict(&bytes, &parsed)
            .map_err(|_| format!("{name}'s release signature does not verify"))?;
        for (path, bytes) in [
            (partial.join(name), bytes.as_slice()),
            (partial.join(format!("{name}.sig")), signature.as_slice()),
        ] {
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    // A folder of this version left by an earlier try is replaced; one in
    // use is not, and the proxy refuses it if it is broken.
    if folder.exists() {
        std::fs::remove_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    std::fs::rename(&partial, &folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let pointer = setup.installation.join("current.json");
    let next = setup.installation.join("current.json.new");
    std::fs::write(
        &next,
        serde_json::json!({"format": 1, "version": version}).to_string(),
    )
    .map_err(|e| format!("{}: {e}", next.display()))?;
    std::fs::rename(&next, &pointer).map_err(|e| format!("{}: {e}", pointer.display()))?;
    remove_old(&versions, &version);
    Ok(())
}

/// Removes version folders other than `keep`; a folder in use stays.
fn remove_old(versions: &Path, keep: &str) {
    let Ok(listing) = std::fs::read_dir(versions) else {
        return;
    };
    for item in listing.filter_map(Result::ok) {
        if item.file_name() != keep {
            let _ = std::fs::remove_dir_all(item.path());
        }
    }
}

/// Runs the checks on a thread of its own: the first after
/// [`FIRST_CHECK`], then every [`CHECK_EVERY`], and when `requested` is
/// requested. `channel` is read before each check, so a change applies at
/// the next one.
pub fn spawn(
    setup: Setup,
    updates: Arc<SelfUpdate>,
    fetch: Arc<dyn Fetch>,
    channel: impl Fn() -> Channel + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("angelica-updates".into())
        .spawn(move || {
            let mut wait = FIRST_CHECK;
            loop {
                let step = Duration::from_millis(500);
                let mut waited = Duration::ZERO;
                while waited < wait && !updates.requested.swap(false, Ordering::AcqRel) {
                    std::thread::sleep(step);
                    waited += step;
                }
                updates.set(Status::Checking);
                let status = match update(&setup, channel(), fetch.as_ref()) {
                    Ok(status) => status,
                    Err(error) => Status::Failed(error),
                };
                updates.set(status);
                wait = CHECK_EVERY;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_update::File;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::{BTreeMap, HashMap};

    const KEY: [u8; 32] = [5; 32];

    struct Releases(Mutex<HashMap<String, Vec<u8>>>);

    impl Fetch for Releases {
        fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
            let body = self
                .0
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or(format!("404 {url}"))?;
            if body.len() as u64 > limit {
                return Err(format!("{url} is larger than {limit} bytes"));
            }
            Ok(body)
        }
    }

    fn publish(releases: &Releases, channel: Channel, version: &str, runtime: &[u8]) {
        let key = SigningKey::from_bytes(&KEY);
        let installer = b"installer".as_slice();
        let feed = Feed {
            format: 1,
            product: Product::Angelica,
            channel,
            version: version.parse().unwrap(),
            published: "2026-10-20T12:00:00Z".into(),
            files: BTreeMap::from([
                ("runtime.dll".into(), File::of("runtime.dll", runtime)),
                (
                    "plugin-installer.exe".into(),
                    File::of("plugin-installer.exe", installer),
                ),
            ]),
        };
        let (json, signature) = feed.sign(&key);
        let url = angelica_update::feed_url(REPOSITORY, Product::Angelica, channel);
        let mut files = releases.0.lock().unwrap();
        files.insert(url.clone(), json);
        files.insert(format!("{url}.sig"), signature.to_vec());
        for (name, bytes) in [
            ("runtime.dll", runtime),
            ("plugin-installer.exe", installer),
        ] {
            let asset = angelica_update::asset_url(REPOSITORY, &feed, &feed.files[name]);
            files.insert(asset.clone(), bytes.to_vec());
            files.insert(format!("{asset}.sig"), key.sign(bytes).to_bytes().to_vec());
        }
    }

    fn setup(folder: &Path, running: &str) -> Setup {
        Setup {
            installation: folder.to_path_buf(),
            repository: REPOSITORY.into(),
            key: SigningKey::from_bytes(&KEY).verifying_key().to_bytes(),
            running: running.parse().unwrap(),
        }
    }

    #[test]
    fn a_newer_release_goes_into_its_own_folder_and_becomes_current() {
        let folder = tempfile::tempdir().unwrap();
        let releases = Releases(Mutex::default());
        publish(&releases, Channel::Stable, "0.2.0", b"runtime 0.2.0");
        // The running version's folder is never touched.
        std::fs::create_dir_all(folder.path().join("versions/0.1.0")).unwrap();
        std::fs::write(folder.path().join("versions/0.1.0/runtime.dll"), b"old").unwrap();

        let status = update(&setup(folder.path(), "0.1.0"), Channel::Stable, &releases).unwrap();
        assert_eq!(status, Status::Ready("0.2.0".into()));
        assert_eq!(current(folder.path()).as_deref(), Some("0.2.0"));
        let installed = folder.path().join("versions/0.2.0");
        assert_eq!(
            std::fs::read(installed.join("runtime.dll")).unwrap(),
            b"runtime 0.2.0"
        );
        assert_eq!(
            std::fs::read(installed.join("runtime.dll.sig"))
                .unwrap()
                .len(),
            64
        );
        assert!(installed.join("plugin-installer.exe.sig").exists());

        // Checking again finds the same version ready.
        let status = update(&setup(folder.path(), "0.1.0"), Channel::Stable, &releases).unwrap();
        assert_eq!(status, Status::Ready("0.2.0".into()));
        // After the restart into 0.2.0 it is up to date.
        let status = update(&setup(folder.path(), "0.2.0"), Channel::Stable, &releases).unwrap();
        assert_eq!(status, Status::UpToDate);
    }

    #[test]
    fn nothing_older_and_nothing_unchecked_is_installed() {
        let folder = tempfile::tempdir().unwrap();
        let releases = Releases(Mutex::default());
        publish(&releases, Channel::Stable, "0.1.0", b"runtime 0.1.0");
        let status = update(
            &setup(folder.path(), "0.1.1-nightly.3"),
            Channel::Stable,
            &releases,
        )
        .unwrap();
        assert_eq!(status, Status::UpToDate, "never older");

        publish(&releases, Channel::Stable, "0.2.0", b"runtime 0.2.0");
        let url = angelica_update::asset_url(
            REPOSITORY,
            &Feed {
                format: 1,
                product: Product::Angelica,
                channel: Channel::Stable,
                version: "0.2.0".parse().unwrap(),
                published: String::new(),
                files: BTreeMap::new(),
            },
            &File::of("runtime.dll", b""),
        );
        releases
            .0
            .lock()
            .unwrap()
            .insert(url, b"runtime 0.2.1".to_vec());
        let error = update(&setup(folder.path(), "0.1.0"), Channel::Stable, &releases).unwrap_err();
        assert!(error.contains("does not match"), "{error}");
        assert_eq!(
            current(folder.path()),
            None,
            "a failed download changes nothing"
        );
        assert!(!folder.path().join("versions/0.2.0").exists());
    }

    #[test]
    fn nightly_takes_the_newer_of_both_feeds_and_a_stranger_key_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let releases = Releases(Mutex::default());
        publish(&releases, Channel::Stable, "0.2.0", b"stable");
        publish(&releases, Channel::Nightly, "0.2.1-nightly.5", b"nightly");
        let status = update(&setup(folder.path(), "0.2.0"), Channel::Nightly, &releases).unwrap();
        assert_eq!(status, Status::Ready("0.2.1-nightly.5".into()));
        assert_eq!(
            update(&setup(folder.path(), "0.2.0"), Channel::Stable, &releases).unwrap(),
            Status::UpToDate
        );

        let mut stranger = setup(folder.path(), "0.1.0");
        stranger.key = SigningKey::from_bytes(&[6; 32]).verifying_key().to_bytes();
        let error = update(&stranger, Channel::Stable, &releases).unwrap_err();
        assert!(error.contains("signature"), "{error}");
    }
}
