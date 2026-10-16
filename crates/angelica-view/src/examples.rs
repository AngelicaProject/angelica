//! One example of every place and part, with the SDK code that builds it.
//! The documentation's gallery draws each with the host's renderer and shows
//! its code; a test keeps every kind of the plugin API covered, so a new
//! kind cannot go undocumented.
use crate::{
    ActionKind, Countdown, Dialog, Entry, Mark, Meter, Panel, Popup, Readout, Ring, Role, Shown,
    Sparkline, Tone, Widget, Window,
};

/// What an example shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Shows {
    /// Parts in a window's body.
    Parts(Vec<Widget>),
    /// A whole window, with its dialog when it asks one.
    Window(Window),
    Panel(Panel),
    Popup(Popup),
}

/// An example of a place or a part.
#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    /// The name its picture goes by, unique among the examples.
    pub name: &'static str,
    /// The kind of place or part it shows, as the plugin API names it.
    pub kind: &'static str,
    /// What the picture shows, in one sentence.
    pub about: &'static str,
    pub shows: Shows,
    /// The SDK code that builds it.
    pub code: String,
}

/// This file, from which each example's code is cut.
const SOURCE: &str = include_str!("examples.rs");

/// The code between `// [name]` and `// [/name]` in this file, unindented.
fn code(name: &str) -> String {
    let start = format!("// [{name}]");
    let end = format!("// [/{name}]");
    let Some(from) = SOURCE.find(&start).map(|at| at + start.len()) else {
        return String::new();
    };
    let to = SOURCE[from..]
        .find(&end)
        .map_or(SOURCE.len(), |at| from + at);
    let lines: Vec<&str> = SOURCE[from..to]
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("// ["))
        .collect();
    let indent = lines
        .iter()
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let code = lines
        .iter()
        .map(|line| &line[indent.min(line.len())..])
        .collect::<Vec<_>>()
        .join("\n");
    // Formatting may leave the end marker after the brackets and comma of
    // the call around the example; they are not the example's.
    let mut code = code.trim_end().to_owned();
    loop {
        let open = code.matches(['(', '[', '{']).count();
        let close = code.matches([')', ']', '}']).count();
        let separator = code.ends_with(',') || code.ends_with(' ');
        let unopened = close > open && (code.ends_with(')') || code.ends_with(']'));
        if !(separator || unopened) {
            break;
        }
        code.pop();
    }
    // A method cut from a chain is shown on its window.
    if code.starts_with('.') {
        let indented: Vec<String> = code.lines().map(|line| format!("    {line}")).collect();
        format!("window\n{}", indented.join("\n"))
    } else {
        code
    }
}

fn example(name: &'static str, kind: &'static str, about: &'static str, shows: Shows) -> Example {
    Example {
        name,
        kind,
        about,
        shows,
        code: code(name),
    }
}

fn part(
    name: &'static str,
    kind: &'static str,
    about: &'static str,
    build: impl FnOnce(Window) -> Window,
) -> Example {
    example(
        name,
        kind,
        about,
        Shows::Parts(build(Window::new("example", "")).widgets),
    )
}

