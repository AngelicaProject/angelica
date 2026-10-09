//! An example plugin that uses every interface of the plugin API: a window
//! with each kind of widget, files in its storage, a job, the game's files
//! and sheets, a text table, letter case, a replaced game file, chat
//! commands that print in the chat log, HTTPS requests from a job, the
//! player's character from the game's state, and the chat log's messages
//! as events. The host's integration tests drive
//! it through its window and its commands.
use angelica_sdk::{
    Plugin,
    api::{excel_text, file_replacements, game_files, runtime, storage as files, text_case},
    chat::{Message, Tone as ChatTone},
    commands::{Command, Invocation, Parameter},
    events::{self, EventKind, GameEvent},
    excel, http,
    jobs::{self, Job, Poll},
    state, storage,
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
    /// The running job, whose output is a line for the window.
    job: Option<Job<String>>,
    progress: f32,
    message: String,
    /// The last message the chat log received, as `sender: text`.
    heard: Option<String>,
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
                if let Err(error) = storage::save("clicks.json", &self.clicks) {
                    self.say(format!("cannot save: {}", error.message));
                }
            }
            "count" | "fail" | "fetch" => match jobs::start(id, &"hello job") {
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
                let text = excel::row("Item", excel::ENGLISH, &["name"], 1)
                    .map_err(|e| e.message)
                    .and_then(|row| row.ok_or_else(|| "no row 1".to_owned()))
                    .map(|values| {
                        values.first().and_then(excel::Value::text).map_or_else(
                            || "not text".to_owned(),
                            |bytes| String::from_utf8_lossy(bytes).into_owned(),
                        )
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
                        files::write("font1.tex", &texture)
                    })
                    .and_then(|()| {
                        file_replacements::replace("common/font/font1.tex", "font1.tex")
                    });
                match result {
                    Ok(()) => self.say("texture replaced"),
                    Err(error) => self.say(format!("texture refused: {}", error.message)),
                }
            }
            "where" => match who() {
                Ok(text) => self.say(text),
                Err(error) => self.say(format!("cannot read the character: {error}")),
            },
            "heard" => {
                let heard = self.heard.clone();
                self.say(heard.map_or_else(
                    || "nothing heard yet".into(),
                    |line| format!("heard {line}"),
                ));
            }
            "statuses" => match statuses() {
                Ok(text) => self.say(text),
                Err(error) => self.say(format!("cannot read the statuses: {error}")),
            },
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

/// `/hello` and its subcommands.
fn commands() -> Command {
    Command::new("hello", "Greets you")
        .subcommand(
            Command::new("greet", "Greets someone")
                .alias("g")
                .parameter(Parameter::word("name", "Who to greet"))
                .parameter(Parameter::integer("times", "How often", 1, 5).optional()),
        )
        .subcommand(
            Command::new("print", "Prints words in a tone")
                .parameter(Parameter::choice(
                    "tone",
                    "How it looks",
                    &TONES.map(|(word, _)| word),
                ))
                .parameter(Parameter::text("words", "What to print")),
        )
        .subcommand(
            Command::new("caps", "Shows the name in the window in capitals")
                .parameter(Parameter::switch("on", "Whether to use capitals")),
        )
        .subcommand(Command::new("colors", "Shows every tone"))
        .subcommand(Command::new("fail", "Always fails"))
}

const TONES: [(&str, ChatTone); 6] = [
    ("normal", ChatTone::Normal),
    ("accent", ChatTone::Accent),
    ("muted", ChatTone::Muted),
    ("good", ChatTone::Good),
    ("warning", ChatTone::Warning),
    ("bad", ChatTone::Bad),
];

/// The player's character and zone from the game's state.
fn who() -> Result<String, String> {
    let Some(values) = state::read_one(
        "self",
        &[
            "name",
            "class_job",
            "level",
            "health",
            "max_health",
            "mana",
            "max_mana",
            "position_x",
            "position_y",
            "position_z",
        ],
    )?
    else {
        return Ok("no character yet".into());
    };
    let name = values[0]
        .as_ref()
        .and_then(state::Value::text)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();
    let number = |index: usize| {
        values[index]
            .as_ref()
            .and_then(state::Value::unsigned)
            .unwrap_or(0)
    };
    let float = |index: usize| {
        values[index]
            .as_ref()
            .and_then(state::Value::float)
            .unwrap_or(0.0)
    };
    let zone = state::read_one("self.zone", &["territory_type"])?
        .and_then(|values| values[0].as_ref().and_then(state::Value::unsigned))
        .unwrap_or(0);
    Ok(format!(
        "{name} is class {} at level {}, HP {}/{}, MP {}/{}, at ({:.1}, {:.1}, {:.1}) in zone {zone}",
        number(1),
        number(2),
        number(3),
        number(4),
        number(5),
        number(6),
        float(7),
        float(8),
        float(9)
    ))
}

/// The statuses on the player's character, with the seconds they last.
fn statuses() -> Result<String, String> {
    let rows = state::read("self.statuses", &["status_id", "remaining_time"])?;
    if rows.is_empty() {
        return Ok("no statuses".into());
    }
    let listed: Vec<String> = rows
        .iter()
        .map(|row| {
            let id = row[0]
                .as_ref()
                .and_then(state::Value::unsigned)
                .unwrap_or(0);
            match row[1].as_ref().and_then(state::Value::float) {
                Some(time) if time > 0.0 => format!("{id} ({time:.0}s)"),
                _ => id.to_string(),
            }
        })
        .collect();
    Ok(format!("statuses: {}", listed.join(", ")))
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
    Ok(files::write("example.table", &bytes)?)
}

