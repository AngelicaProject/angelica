//! Runs `plugin-installer.exe` outside the game for the host: installs a
//! plugin from the link the player gives in the menu, updates the plugins
//! installed from links at start and then every few hours, and compiles a
//! plugin again when this runtime cannot load its compiled component. One
//! job runs at a time, on a host thread; the game never waits for it.
#![forbid(unsafe_code)]
use ed25519_dalek::{Signature, VerifyingKey};
use std::{
    collections::{BTreeSet, VecDeque},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

/// How often the plugins installed from links are updated.
pub const UPDATE_INTERVAL: Duration = Duration::from_hours(6);
/// The messages the menu keeps.
const MESSAGES: usize = 16;
/// The largest installer program and output the host reads.
const MAX_PROGRAM: u64 = 128 * 1024 * 1024;
const MAX_OUTPUT: u64 = 1024 * 1024;

/// A job for the installer program.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    Install(String),
    Update,
    Repair(Vec<String>),
}

/// The program, where it installs, and what it reported.
pub struct PluginInstaller {
    program: PathBuf,
    key: Option<[u8; 32]>,
    installation: PathBuf,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    /// Whether a job the player started is running or waiting.
    busy: Arc<Mutex<usize>>,
    messages: Arc<Mutex<VecDeque<String>>>,
    /// The plugins compiled again this session: once each.
    repaired: Mutex<BTreeSet<String>>,
    log: Arc<dyn Fn(&str) + Send + Sync>,
}

impl PluginInstaller {
    /// The installer `program`, whose detached signature `<program>.sig`
    /// must verify with `key`, for the installation in `installation`.
    #[must_use]
    pub fn new(
        program: PathBuf,
        key: Option<[u8; 32]>,
        installation: PathBuf,
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        Self {
            program,
            key,
            installation,
            jobs: Mutex::new(None),
            busy: Arc::new(Mutex::new(0)),
            messages: Arc::new(Mutex::new(VecDeque::new())),
            repaired: Mutex::new(BTreeSet::new()),
            log,
        }
    }

    /// Starts the thread that runs jobs; with `updates`, it updates the
    /// plugins installed from links now and then at that interval.
    pub fn start(self: &Arc<Self>, updates: Option<Duration>) {
        let (sender, receiver) = mpsc::channel();
        *self.jobs.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
        let installer = self.clone();
        let spawned = std::thread::Builder::new()
            .name("angelica-plugin-installer".into())
            .spawn(move || {
                let mut next_update = updates.map(|_| Instant::now());
                loop {
                    let job = match next_update {
                        Some(at) => {
                            match receiver
                                .recv_timeout(at.saturating_duration_since(Instant::now()))
                            {
                                Ok(job) => job,
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    next_update = updates.map(|every| Instant::now() + every);
                                    Job::Update
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            }
                        }
                        None => match receiver.recv() {
                            Ok(job) => job,
                            Err(_) => return,
                        },
                    };
                    installer.run(&job);
                }
            });
        if let Err(error) = spawned {
            self.note(format!("the plugin installer could not start: {error}"));
        }
    }

    /// Shows a message to the player with the installer's own.
    pub fn report(&self, message: &str) {
        self.note(message.to_owned());
    }

    /// Whether the installer program is there.
    #[must_use]
    pub fn program_exists(&self) -> bool {
        self.program.is_file()
    }

