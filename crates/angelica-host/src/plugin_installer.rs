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

/// What the installer reported, for the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// Installing from the link has started.
    Installing(String),
    Installed {
        plugin: String,
        version: String,
    },
    Updated {
        plugin: String,
        from: String,
        to: String,
    },
    /// A plugin compiled again for this runtime.
    Compiled(String),
    /// A job or part of one failed, with the reason.
    Failed(Failure, String),
}

/// What failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The installer could not start.
    Start,
    /// Installing from the link.
    Install(String),
    /// Checking updates.
    Updates,
    /// Updating the plugin.
    Update(String),
    /// Compiling plugins again.
    Compiling,
    /// Compiling the plugin again.
    Compile(String),
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Installing(url) => write!(f, "Installing from {url}"),
            Self::Installed { plugin, version } => write!(
                f,
                "Installed {plugin} {version}; it appears in the list within a second"
            ),
            Self::Updated { plugin, from, to } => write!(f, "Updated {plugin} from {from} to {to}"),
            Self::Compiled(plugin) => {
                write!(f, "Compiled {plugin} again for this version of Angelica")
            }
            Self::Failed(failure, reason) => match failure {
                Failure::Start => write!(f, "the plugin installer could not start: {reason}"),
                Failure::Install(url) => write!(f, "Could not install from {url}: {reason}"),
                Failure::Updates => write!(f, "Could not check plugin updates: {reason}"),
                Failure::Update(plugin) => write!(f, "Could not update {plugin}: {reason}"),
                Failure::Compiling => write!(f, "Could not compile plugins again: {reason}"),
                Failure::Compile(plugin) => {
                    write!(f, "Could not compile {plugin} again: {reason}")
                }
            },
        }
    }
}

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
    /// The latest reports, each with its number in the session.
    messages: Arc<Mutex<VecDeque<(u64, Report)>>>,
    reported: Mutex<u64>,
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
            reported: Mutex::new(0),
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
            self.note(Report::Failed(Failure::Start, error.to_string()));
        }
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
        self.note(Report::Installing(url.to_owned()));
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

    /// What the installer reported, oldest first, each with its number in
    /// the session.
    #[must_use]
    pub fn messages(&self) -> Vec<(u64, Report)> {
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

    fn note(&self, report: Report) {
        (self.log)(&report.to_string());
        let mut reported = self.reported.lock().unwrap_or_else(PoisonError::into_inner);
        *reported += 1;
        let mut messages = self.messages.lock().unwrap_or_else(PoisonError::into_inner);
        if messages.len() == MESSAGES {
            messages.pop_front();
        }
        messages.push_back((*reported, report));
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
                let failure = match job {
                    Job::Install(url) => Failure::Install(url.clone()),
                    Job::Update => Failure::Updates,
                    Job::Repair(_) => Failure::Compiling,
                };
                self.note(Report::Failed(failure, error));
                return;
            }
        };
        if let Some(installed) = report.get("installed") {
            self.note(Report::Installed {
                plugin: text(&installed["plugin"]),
                version: text(&installed["version"]),
            });
        }
        if let Some(updates) = report.get("updates").and_then(|u| u.as_object()) {
            for (id, outcome) in updates {
                match outcome["outcome"].as_str() {
                    Some("updated") => self.note(Report::Updated {
                        plugin: id.clone(),
                        from: text(&outcome["from"]),
                        to: text(&outcome["to"]),
                    }),
                    Some("failed") => self.note(Report::Failed(
                        Failure::Update(id.clone()),
                        text(&outcome["reason"]),
                    )),
                    _ => {}
                }
            }
        }
        if let Some(repaired) = report.get("repaired").and_then(|r| r.as_object()) {
            for (id, outcome) in repaired {
                self.note(match outcome.get("error") {
                    Some(error) => Report::Failed(Failure::Compile(id.clone()), text(error)),
                    None => Report::Compiled(id.clone()),
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