impl Plugin for Hello {
    fn start() -> Result<Self, String> {
        let clicks = storage::load("clicks.json")?.unwrap_or(0);
        angelica_sdk::info(&format!(
            "started for game {}, with the game: {}",
            runtime::game_version(),
            runtime::started_with_game()
        ));
        let mut hello = Self {
            open: false,
            clicks,
            name: String::new(),
            shout: false,
            color: None,
            job: None,
            progress: 0.0,
            message: String::new(),
            heard: None,
        };
        if let Err(error) = events::subscribe(&[EventKind::ChatMessage]) {
            hello.say(format!("events refused: {}", error.message));
        }
        if let Err(error) = angelica_sdk::commands::define(&[commands()]) {
            hello.say(format!("commands refused: {}", error.message));
        }
        Ok(hello)
    }

    fn tick(&mut self, _elapsed: f64) -> Result<(), String> {
        if let Some(job) = &mut self.job {
            match job.poll() {
                Poll::Running => self.progress = job.progress().fraction,
                Poll::Done(result) => {
                    self.job = None;
                    self.progress = 1.0;
                    match result {
                        Ok(line) => self.say(format!("job finished: {line}")),
                        Err(reason) => self.say(format!("job failed: {reason}")),
                    }
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
                                .button_enabled("fetch", "Fetch example.com", self.job.is_none())
                        })
                        .progress(self.progress, "")
                })
                .section("game", "Game", false, |section| {
                    section.row(|row| {
                        row.button("table", "Activate the table")
                            .button("status", "Table status")
                            .button("letters", "Add letters")
                            .button("read", "Read game data")
                            .button("where", "Who am I")
                            .button("statuses", "My statuses")
                            .button("heard", "Last chat message")
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

    fn game_event(&mut self, event: GameEvent) {
        if let GameEvent::ChatMessage(message) = event {
            self.heard = Some(format!(
                "{} ({}): {}",
                String::from_utf8_lossy(&message.sender),
                message.kind,
                String::from_utf8_lossy(&message.message)
            ));
        }
    }

    fn command(&mut self, invocation: Invocation) -> Result<(), String> {
        let message = if invocation.is(&["hello"]) {
            Message::new()
                .text("Hello! Try ")
                .accent("/hello help")
                .text(".")
        } else if invocation.is(&["hello", "greet"]) {
            let name = invocation.word("name").unwrap_or_default();
            let mut message = Message::new();
            for time in 0..invocation.integer("times").unwrap_or(1) {
                if time > 0 {
                    message = message.line();
                }
                message = message.text("Hello, ").accent(&name).text("!");
            }
            message
        } else if invocation.is(&["hello", "print"]) {
            let tone = invocation.word("tone").unwrap_or_default();
            let tone = TONES
                .iter()
                .find(|(word, _)| *word == tone)
                .map_or(ChatTone::Normal, |(_, tone)| *tone);
            Message::new().span(invocation.word("words").unwrap_or_default(), tone)
        } else if invocation.is(&["hello", "caps"]) {
            self.shout = invocation.switch("on").unwrap_or(false);
            Message::new()
                .text("Capitals are ")
                .accent(if self.shout { "on" } else { "off" })
        } else if invocation.is(&["hello", "colors"]) {
            TONES
                .iter()
                .fold(Message::new(), |message, (word, tone)| {
                    message.span(format!("{word} "), *tone)
                })
                .span("italic", ChatTone::Normal)
                .italic()
        } else if invocation.is(&["hello", "fail"]) {
            return Err("this command always fails".into());
        } else {
            return Err(format!("no command {}", invocation.path.join(" ")));
        };
        if let Err(error) = message.print() {
            self.say(format!("cannot print: {}", error.message));
        }
        Ok(())
    }

    fn run_job(name: &str, input: Vec<u8>) -> Result<Vec<u8>, String> {
        match name {
            "count" => jobs::run(&input, |text: String| {
                // The standard library's hashing and clocks work in a plugin.
                let started = std::time::Instant::now();
                let distinct: std::collections::HashSet<u8> = text.bytes().collect();
                angelica_sdk::info(&format!(
                    "{} distinct bytes in {:?}",
                    distinct.len(),
                    started.elapsed()
                ));
                for step in 0..=10u8 {
                    jobs::report(f32::from(step) / 10.0, "counting")?;
                }
                files::append("jobs.log", text.as_bytes())?;
                Ok(format!("{} bytes", text.len()))
            }),
            "fail" => jobs::run(&input, |_: String| -> Result<String, String> {
                Err("this job always fails".into())
            }),
            "fetch" => jobs::run(&input, |_: String| {
                let request =
                    http::Request::new("https://example.com/").header("accept", "text/html");
                let page = request.get()?;
                let (head, saved) = request.download("example.html")?;
                Ok(format!(
                    "{} with {} bytes; saved {} bytes with {}",
                    page.head.status,
                    page.body.len(),
                    saved,
                    head.status
                ))
            }),
            _ => Err(format!("no job {name}")),
        }
    }
}

angelica_sdk::plugin!(Hello);