    /// Installs the plugin at `url`, an `https` link the player gave.
    ///
    /// # Errors
    /// A link that is not `https`, too long, or has spaces.
    pub fn install(&self, url: &str) -> Result<(), String> {
        let url = url.trim();
        if !url.starts_with("https://")
            || url.len() > crate::http::MAX_URL
            || url.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err("a plugin link starts with https:// and has no spaces".into());
        }
        *self.busy.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.note(format!("Installing from {url}"));
        self.send(Job::Install(url.to_owned()));
        Ok(())
    }

    /// Compiles these plugins again, once each per session.
    pub fn repair(&self, ids: Vec<String>) {
        let mut repaired = self.repaired.lock().unwrap_or_else(PoisonError::into_inner);
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| repaired.insert(id.clone()))
            .collect();
        drop(repaired);
        if !ids.is_empty() {
            self.send(Job::Repair(ids));
        }
    }

    /// Whether an installation the player started has not finished.
    #[must_use]
    pub fn busy(&self) -> bool {
        *self.busy.lock().unwrap_or_else(PoisonError::into_inner) > 0
    }

    /// What the installer reported, oldest first.
    #[must_use]
    pub fn messages(&self) -> Vec<String> {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    fn send(&self, job: Job) {
        let jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let unsent = match jobs.as_ref() {
            Some(jobs) => jobs.send(job).err().map(|error| error.0),
            None => Some(job),
        };
        drop(jobs);
        if let Some(job) = unsent {
            self.finish(&job, Err("the plugin installer is not running".into()));
        }
    }

    fn note(&self, message: String) {
        (self.log)(&message);
        let mut messages = self.messages.lock().unwrap_or_else(PoisonError::into_inner);
        if messages.len() == MESSAGES {
            messages.pop_front();
        }
        messages.push_back(message);
    }

    fn run(&self, job: &Job) {
        let mut arguments: Vec<String> = match job {
            Job::Install(url) => vec!["install".into(), url.clone()],
            Job::Update => vec!["update".into()],
            Job::Repair(ids) => std::iter::once("repair".into())
                .chain(ids.iter().cloned())
                .collect(),
        };
        arguments.push("--installation".into());
        arguments.push(self.installation.display().to_string());
        let result = run_program(&self.program, self.key.as_ref(), &arguments);
        self.finish(job, result);
    }

    /// Turns the program's report into messages for the player.
    fn finish(&self, job: &Job, result: Result<serde_json::Value, String>) {
        if matches!(job, Job::Install(_)) {
            let mut busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
            *busy = busy.saturating_sub(1);
        }
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                self.note(match job {
                    Job::Install(url) => format!("Could not install from {url}: {error}"),
                    Job::Update => format!("Could not check plugin updates: {error}"),
                    Job::Repair(_) => format!("Could not compile plugins again: {error}"),
                });
                return;
            }
        };
        if let Some(installed) = report.get("installed") {
            self.note(format!(
                "Installed {} {}; it appears in the list within a second",
                text(&installed["plugin"]),
                text(&installed["version"])
            ));
        }
        if let Some(updates) = report.get("updates").and_then(|u| u.as_object()) {
            for (id, outcome) in updates {
                match outcome["outcome"].as_str() {
                    Some("updated") => self.note(format!(
                        "Updated {id} from {} to {}",
                        text(&outcome["from"]),
                        text(&outcome["to"])
                    )),
                    Some("failed") => self.note(format!(
                        "Could not update {id}: {}",
                        text(&outcome["reason"])
                    )),
                    _ => {}
                }
            }
        }
        if let Some(repaired) = report.get("repaired").and_then(|r| r.as_object()) {
            for (id, outcome) in repaired {
                self.note(match outcome.get("error") {
                    Some(error) => format!("Could not compile {id} again: {}", text(error)),
                    None => format!("Compiled {id} again for this version of Angelica"),
                });
            }
        }
    }
}

fn text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Runs the program after checking its signature, holding it open so it
/// cannot be replaced in between, and returns the JSON it printed.
fn run_program(
    program: &Path,
    key: Option<&[u8; 32]>,
    arguments: &[String],
) -> Result<serde_json::Value, String> {
    let held = open_verified(program, key)?;
    let mut command = std::process::Command::new(program);
    command
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    drop(held);
    let mut output = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout
            .take(MAX_OUTPUT)
            .read_to_end(&mut output)
            .map_err(|e| e.to_string())?;
    }
    child.wait().map_err(|e| e.to_string())?;
    let report: serde_json::Value = serde_json::from_slice(&output)
        .map_err(|_| "the plugin installer gave no report".to_owned())?;
    if let Some(error) = report.get("error") {
        return Err(text(&error["message"]));
    }
    Ok(report)
}

/// Opens the program for reading only, sharing it only for reading, and
/// checks its detached Ed25519 signature `<program>.sig` with `key`.
fn open_verified(program: &Path, key: Option<&[u8; 32]>) -> Result<std::fs::File, String> {
    let key = VerifyingKey::from_bytes(key.ok_or("no compiled release key")?)
        .map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        options.share_mode(FILE_SHARE_READ);
    }
    let mut file = options
        .open(program)
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_PROGRAM + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PROGRAM {
        return Err(format!("{} is too large", program.display()));
    }
    let mut signature_path = program.as_os_str().to_owned();
    signature_path.push(".sig");
    let signature = std::fs::read(&signature_path)
        .map_err(|e| format!("{}: {e}", Path::new(&signature_path).display()))?;
    let signature = Signature::from_slice(&signature).map_err(|e| e.to_string())?;
    key.verify_strict(&bytes, &signature)
        .map_err(|_| format!("{} is not signed by Angelica", program.display()))?;
    Ok(file)
}
