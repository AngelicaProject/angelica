//! An example plugin that uses every interface of the plugin API: a window
//! with each kind of widget, files in its storage, a job, the game's files
//! and sheets, a text table, letter case, and a replaced game file. The host's integration tests drive it through its window.
use angelica_sdk::{
    Plugin,
    api::{excel, excel_text, file_replacements, game_files, jobs, runtime, storage, text_case},
    text::{
        guard,
        table::{Column, Variant, Writer},
    },
    ui::{Event, Tone, Window},
};

/// The domain of the example table's guards.
const DOMAIN: &[u8] = b"HELLO";

struct Hello {
    open: bool,
    clicks: u32,
    name: String,
    shout: bool,
    color: Option<u32>,
    job: Option<u64>,
    progress: f32,
    message: String,
}

impl Hello {
    fn say(&mut self, message: impl Into<String>) {
        self.message = message.into();
        angelica_sdk::info(&self.message);
    }

    fn click(&mut self, id: &str) {
        match id {
            "click" => {
                self.clicks += 1;
                if let Err(error) = storage::write("clicks.txt", self.clicks.to_string().as_bytes())
                {
                    self.say(format!("cannot save: {}", error.message));
                }
            }
            "count" | "fail" => match jobs::start(id, b"hello job") {
                Ok(job) => self.job = Some(job),
                Err(error) => self.say(format!("job refused: {}", error.message)),
            },
            "table" => {
                let result = write_table().and_then(|()| {
                    excel_text::activate("example", "example.table", 0).map_err(|e| e.message)
                });
                match result {
                    Ok(info) => self.say(format!(
                        "table active: {} sheets, {} cells",
                        info.sheets, info.cells
                    )),
                    Err(error) => self.say(format!("table refused: {error}")),
                }
            }
            "read" => {
                let text = excel::row("Item", 2, excel::RowKey { row: 1, subrow: 0 })
                    .map_err(|e| e.message)
                    .and_then(|row| row.ok_or_else(|| "no row 1".to_owned()))
                    .map(|row| match row.values.first() {
                        Some(excel::Value::Text(bytes)) => {
                            String::from_utf8_lossy(bytes).into_owned()
                        }
                        _ => "not text".to_owned(),
                    });
                let font = game_files::read("common/font/AXIS_12.fdt").map(|bytes| bytes.len());
                match (text, font) {
                    (Ok(text), Ok(size)) => {
                        self.say(format!("Item 1 is {text}; the font file has {size} bytes"));
                    }
                    (Err(error), _) => self.say(format!("cannot read Item: {error}")),
                    (_, Err(error)) => self.say(format!("cannot read the font: {}", error.message)),
                }
            }
            "texture" => {
                // The font texture with its last pixel changed.
                let result = game_files::read("common/font/font1.tex")
                    .and_then(|mut texture| {
                        if let Some(last) = texture.last_mut() {
                            *last ^= 0xFF;
                        }
                        storage::write("font1.tex", &texture)
                    })
                    .and_then(|()| {
                        file_replacements::replace("common/font/font1.tex", "font1.tex")
                    });
                match result {
                    Ok(()) => self.say("texture replaced"),
                    Err(error) => self.say(format!("texture refused: {}", error.message)),
                }
            }
            "status" => match excel_text::status("example") {
                Ok(status) => self.say(format!(
                    "table: {} rows changed, {} cells, {} mismatches",
                    status.rows_changed, status.cells_replaced, status.guard_mismatches
                )),
                Err(error) => self.say(format!("no status: {}", error.message)),
            },
            "letters" => {
                let letters: Vec<_> = ('а'..='я')
                    .zip('А'..='Я')
                    .map(|(lower, upper)| text_case::Letter { lower, upper })
                    .collect();
                match text_case::set_letters(&letters) {
                    Ok(()) => self.say("letters added"),
                    Err(error) => self.say(format!("letters refused: {}", error.message)),
                }
            }
            _ => {}
        }
    }
}

