//! An example plugin that uses every interface of the plugin API: a window
//! with each kind of widget, files in its storage, a job, the game's files
//! and sheets, a text table, letter case, a replaced game file, chat
//! commands that print in the chat log, HTTPS requests from a job, the
//! player's character read through Lens, the actions the player uses
//! watched through Lens, the chat log's messages watched through Lens,
//! and the game's own windows changed through Lens. The host's integration
//! tests drive it through its window and its commands.
use angelica_sdk::{
    Plugin,
    api::{file_replacements, game_files, runtime, storage as files},
    chat::{Message, Tone as ChatTone},
    commands::{Command, Invocation, Parameter},
    events::GameEvent,
    excel, http,
    jobs::{self, Job, Poll},
    lens, storage,
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
    /// The watch of the chat log, or why there is none.
    chat: Result<lens::Watch, String>,
    /// The watch of the actions the player uses, or why there is none.
    used: Result<lens::Watch, String>,
    /// The last action the player used.
    last_used: Option<String>,
    /// The rule hiding system messages while the player wants them hidden.
    quiet: Option<lens::Rule>,
    /// The example table's rules for the game's sheet text.
    table: Option<lens::RuleFile>,
    /// The rules that give the game's case conversion Cyrillic letters.
    letters: Vec<lens::Rule>,
    /// The selected tab of the widgets section.
    tab: u32,
    volume: f64,
    bar: ExperienceBar,
}

/// Whether the game's experience bar is shown: the element the game map
/// names `window` in the window `_Exp`.
const EXPERIENCE_BAR: &str =
    "windows.unit_manager.windows[name = \"_Exp\"].layout.elements[name = window].visible";

/// The text above the experience bar.
const EXPERIENCE_TEXT: &str =
    "windows.unit_manager.windows[name = \"_Exp\"].layout.elements[name = text].text";

/// What the example does to the game's experience bar.
#[derive(Default)]
struct ExperienceBar {
    /// Whether the bar is hidden.
    hidden: bool,
    /// Whether the clicks are shown after the bar's text.
    clicks: bool,
}

impl Hello {
    /// Greets the echo server once the connection opens, shows its answer,
    /// and closes.
    fn socket_event(&mut self, event: &GameEvent) {
        use angelica_sdk::api::websocket::{self, Data};
        match event {
            GameEvent::SocketOpened(opened) => {
                if let Err(error) = websocket::send(opened.socket, &Data::Text("hello".into())) {
                    self.say(format!("cannot greet: {}", error.message));
                }
            }
            GameEvent::SocketMessage(message) => {
                if let Data::Text(text) = &message.data {
                    self.say(format!("socket said {text}"));
                }
                let _ = websocket::close(message.socket);
            }
            GameEvent::SocketClosed(closed) if closed.code != 1000 => {
                self.say(format!("socket closed: {}", closed.reason));
            }
            _ => {}
        }
    }

    fn say(&mut self, message: impl Into<String>) {
        self.message = message.into();
        angelica_sdk::info(&self.message);
    }

    /// Writes the example table and adds its rules for the game's sheet
    /// text.
    fn add_table(&mut self) {
        let added = write_table().and_then(|()| {
            lens::RuleFile::new("sheet_text", "example.table").map_err(|e| e.message)
        });
        match added {
            Ok(table) => {
                let text = format!(
                    "table active: {} sheets, {} cells",
                    table.info().parts,
                    table.info().rules
                );
                self.table = Some(table);
                self.say(text);
            }
            Err(error) => self.say(format!("table refused: {error}")),
        }
    }

    /// Shows the clicks in place of the game's experience text, or puts the
    /// game's text back. The host keeps the text while the game writes its
    /// own again, and whenever the bar is built again.
    fn show_clicks(&mut self) {
        let result = if self.bar.clicks {
            lens::set(EXPERIENCE_TEXT, format!("{} clicks", self.clicks)).map(drop)
        } else {
            lens::restore(EXPERIENCE_TEXT)
        };
        if let Err(error) = result {
            self.say(format!("text refused: {}", error.message));
        }
    }

