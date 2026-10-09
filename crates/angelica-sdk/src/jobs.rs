//! Long work in a separate instance, with typed input and output.
//!
//! The main instance starts a job with a value, keeps the [`Job`], and polls
//! it in [`crate::Plugin::tick`]; the job instance decodes the value with
//! [`run`]. Values travel as JSON, so any type with serde's `Serialize` and
//! `Deserialize` works on both sides.
//!
//! ```ignore
//! use angelica_sdk::jobs::{self, Job, Poll};
//!
//! // In the main instance:
//! self.lookup = Some(jobs::start::<u32>("count", &"some text".to_owned())?);
//!
//! // In `Plugin::tick`:
//! if let Some(job) = &mut self.lookup {
//!     match job.poll() {
//!         Poll::Running => self.fraction = job.progress().fraction,
//!         Poll::Done(result) => {
//!             self.lookup = None;
//!             let count = result?;
//!         }
//!     }
//! }
//!
//! // In `Plugin::run_job`:
//! match name {
//!     "count" => jobs::run(&input, |text: String| Ok(text.len() as u32)),
//!     _ => Err(format!("no job {name}")),
//! }
//! ```
use std::marker::PhantomData;

use serde::{Serialize, de::DeserializeOwned};

use crate::api::jobs as api;
pub use api::JobProgress as Progress;

/// A running job whose output is an `O`. Dropping it before it is done
/// cancels the job.
#[derive(Debug)]
pub struct Job<O> {
    id: u64,
    progress: Progress,
    done: bool,
    output: PhantomData<fn() -> O>,
}

/// What [`Job::poll`] found.
#[derive(Debug)]
pub enum Poll<O> {
    /// Still running; [`Job::progress`] has the last reported progress.
    Running,
    /// The job's output, or why it failed: `run-job` returned an error,
    /// trapped, ran out of time, or was cancelled, or its output was not
    /// an `O`. Returned once; the job is forgotten afterwards.
    Done(Result<O, String>),
}

/// Starts the job `name` with `input`. Called from the main instance.
///
/// # Errors
/// See `jobs.start`; at most four jobs of a plugin run at once.
pub fn start<O: DeserializeOwned>(
    name: &str,
    input: &impl Serialize,
) -> Result<Job<O>, crate::Error> {
    let input = serde_json::to_vec(input).map_err(|e| crate::Error {
        kind: crate::ErrorKind::InvalidArgument,
        message: format!("the input of job {name}: {e}"),
    })?;
    api::start(name, &input).map(|id| Job {
        id,
        progress: Progress {
            fraction: 0.0,
            message: String::new(),
        },
        done: false,
        output: PhantomData,
    })
}

impl<O: DeserializeOwned> Job<O> {
    /// The job's state. Called from the main instance, usually in `tick`.
    /// After [`Poll::Done`] the job is forgotten and polling again reports
    /// it failed.
    pub fn poll(&mut self) -> Poll<O> {
        if self.done {
            return Poll::Done(Err("the job's result was already returned".into()));
        }
        let state = match api::poll(self.id) {
            Ok(state) => state,
            Err(error) => {
                self.done = true;
                return Poll::Done(Err(error.message));
            }
        };
        match state {
            api::State::Running(progress) => {
                self.progress = progress;
                Poll::Running
            }
            api::State::Finished(output) => {
                self.done = true;
                self.progress.fraction = 1.0;
                Poll::Done(
                    serde_json::from_slice(&output)
                        .map_err(|e| format!("the job's output is not what was expected: {e}")),
                )
            }
            api::State::Failed(reason) => {
                self.done = true;
                Poll::Done(Err(reason))
            }
        }
    }
}

impl<O> Job<O> {
    /// The progress the job last reported, as of the last [`Job::poll`].
    #[must_use]
    pub fn progress(&self) -> &Progress {
        &self.progress
    }
}

impl<O> Drop for Job<O> {
    fn drop(&mut self) {
        if !self.done {
            // A job that ended since the last poll is no longer running.
            let _ = api::cancel(self.id);
        }
    }
}

/// Runs a job's work in [`crate::Plugin::run_job`]: decodes `input` as an
/// `I`, calls `work`, and encodes its output.
///
/// # Errors
/// The input is not an `I`, or `work` failed.
pub fn run<I: DeserializeOwned, O: Serialize>(
    input: &[u8],
    work: impl FnOnce(I) -> Result<O, String>,
) -> Result<Vec<u8>, String> {
    let input = serde_json::from_slice(input).map_err(|e| format!("the job's input: {e}"))?;
    let output = work(input)?;
    serde_json::to_vec(&output).map_err(|e| format!("the job's output: {e}"))
}

/// Called from inside a job: progress from 0 to 1 that [`Job::progress`]
/// shows after the next poll.
///
/// # Errors
/// See `jobs.report`.
pub fn report(fraction: f32, message: &str) -> Result<(), crate::Error> {
    api::report(fraction, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_decodes_the_input_and_encodes_the_output() {
        let output = run(br#""four""#, |text: String| Ok(text.len())).unwrap();
        assert_eq!(output, b"4");
        assert_eq!(
            run(b"4", |n: u32| -> Result<u32, String> {
                Err(format!("no {n}"))
            })
            .unwrap_err(),
            "no 4"
        );
        assert!(
            run(b"not json", |n: u32| Ok(n))
                .unwrap_err()
                .starts_with("the job's input: ")
        );
    }
}
