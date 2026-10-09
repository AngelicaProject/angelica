//! A test plugin that tries to break the host's boundaries, one per button:
//! the host's tests click a button and check that the host faults the
//! plugin or refuses the operation while the game and other plugins go on.
use angelica_sdk::{
    Plugin,
    api::{excel_text, http, jobs, state, storage},
    commands::{self, Command},
    ui::{Event, Window},
};
use std::hint::black_box;

#[derive(Default)]
struct Hostile {
    /// What to do in the next `tick`.
    next: Option<String>,
    huge: bool,
    jobs: Vec<u64>,
    /// What the last refused operation reported, as `kind: message`.
    report: String,
}

/// Recurses until the stack runs out.
#[allow(unconditional_recursion)]
fn deeper(depth: u64) -> u64 {
    black_box(deeper(black_box(depth + 1))) + 1
}

/// Burns guest instructions: tens of millions of fuel units, well under the
/// default frame's fuel once, over it several times.
fn burn() {
    let mut value = 0u64;
    for step in 0..4_000_000u64 {
        value = black_box(value.wrapping_mul(31).wrapping_add(step));
    }
    black_box(value);
}

fn outcome<T>(result: Result<T, angelica_sdk::Error>) -> String {
    match result {
        Ok(_) => "ok".into(),
        Err(error) => format!("{:?}: {}", error.kind, error.message),
    }
}

impl Hostile {
    fn click(&mut self, id: &str) {
        match id {
            "spin" | "host-loop" | "memory" | "stack" | "panic" => self.next = Some(id.into()),
            "burn" => burn(),
            "huge-ui" => self.huge = true,
            "jobs" => {
                for _ in 0..4 {
                    match jobs::start("quick", b"") {
                        Ok(job) => self.jobs.push(job),
                        Err(error) => self.report = outcome::<()>(Err(error)),
                    }
                }
            }
            "fifth" => self.report = outcome(jobs::start("quick", b"")),
            "start-cancel" => {
                self.report = outcome(
                    jobs::start("quick", b"").and_then(|job| jobs::cancel(job).map(|()| job)),
                );
            }
            "paths" => {
                self.report = [
                    "../escape",
                    "a/../../b",
                    "CONIN$",
                    "C:/x",
                    "a\\b",
                    "nul.txt",
                ]
                .iter()
                .map(|path| outcome(storage::write(path, b"x")))
                .collect::<Vec<_>>()
                .join("; ");
            }
            "table" => self.report = outcome(excel_text::activate("big", "big.table", 0)),
            "replace" => {
                self.report = outcome(angelica_sdk::api::file_replacements::replace(
                    "common/font/font1.tex",
                    "big.table",
                ));
            }
            "http" => {
                self.report = outcome(http::get(&http::Request {
                    url: "https://example.com/".into(),
                    headers: Vec::new(),
                }));
            }
            "job-print" => match jobs::start("print", b"") {
                Ok(job) => self.jobs.push(job),
                Err(error) => self.report = outcome::<()>(Err(error)),
            },
            "poll" => {
                if let Some(&job) = self.jobs.last() {
                    match jobs::poll(job) {
                        Ok(jobs::State::Failed(reason)) => self.report = reason,
                        Ok(jobs::State::Finished(_)) => self.report = "finished".into(),
                        Ok(jobs::State::Running(_)) => self.report = "running".into(),
                        Err(error) => self.report = outcome::<()>(Err(error)),
                    }
                }
            }
            "permissions" => {
                self.report = [
                    outcome(state::read("self", &["level".into()])),
                    outcome(commands::define(&[Command::new("hostile", "Takes over")])),
                ]
                .join("; ");
            }
            _ => {}
        }
    }
}

impl Plugin for Hostile {
    fn start() -> Result<Self, String> {
        Ok(Self::default())
    }

    fn tick(&mut self, _elapsed: f64) -> Result<(), String> {
        match self.next.take().as_deref() {
            Some("spin") => loop {
                black_box(());
            },
            Some("host-loop") => loop {
                let _ = black_box(storage::write("busy", b"x"));
            },
            Some("memory") => {
                let block = vec![1u8; 1 << 30];
                black_box(block);
            }
            Some("stack") => {
                black_box(deeper(0));
            }
            Some("panic") => panic!("the plugin panics"),
            _ => {}
        }
        Ok(())
    }

    fn ui(&mut self) -> Vec<Window> {
        let mut window = Window::new("main", "Hostile").text(&self.report);
        if self.huge {
            for index in 0..5000 {
                window = window.text(index.to_string());
            }
        }
        vec![window]
    }

    fn ui_event(&mut self, event: Event) {
        if let Event::Clicked(target) = event {
            self.click(&target.id);
        }
    }

    fn run_job(name: &str, _input: Vec<u8>) -> Result<Vec<u8>, String> {
        match name {
            "quick" => Ok(vec![0; 1024]),
            "print" => angelica_sdk::chat::print("from a job")
                .map(|()| Vec::new())
                .map_err(|e| format!("{:?}: {}", e.kind, e.message)),
            _ => Err(format!("no job {name}")),
        }
    }
}

angelica_sdk::plugin!(Hostile);