    /// Hides the game's experience bar, or shows it again.
    fn hide_bar(&mut self) {
        let result = if self.bar.hidden {
            lens::restore(EXPERIENCE_BAR)
        } else {
            lens::set(EXPERIENCE_BAR, false).map(drop)
        };
        match result {
            Ok(()) => {
                self.bar.hidden = !self.bar.hidden;
                self.say(if self.bar.hidden {
                    "experience bar hidden"
                } else {
                    "experience bar shown"
                });
            }
            Err(error) => self.say(format!("hiding refused: {}", error.message)),
        }
    }

    /// Shows the clicks in the game's experience text, or stops.
    fn label_clicks(&mut self) {
        self.bar.clicks = !self.bar.clicks;
        self.say(if self.bar.clicks {
            "clicks in the experience text"
        } else {
            "clicks removed from the experience text"
        });
        self.show_clicks();
    }

    fn click(&mut self, id: &str) {
        if let Some(text) = around_lens(id) {
            self.say(text);
            return;
        }
        match id {
            "click" => {
                self.clicks += 1;
                if let Err(error) = storage::save("clicks.json", &self.clicks) {
                    self.say(format!("cannot save: {}", error.message));
                }
                if self.bar.clicks {
                    self.show_clicks();
                }
            }
            "hide" => self.hide_bar(),
            "label" => self.label_clicks(),
            "count" | "fail" | "fetch" => match jobs::start(id, &"hello job") {
                Ok(job) => self.job = Some(job),
                Err(error) => self.say(format!("job refused: {}", error.message)),
            },
            "table" => self.add_table(),
            "read" => {
                let text = read_item_and_font();
                self.say(text);
            }
            "texture" => {
                let text = replace_texture();
                self.say(text);
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
            "used" => {
                let text = self.used_text();
                self.say(text);
            }
            "yes" => self.say(answer_yes()),
            "quiet" => {
                if self.quiet.take().is_some() {
                    self.say("system messages shown");
                } else {
                    // Log kind 57 is the game's system messages.
                    match lens::Rule::new("chat_message_added[kind = 57]", "hide") {
                        Ok(rule) => {
                            self.quiet = Some(rule);
                            self.say("system messages hidden");
                        }
                        Err(error) => self.say(format!("cannot hide: {}", error.message)),
                    }
                }
            }
            "zoom" => match lens::set("camera.camera.max_distance", 40.0f32) {
                Ok(count) => self.say(format!("zoom limit 40 on {count} cameras")),
                Err(error) => self.say(format!("cannot zoom: {}", error.message)),
            },
            "sprint" => {
                // Sprint is general action 4, used as its hotbar slot does.
                let sprint = [
                    ("kind", lens::named("general_action")),
                    ("action", 4u32.into()),
                ];
                match lens::act("use_action", &sprint) {
                    Ok(()) => self.say("sprinting"),
                    Err(error) => self.say(format!("cannot sprint: {}", error.message)),
                }
            }
            "statuses" => match statuses() {
                Ok(text) => self.say(text),
                Err(error) => self.say(format!("cannot read the statuses: {error}")),
            },
            "status" => match self.table.as_ref().map(lens::RuleFile::decisions) {
                Some(Ok(status)) => self.say(format!(
                    "table: {} cells, {} stale",
                    status.taken, status.stale
                )),
                Some(Err(error)) => self.say(format!("no status: {}", error.message)),
                None => self.say("no status: the table is not active"),
            },
            "letters" => match cyrillic_letters() {
                Ok(rules) => {
                    self.letters = rules;
                    self.say("letters added");
                }
                Err(error) => self.say(format!("letters refused: {}", error.message)),
            },
            _ => {}
        }
    }
}

/// Rules that give the game's case conversion the Cyrillic letters а to я,
/// both ways.
fn cyrillic_letters() -> Result<Vec<lens::Rule>, angelica_sdk::Error> {
    let mut rules = Vec::new();
    for (small, capital) in ('а'..='я').zip('А'..='Я') {
        rules.push(lens::Rule::with(
            &format!("letter_upper[letter = \"{small}\"]"),
            "becomes",
            capital.to_string(),
        )?);
        rules.push(lens::Rule::with(
            &format!("letter_lower[letter = \"{capital}\"]"),
            "becomes",
            small.to_string(),
        )?);
    }
    Ok(rules)
}

/// Item 1's name and the size of a font file, read from the game's files.
fn read_item_and_font() -> String {
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
        (Ok(text), Ok(size)) => format!("Item 1 is {text}; the font file has {size} bytes"),
        (Err(error), _) => format!("cannot read Item: {error}"),
        (_, Err(error)) => format!("cannot read the font: {}", error.message),
    }
}