/// A table that replaces the text of Item row 1, column 0, while it is
/// "Potion".
fn write_table() -> Result<(), String> {
    let mut writer = Writer::new(2, DOMAIN).map_err(|e| e.to_string())?;
    writer
        .sheet(
            "Item",
            Variant::Rows,
            &[Column {
                index: 0,
                offset: 0,
            }],
        )
        .map_err(|e| e.to_string())?;
    writer
        .cell(
            "Item",
            1,
            0,
            0,
            guard(DOMAIN, b"Potion"),
            "Зелье".as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    let bytes = writer.finish().map_err(|e| e.to_string())?;
    storage::write("example.table", &bytes).map_err(|e| e.message)
}

impl Plugin for Hello {
    fn start() -> Result<Self, String> {
        let clicks = storage::read("clicks.txt")
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        angelica_sdk::info(&format!(
            "started for game {}, with the game: {}",
            runtime::game_version(),
            runtime::started_with_game()
        ));
        Ok(Self {
            open: false,
            clicks,
            name: String::new(),
            shout: false,
            color: None,
            job: None,
            progress: 0.0,
            message: String::new(),
        })
    }

    fn tick(&mut self, _elapsed: f64) -> Result<(), String> {
        if let Some(job) = self.job {
            match jobs::poll(job).map_err(|e| e.message)? {
                jobs::State::Running(progress) => self.progress = progress.fraction,
                jobs::State::Finished(output) => {
                    self.job = None;
                    self.progress = 1.0;
                    self.say(format!(
                        "job finished: {}",
                        String::from_utf8_lossy(&output)
                    ));
                }
                jobs::State::Failed(reason) => {
                    self.job = None;
                    self.say(format!("job failed: {reason}"));
                }
            }
        }
        Ok(())
    }

    fn ui(&mut self) -> Vec<Window> {
        if !self.open {
            return Vec::new();
        }
        let greeting = if self.shout {
            self.name.to_uppercase()
        } else {
            self.name.clone()
        };
        let colors = ["Red", "Green", "Blue"].map(String::from).to_vec();
        vec![
            Window::new("main", "Hello")
                .heading(format!("Hello, {greeting}"))
                .input("name", "Name", &self.name, "your name")
                .checkbox("shout", "Shout", self.shout)
                .choice("color", "Color", colors, self.color)
                .row(|row| {
                    row.button("click", "Click")
                        .text(format!("{} clicks", self.clicks))
                })
                .section("job", "Job", true, |section| {
                    section
                        .row(|row| {
                            row.button_enabled("count", "Run a job", self.job.is_none())
                                .button_enabled("fail", "Run a failing job", self.job.is_none())
                        })
                        .progress(self.progress, "")
                })
                .section("game", "Game", false, |section| {
                    section.row(|row| {
                        row.button("table", "Activate the table")
                            .button("status", "Table status")
                            .button("letters", "Add letters")
                            .button("read", "Read game data")
                            .button("texture", "Replace a texture")
                    })
                })
                .separator()
                .toned(&self.message, Tone::Muted),
        ]
    }

    fn ui_event(&mut self, event: Event) {
        match event {
            Event::OpenRequested => self.open = true,
            Event::Closed(_) => self.open = false,
            Event::Clicked(target) => self.click(&target.id),
            Event::Toggled(toggled) => self.shout = toggled.checked,
            Event::Edited(edited) => self.name = edited.value,
            Event::Chosen(chosen) => self.color = Some(chosen.index),
        }
    }

    fn run_job(name: &str, input: Vec<u8>) -> Result<Vec<u8>, String> {
        match name {
            "count" => {
                // The standard library's hashing and clocks work in a plugin.
                let started = std::time::Instant::now();
                let distinct: std::collections::HashSet<u8> = input.iter().copied().collect();
                angelica_sdk::info(&format!(
                    "{} distinct bytes in {:?}",
                    distinct.len(),
                    started.elapsed()
                ));
                for step in 0..=10u8 {
                    jobs::report(f32::from(step) / 10.0, "counting").map_err(|e| e.message)?;
                }
                storage::append("jobs.log", &input).map_err(|e| e.message)?;
                Ok(format!("{} bytes", input.len()).into_bytes())
            }
            "fail" => Err("this job always fails".into()),
            _ => Err(format!("no job {name}")),
        }
    }
}

angelica_sdk::plugin!(Hello);
