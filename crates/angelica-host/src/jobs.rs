//! Jobs (the `jobs` interface): `run-job` in a fresh instance of the plugin
//! on a host thread.
#![forbid(unsafe_code)]
use crate::{
    error::{Error, Kind, Result},
    sandbox::{self, Context, PluginPre, Role, Sandbox, State},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

/// Jobs of one plugin that run at once.
pub const MAX_RUNNING: usize = 4;
/// Bytes of a job's input or output.
pub const MAX_DATA: usize = 16 * 1024 * 1024;

pub enum JobState {
    Running(f32, String),
    Finished(Vec<u8>),
    Failed(String),
}

/// One job, shared by its thread and the plugin's main instance.
pub struct Slot {
    cancelled: AtomicBool,
    progress: Mutex<(f32, String)>,
    result: Mutex<Option<std::result::Result<Vec<u8>, String>>>,
}

impl Slot {
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn report(&self, fraction: f32, mut message: String) {
        message.truncate(message.floor_char_boundary(1024));
        *self.progress.lock().unwrap_or_else(PoisonError::into_inner) =
            (fraction.clamp(0.0, 1.0), message);
    }

    fn finish(&self, result: std::result::Result<Vec<u8>, String>) {
        *self.result.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
    }
}

/// The jobs of one plugin.
pub struct Jobs {
    sandbox: Arc<Sandbox>,
    pre: PluginPre<State>,
    time: Duration,
    next: AtomicU64,
    slots: Mutex<BTreeMap<u64, Arc<Slot>>>,
}

impl Jobs {
    #[must_use]
    pub fn new(sandbox: Arc<Sandbox>, pre: PluginPre<State>, time: Duration) -> Self {
        Self {
            sandbox,
            pre,
            time,
            next: AtomicU64::new(1),
            slots: Mutex::new(BTreeMap::new()),
        }
    }

    /// # Errors
    /// Refuses a fifth running job, oversized input, and an invalid name.
    pub fn start(&self, context: &Arc<Context>, name: &str, input: Vec<u8>) -> Result<u64> {
        if name.is_empty() || name.len() > 256 {
            return Err(Error::invalid("a job name has 1 to 256 bytes"));
        }
        if input.len() > MAX_DATA {
            return Err(Error::new(
                Kind::LimitExceeded,
                "a job's input is at most 16 MiB",
            ));
        }
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let running = slots
            .values()
            .filter(|slot| {
                slot.result
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_none()
            })
            .count();
        if running >= MAX_RUNNING {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("at most {MAX_RUNNING} jobs run at once"),
            ));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(Slot {
            cancelled: AtomicBool::new(false),
            progress: Mutex::new((0.0, String::new())),
            result: Mutex::new(None),
        });
        let (sandbox, pre, time, context, name) = (
            self.sandbox.clone(),
            self.pre.clone(),
            self.time,
            context.clone(),
            name.to_owned(),
        );
        let thread_slot = slot.clone();
        std::thread::Builder::new()
            .name(format!("angelica-job-{}", context.id))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&sandbox, &pre, time, context, &thread_slot, &name, &input)
                }))
                .unwrap_or_else(|_| Err("the job panicked in the host".into()));
                thread_slot.finish(result);
            })
            .map_err(|e| Error::new(Kind::Io, format!("cannot start a job thread: {e}")))?;
        slots.insert(id, slot);
        Ok(id)
    }

    /// # Errors
    /// Fails for an unknown id.
    pub fn poll(&self, id: u64) -> Result<JobState> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots
            .get(&id)
            .ok_or_else(|| Error::new(Kind::NotFound, format!("no job {id}")))?
            .clone();
        let result = slot
            .result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        Ok(match result {
            None => {
                let (fraction, message) = slot
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                JobState::Running(fraction, message)
            }
            Some(result) => {
                slots.remove(&id);
                match result {
                    Ok(output) => JobState::Finished(output),
                    Err(reason) => JobState::Failed(reason),
                }
            }
        })
    }

    /// # Errors
    /// Fails for an unknown id.
    pub fn cancel(&self, id: u64) -> Result<()> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots
            .get(&id)
            .ok_or_else(|| Error::new(Kind::NotFound, format!("no job {id}")))?;
        slot.cancelled.store(true, Ordering::Release);
        Ok(())
    }

    /// Cancels every job and forgets them; their threads end at the next
    /// epoch check.
    pub fn cancel_all(&self) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        for slot in slots.values() {
            slot.cancelled.store(true, Ordering::Release);
        }
        slots.clear();
    }
}

fn run(
    sandbox: &Sandbox,
    pre: &PluginPre<State>,
    time: Duration,
    context: Arc<Context>,
    slot: &Arc<Slot>,
    name: &str,
    input: &[u8],
) -> std::result::Result<Vec<u8>, String> {
    let mut store = sandbox::store(sandbox, context, Role::Job(slot.clone()));
    sandbox::budget(&mut store, time, u64::MAX);
    let bindings = pre
        .instantiate(&mut store)
        .map_err(|e| sandbox::describe_trap(&e))?;
    let result = bindings
        .angelica_plugin_guest()
        .call_run_job(&mut store, name, input)
        .map_err(|e| {
            if slot.cancelled() {
                "the job was cancelled".to_owned()
            } else {
                sandbox::describe_trap(&e)
            }
        })?;
    let output = result?;
    if output.len() > MAX_DATA {
        return Err("the job's output is larger than 16 MiB".into());
    }
    Ok(output)
}