/// Replaces the font texture with a copy whose last pixel is changed.
fn replace_texture() -> String {
    let result = game_files::read("common/font/font1.tex")
        .and_then(|mut texture| {
            if let Some(last) = texture.last_mut() {
                *last ^= 0xFF;
            }
            files::write("font1.tex", &texture)
        })
        .and_then(|()| file_replacements::replace("common/font/font1.tex", "font1.tex"));
    match result {
        Ok(()) => "texture replaced".to_owned(),
        Err(error) => format!("texture refused: {}", error.message),
    }
}

/// What the buttons of the host's services beside Lens say: drawing,
/// messages, the clipboard, notifications, sound, and sockets.
fn around_lens(id: &str) -> Option<String> {
    match id {
        "mark" => Some(mark_where_i_am()),
        // The example sends to itself; a plugin lists the others it talks to.
        "ping" => Some(said(
            angelica_sdk::messages::send("hello", "ping", b"ping"),
            "ping sent",
            "cannot send",
        )),
        "copy" => Some(said(
            angelica_sdk::clipboard::copy("Hello from Angelica"),
            "copied",
            "cannot copy",
        )),
        "notify" => Some(said(
            angelica_sdk::notifications::show("Hello from Angelica"),
            "notified",
            "cannot notify",
        )),
        "cache" => Some(said(
            files::write("cache/hello.txt", b"fetched again when gone"),
            "cached",
            "cannot cache",
        )),
        "socket" => Some(said(
            angelica_sdk::websocket::Socket::open("wss://example.com/echo"),
            "connecting",
            "cannot connect",
        )),
        "beep" => Some(said(
            files::write("beep.wav", &beep())
                .and_then(|()| angelica_sdk::sound::play("beep.wav", 0.5)),
            "beeped",
            "cannot beep",
        )),
        _ => None,
    }
}

/// A WAV file of a fifth of a second of A4, 16-bit mono at 22 050 Hz.
fn beep() -> Vec<u8> {
    const RATE: u32 = 22_050;
    let samples: Vec<u8> = (0..RATE / 5)
        .flat_map(|index| {
            #[allow(clippy::cast_precision_loss)] // small sample indexes
            let phase = index as f32 * 440.0 * std::f32::consts::TAU / RATE as f32;
            #[allow(clippy::cast_possible_truncation)] // within i16
            let value = (phase.sin() * 12_000.0) as i16;
            value.to_le_bytes()
        })
        .collect();
    let length = u32::try_from(samples.len()).unwrap_or_default();
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + length).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    // Format size, PCM and one channel, rate, bytes a second, and two
    // bytes a frame of 16 bits.
    for field in [16u32, 1 | (1 << 16), RATE, RATE * 2, 2 | (16 << 16)] {
        wav.extend_from_slice(&field.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&length.to_le_bytes());
    wav.extend_from_slice(&samples);
    wav
}

/// The line shown for an operation's result: `done`, or `failed` with the
/// reason.
fn said<T>(result: Result<T, angelica_sdk::Error>, done: &str, failed: &str) -> String {
    match result {
        Ok(_) => done.to_owned(),
        Err(error) => format!("{failed}: {}", error.message),
    }
}