/// Every example. `now` is the Unix time in milliseconds that the
/// countdown counts towards from.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn examples(now: u64) -> Vec<Example> {
    let pull_at = now + 83_000;
    vec![
        example(
            "window",
            "window",
            "A tool the player opens. Its title bar shows where the player is, with a way back, and a status.",
            Shows::Window(
                // [window]
                Window::new("main", "Timers")
                    .path(["Settings"])
                    .status("3 running", Tone::Good)
                    .heading("Sounds")
                    .checkbox("bell", "Ring when a timer ends", true)
                    .row(|row| row.action("save", "Save", ActionKind::Main, true)), // [/window]
            ),
        ),
        example(
            "dialog",
            "dialog",
            "A question over its window. The window waits, dimmed, until the player answers or closes the dialog.",
            Shows::Window(
                Window::new("main", "Timers")
                    .text("Tea · 2:30")
                    .text("Bread · 41:05")
                    .text("Laundry · 1:12:00")
                    // [dialog]
                    .dialog(
                        Dialog::new("remove", "Remove this timer?")
                            .styled("Bread", Role::Title, Tone::Normal)
                            .text("It stops and leaves the list.")
                            .row(|row| {
                                row.action("yes", "Remove", ActionKind::Danger, true)
                                    .action("no", "Cancel", ActionKind::Quiet, true)
                            }),
                    ),
                // [/dialog]
            ),
        ),
        example(
            "panel",
            "panel",
            "Stays on the screen while the player plays, where the player put it.",
            Shows::Panel(
                // [panel]
                Panel::new("timers", "Timers")
                    .row(|row| row.text("Tea").spacer().countdown(Countdown::new(pull_at)))
                    .meter(Meter::new(0.4, 1.0)), // [/panel]
            ),
        ),
        example(
            "panel-bare",
            "panel",
            "Bare, straight over the game; this one shows only in combat.",
            Shows::Panel(
                // [panel-bare]
                Panel::new("gauge", "Gauge")
                    .bare()
                    .shown(Shown::InCombat)
                    .row(|row| {
                        row.styled("Gauge", Role::Label, Tone::Normal)
                            .spacer()
                            .readout(Readout::new("gauge", 70.0))
                    })
                    .meter(Meter::new(70.0, 100.0).segments(10).tone(Tone::Good)), // [/panel-bare]
            ),
        ),
        example(
            "popup",
            "popup",
            "Shows for a few seconds under the plugin's name, then goes. It takes no input.",
            Shows::Popup(
                // [popup]
                Popup::corner()
                    .styled("Pull in", Role::Label, Tone::Normal)
                    .countdown(Countdown::new(pull_at).role(Role::Big)), // [/popup]
            ),
        ),
        part(
            "text",
            "text",
            "Text in a role (body, title, value, big, label, small) and a tone.",
            |window| {
                // [text]
                window
                    .styled("Ready", Role::Title, Tone::Good)
                    .text("The body text of a window.")
                    .muted("A note that matters less.")
                    .toned("Needs the player", Tone::Warning)
                    .styled("41 230", Role::Big, Tone::Normal)
                // [/text]
            },
        ),
        part(
            "heading",
            "heading",
            "A small header with a rule to the right edge.",
            |window| {
                // [heading]
                window.heading("Sounds").text("Ring when a timer ends")
                // [/heading]
            },
        ),
        part("fact", "fact", "A name and its value.", |window| {
            // [fact]
            window
                .fact("Version", "0.4.0")
                .fact("Publisher", "key 3F9A·C21B")
            // [/fact]
        }),
        part(
            "tile",
            "tile",
            "A square with the first letter of a name.",
            |window| {
                // [tile]
                window.row(|row| {
                    row.tile("Harmonia")
                        .text("Harmonia")
                        .tile("Timers")
                        .text("Timers")
                })
                // [/tile]
            },
        ),
        part(
            "icon",
            "icon",
            "A game icon by its number. Outside the game it draws as an empty frame, as here.",
            |window| {
                // [icon]
                window.row(|row| row.icon(65002, 24).text("Gil"))
                // [/icon]
            },
        ),
        part(
            "tooltip",
            "tooltip",
            "Shows while the pointer is on the part before it, so the picture has only the part.",
            |window| {
                // [tooltip]
                window
                    .button("save", "Save")
                    .tooltip("Writes the list to the plugin's storage")
                // [/tooltip]
            },
        ),
        part("separator", "separator", "A thin line.", |window| {
            // [separator]
            window.text("Above").separator().text("Below")
            // [/separator]
        }),
        part(
            "row",
            "row",
            "Parts side by side. A row too narrow for them wraps.",
            |window| {
                // [row]
                window.row(|row| row.text("Tea").text("2:30").button("stop", "Stop"))
                // [/row]
            },
        ),
        part(
            "column",
            "column",
            "Parts one under another, here two columns in a row.",
            |window| {
                // [column]
                window.row(|row| {
                    row.column(|column| {
                        column.styled("HP", Role::Label, Tone::Normal).styled(
                            "41 230",
                            Role::Value,
                            Tone::Normal,
                        )
                    })
                    .column(|column| {
                        column.styled("MP", Role::Label, Tone::Normal).styled(
                            "9 800",
                            Role::Value,
                            Tone::Normal,
                        )
                    })
                })
                // [/column]
            },
        ),
        part(
            "grid",
            "grid",
            "Equal columns, filled left to right.",
            |window| {
                // [grid]
                window.grid(3, |grid| {
                    grid.ring(Ring::new(0.8, 1.0).text("80%"))
                        .ring(Ring::new(3.0, 10.0).text("3"))
                        .ring(Ring::new(1.0, 1.0).tone(Tone::Good).text("ok"))
                })
                // [/grid]
            },
        ),
        part(
            "spacer",
            "spacer",
            "Pushes the rest of its row to the right end.",
            |window| {
                // [spacer]
                window.row(|row| {
                    row.text("Clicks")
                        .spacer()
                        .readout(Readout::new("clicks", 31.0))
                })
                // [/spacer]
            },
        ),
        part(
            "section",
            "section",
            "A heading the player clicks to fold what is under it.",
            |window| {
                // [section]
                window.section("more", "More", true, |section| {
                    section.text("Shown while the section is open.")
                })
                // [/section]
            },
        ),
        part(
            "tabs",
            "tabs",
            "The tab row only. The plugin adds the chosen tab's parts after it.",
            |window| {
                // [tabs]
                window
                    .tabs("view", &["Running", "Settings"], 0)
                    .text("The Running tab's parts.")
                // [/tabs]
            },
        ),
        part(
            "table",
            "table",
            "Columns under headings; each cell is one part.",
            |window| {
                // [table]
                window.table("timers", &["", "Timer", "Left"], |table| {
                    table
                        .tile("Tea")
                        .text("Tea")
                        .text("2:30")
                        .tile("Bread")
                        .text("Bread")
                        .text("41:05")
                })
                // [/table]
            },
        ),
        part(
            "entry",
            "entry",
            "A row like those of Angelica's menu. The chosen row opens its facts and actions.",
            |window| {
                // [entry]
                window
                    .entry(
                        Entry::new("tea", "Tea")
                            .note("every day")
                            .value("2:30")
                            .mark(Mark::On)
                            .chosen(true)
                            .fact("Sound", "Bell")
                            .more("remove", "Remove", ActionKind::Danger, true),
                    )
                    .entry(Entry::new("bread", "Bread").note("once").value("41:05"))
                // [/entry]
            },
        ),
        part(
            "meter",
            "meter",
            "A value out of a maximum, as a bar or in segments. A new value slides into place.",
            |window| {
                // [meter]
                window
                    .meter(
                        Meter::new(35_250.0, 52_000.0)
                            .label("Experience")
                            .text("35 250 / 52 000"),
                    )
                    .meter(Meter::new(3.0, 5.0).segments(5).tone(Tone::Warning))
                // [/meter]
            },
        ),
        part(
            "ring",
            "ring",
            "A value out of a maximum as an arc, with a short text or a game icon inside.",
            |window| {
                // [ring]
                window.row(|row| {
                    row.ring(Ring::new(0.62, 1.0).text("62%"))
                        .ring(Ring::new(2.0, 10.0).size(56).tone(Tone::Bad).text("2"))
                })
                // [/ring]
            },
        ),
        part(
            "readout",
            "readout",
            "A number. When it changes, the difference shows beside it for a moment.",
            |window| {
                // [readout]
                window.readout(Readout::new("dps", 12_480.0).unit(" dps").role(Role::Big))
                // [/readout]
            },
        ),
        part(
            "countdown",
            "countdown",
            "Counts down to a time by itself; the plugin gives the time once.",
            |window| {
                // [countdown]
                window.countdown(Countdown::new(pull_at).done("Pull!").role(Role::Big))
                // [/countdown]
            },
        ),
        part(
            "sparkline",
            "sparkline",
            "Recent values as a line, scaled between the lowest and the highest.",
            |window| {
                // [sparkline]
                window.sparkline(Sparkline::new(vec![
                    3.0, 4.5, 4.0, 6.5, 6.0, 8.0, 7.0, 9.5, 9.0, 11.0,
                ]))
                // [/sparkline]
            },
        ),
        part(
            "button",
            "button",
            "Actions are words: main, normal, quiet, and danger.",
            |window| {
                // [button]
                window.row(|row| {
                    row.action("start", "Start", ActionKind::Main, true)
                        .button("pause", "Pause")
                        .action("later", "Later", ActionKind::Quiet, true)
                        .action("reset", "Reset", ActionKind::Danger, true)
                })
                // [/button]
            },
        ),
        part(
            "checkbox",
            "checkbox",
            "On or off. The plugin keeps the state; a click arrives as an event.",
            |window| {
                // [checkbox]
                window.checkbox("bell", "Ring when a timer ends", true)
                // [/checkbox]
            },
        ),
        part(
            "text-field",
            "text-field",
            "A text field with a hint while it is empty.",
            |window| {
                // [text-field]
                window.input("name", "Name", "", "a timer name")
                // [/text-field]
            },
        ),
        part("choice", "choice", "One of a list of options.", |window| {
            // [choice]
            window.choice(
                "sound",
                "Sound",
                vec!["Bell".into(), "Chime".into()],
                Some(0),
            )
            // [/choice]
        }),
        part(
            "number",
            "number",
            "A number in a field the player drags or types into, or on a slider.",
            |window| {
                // [number]
                window
                    .number("minutes", "Minutes", 5.0, 0.0, 60.0)
                    .slider("volume", "Volume", 0.8, 0.0, 1.0)
                // [/number]
            },
        ),
    ]
}