/// Draws a circle around where the character stands, with a label.
fn mark_where_i_am() -> String {
    let place = lens::Query::prepare("player { position_x, position_y, position_z }")
        .and_then(|query| query.run().map(|rows| (query, rows)))
        .map(|(query, rows)| {
            rows.first().map(|row| {
                ["position_x", "position_y", "position_z"].map(|name| {
                    let value = query.get(row, name).and_then(lens::Value::float);
                    #[allow(clippy::cast_possible_truncation)] // a position in yalms
                    let value = value.unwrap_or_default() as f32;
                    value
                })
            })
        });
    match place {
        Ok(Some([x, y, z])) => match angelica_sdk::draw::set(&[
            angelica_sdk::draw::circle([x, y, z], 3.0, 0xE873_5AFF),
            angelica_sdk::draw::text([x, y + 2.5, z], "here", 0xFFFF_FFFF),
        ]) {
            Ok(()) => format!("marked {x:.1}, {z:.1}"),
            Err(error) => format!("cannot draw: {}", error.message),
        },
        Ok(None) => "not in the world".to_owned(),
        Err(error) => format!("cannot read the position: {}", error.message),
    }
}

/// Answers the open yes or no dialog with yes, saying what it asked.
fn answer_yes() -> String {
    let question = lens::Query::prepare(
        "windows.unit_manager.windows[name = \"SelectYesno\"].values[0] { text }",
    )
    .and_then(|query| query.run().map(|rows| (query, rows)))
    .map(|(query, rows)| {
        rows.first()
            .and_then(|row| query.get(row, "text"))
            .and_then(lens::Value::plain)
            .unwrap_or_default()
    });
    match (
        question,
        lens::act("answer_yes_no", &[("answer", lens::named("yes"))]),
    ) {
        (Ok(question), Ok(())) => format!("said yes to {question}"),
        (Err(error), _) | (_, Err(error)) => format!("cannot say yes: {}", error.message),
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

/// The player's character and zone, read through Lens.
fn who() -> Result<String, String> {
    let character = lens::Query::prepare(
        "player { name, class_job: data.class_job, level: data.level, health: data.health,          max_health: data.max_health, mana: data.mana, max_mana: data.max_mana,          position_x, position_y, position_z }",
    )?;
    let rows = character.run()?;
    let Some(row) = rows.first() else {
        return Ok("no character yet".into());
    };
    let name = character
        .get(row, "name")
        .and_then(lens::Value::plain)
        .unwrap_or_default();
    let number = |name: &str| {
        character
            .get(row, name)
            .and_then(lens::Value::unsigned)
            .unwrap_or(0)
    };
    let float = |name: &str| {
        character
            .get(row, name)
            .and_then(lens::Value::float)
            .unwrap_or(0.0)
    };
    let zone = lens::Query::prepare("game.current_territory_type")?;
    let zone = zone
        .run()?
        .first()
        .and_then(|row| row.values[0].as_ref())
        .and_then(lens::Value::unsigned)
        .unwrap_or(0);
    Ok(format!(
        "{name} is class {} at level {}, HP {}/{}, MP {}/{}, at ({:.1}, {:.1}, {:.1}) in zone {zone}",
        number("class_job"),
        number("level"),
        number("health"),
        number("max_health"),
        number("mana"),
        number("max_mana"),
        float("position_x"),
        float("position_y"),
        float("position_z")
    ))
}

/// The statuses on the player's character, with the seconds they last.
fn statuses() -> Result<String, String> {
    let statuses =
        lens::Query::prepare("player.status_manager.statuses { status_id, remaining_time }")?;
    let rows = statuses.run()?;
    if rows.is_empty() {
        return Ok("no statuses".into());
    }
    let listed: Vec<String> = rows
        .iter()
        .map(|row| {
            let id = row.values[0]
                .as_ref()
                .and_then(lens::Value::unsigned)
                .unwrap_or(0);
            match row.values[1].as_ref().and_then(lens::Value::float) {
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

impl Hello {
    /// The last action the player used, or why the watch is missing.
    fn used_text(&self) -> String {
        match (&self.used, &self.last_used) {
            (Err(error), _) => format!("cannot watch the actions: {error}"),
            (Ok(_), None) => "no action used yet".into(),
            (Ok(_), Some(used)) => format!("used {used}"),
        }
    }
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
            chat: lens::Watch::new("chat_message_added { kind, sender.text, message.text }")
                .map_err(|error| error.message),
            used: lens::Watch::new("action_used[kind in (action, item)] { kind, action, target }")
                .map_err(|error| error.message),
            last_used: None,
            quiet: None,
            table: None,
            letters: Vec::new(),
            tab: 0,
            volume: 0.5,
            bar: ExperienceBar::default(),
        };
        if let Err(reason) = &hello.chat {
            let reason = format!("cannot watch the chat: {reason}");
            hello.say(reason);
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
                    section
                        .row(|row| {
                            row.button("table", "Activate the table")
                                .button("status", "Table status")
                                .button("letters", "Add letters")
                                .button("read", "Read game data")
                                .button("where", "Who am I")
                                .button("statuses", "My statuses")
                                .button("heard", "Last chat message")
                                .button("used", "Last action used")
                                .button("sprint", "Sprint")
                                .button("zoom", "Zoom out further")
                                .button("yes", "Say yes")
                                .button("mark", "Mark where I am")
                                .button("ping", "Ping myself")
                                .button("copy", "Copy a greeting")
                                .button("notify", "Notify me")
                                .button("beep", "Beep")
                                .button("cache", "Fill the cache")
                                .button("socket", "Echo over a socket")
                                .button(
                                    "quiet",
                                    if self.quiet.is_some() {
                                        "Show system messages"
                                    } else {
                                        "Hide system messages"
                                    },
                                )
                                .button("texture", "Replace a texture")
                        })
                        .row(|row| {
                            row.button(
                                "hide",
                                if self.bar.hidden {
                                    "Show the experience bar"
                                } else {
                                    "Hide the experience bar"
                                },
                            )
                            .button("label", "Clicks in the experience text")
                        })
                })
                .section("widgets", "Widgets", false, |section| {
                    let section = section.tabs("tabs", &["Numbers", "Table"], self.tab);
                    if self.tab == 0 {
                        section
                            .slider("volume", "Volume", self.volume, 0.0, 1.0)
                            .tooltip("How loud hello is")
                            .text(format!("volume {:.2}", self.volume))
                    } else {
                        section.table("items", &["", "Item", "Count"], |table| {
                            table
                                .icon(65002, 20)
                                .tooltip("Gil")
                                .text("Potion")
                                .tooltip("Restores HP")
                                .text(self.clicks.to_string())
                        })
                    }
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
            Event::Chosen(chosen) if chosen.id == "tabs" => self.tab = chosen.index,
            Event::Chosen(chosen) => self.color = Some(chosen.index),
            Event::Set(set) => self.volume = set.value,
            Event::Navigated(_) => {}
        }
    }

    fn game_event(&mut self, event: GameEvent) {
        if let Ok(used) = &self.used
            && let Some(rows) = used.rows(&event)
            && let Some(row) = rows.last()
        {
            let kind = used
                .get(row, "kind")
                .and_then(lens::Value::named)
                .unwrap_or("?");
            let action = used
                .get(row, "action")
                .and_then(lens::Value::unsigned)
                .unwrap_or(0);
            let target = used
                .get(row, "target")
                .and_then(lens::Value::unsigned)
                .unwrap_or(0);
            self.last_used = Some(format!("{kind} {action} on {target:#x}"));
        }
        if let GameEvent::Message(message) = &event {
            if message.in_reply_to.is_some() {
                self.say(format!(
                    "{} answered {}",
                    message.sender,
                    String::from_utf8_lossy(&message.payload)
                ));
            } else if let Err(error) = angelica_sdk::messages::reply(message, b"pong") {
                self.say(format!("cannot answer: {}", error.message));
            }
        }
        self.socket_event(&event);
        if let Ok(chat) = &self.chat
            && let Some(row) = chat.rows(&event).and_then(<[_]>::last)
        {
            let text = |name: &str| {
                chat.get(row, name)
                    .and_then(lens::Value::plain)
                    .unwrap_or_default()
            };
            let kind = chat
                .get(row, "kind")
                .and_then(lens::Value::unsigned)
                .unwrap_or_default();
            self.heard = Some(format!(
                "{} ({kind}): {}",
                text("sender_text"),
                text("message_text")
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
