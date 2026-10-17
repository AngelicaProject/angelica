//! Builder methods: the same parts for windows, panels, and popups.
//!
//! ```ignore
//! Window::new("main", "Timers")
//!     .heading("Running")
//!     .row(|row| row.tile("Retainers").text("Retainers").spacer().countdown(Countdown::new(back)))
//!     .button("add", "Add");
//!
//! Panel::new("health", "Health")
//!     .bare()
//!     .meter(Meter::new(hp, max_hp).label("HP").text(format!("{hp} / {max_hp}")));
//! ```
use crate::{
    Action, ActionKind, Anchor, Backing, Button, Checkbox, Choice, Countdown, Dialog, Entry, Fact,
    Icon, Mark, MenuEntry, Meter, Number, Panel, Popup, PopupAt, Readout, Ring, Role, Section,
    Shown, Sparkline, Split, Status, Table, Tabs, Text, TextInput, Tone, Widget, Window,
};

impl MenuEntry {
    /// An entry with `label` in the menu of the items the player carries;
    /// choosing it arrives as `Event::MenuChosen` with `id` and the item's
    /// row of `Item`.
    #[must_use]
    pub fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            target: crate::MenuFor::Item,
        }
    }
}

impl Split {
    /// A column of sections with the one at `selected` chosen; a click on
    /// another arrives as `Event::Chosen` with `id`.
    #[must_use]
    pub fn new(id: impl Into<String>, selected: u32) -> Self {
        Self {
            id: id.into(),
            labels: Vec::new(),
            values: Vec::new(),
            groups: Vec::new(),
            selected,
            width: crate::data::SPLIT_WIDTH,
            action: String::new(),
        }
    }

    /// A section, under the ones added before it.
    #[must_use]
    pub fn section(self, label: impl Into<String>) -> Self {
        self.section_valued(label, "")
    }

    /// A section with a short value at its right.
    #[must_use]
    pub fn section_valued(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.values.resize(self.labels.len(), String::new());
            self.values.push(value);
        }
        self.labels.push(label.into());
        self
    }

    /// A small heading above the next section.
    #[must_use]
    pub fn group(mut self, heading: impl Into<String>) -> Self {
        self.groups.resize(self.labels.len(), String::new());
        self.groups.push(heading.into());
        self
    }

    /// The side's width in points, 96 to 320; 160 by default.
    #[must_use]
    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// An action word under the sections, such as `+ Group`; a click on it
    /// arrives as `Event::Clicked` with the split's id.
    #[must_use]
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = label.into();
        self
    }
}

impl Entry {
    /// A row of a list with its title. A click on it arrives as
    /// `Event::Clicked` with `id`.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            note: String::new(),
            value: String::new(),
            mark: Mark::None,
            chosen: false,
            progress: None,
            icon: None,
            plain: false,
            actions: Vec::new(),
            facts: Vec::new(),
            more: Vec::new(),
        }
    }

    /// A few words after the title.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    /// A short value at the right, such as a version.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// The mark at the right of the row: on, off, or needing the player.
    #[must_use]
    pub fn mark(mut self, mark: Mark) -> Self {
        self.mark = mark;
        self
    }

    /// Highlights the row and shows its facts and other actions under it.
    #[must_use]
    pub fn chosen(mut self, chosen: bool) -> Self {
        self.chosen = chosen;
        self
    }

    /// One of the game's icons by its number in place of the letter tile;
    /// an icon the game does not have shows as an empty frame.
    #[must_use]
    pub fn icon(mut self, icon: u32) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Draws the row without a tile, with the note under the title, for
    /// rows a letter or an icon would not tell apart.
    #[must_use]
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    /// A thin progress line in place of the note, from 0 to 1.
    #[must_use]
    pub fn progress(mut self, fraction: f32) -> Self {
        self.progress = Some(fraction);
        self
    }

    /// An action always shown at the right of the row.
    #[must_use]
    pub fn action(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.actions.push(Action {
            id: id.into(),
            label: label.into(),
            kind: ActionKind::Normal,
            enabled: true,
        });
        self
    }

    /// A fact under the chosen row.
    #[must_use]
    pub fn fact(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.facts.push(Fact {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    /// An action under the chosen row's facts.
    #[must_use]
    pub fn more(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        kind: ActionKind,
        enabled: bool,
    ) -> Self {
        self.more.push(Action {
            id: id.into(),
            label: label.into(),
            kind,
            enabled,
        });
        self
    }
}

impl Meter {
    /// `value` from 0 to `max` as a thin bar.
    #[must_use]
    pub fn new(value: f64, max: f64) -> Self {
        Self {
            label: String::new(),
            text: String::new(),
            value,
            max,
            segments: 0,
            tone: Tone::Normal,
        }
    }

    /// What it measures, before the bar.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The value in words, such as `35 250 / 52 000`.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// Shows the value as 2 to 32 segments instead of a bar.
    #[must_use]
    pub fn segments(mut self, segments: u8) -> Self {
        self.segments = segments;
        self
    }

    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl Ring {
    /// `value` from 0 to `max` as a ring 40 points wide.
    #[must_use]
    pub fn new(value: f64, max: f64) -> Self {
        Self {
            value,
            max,
            size: 40,
            icon: None,
            text: String::new(),
            tone: Tone::Normal,
        }
    }

    /// The diameter, 16 to 128.
    #[must_use]
    pub fn size(mut self, size: u16) -> Self {
        self.size = size;
        self
    }

    /// One of the game's icons inside.
    #[must_use]
    pub fn icon(mut self, icon: u32) -> Self {
        self.icon = Some(icon);
        self
    }

    /// At most 8 characters inside, when there is no icon.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl Readout {
    /// A number whose change the host shows for a moment.
    #[must_use]
    pub fn new(id: impl Into<String>, value: f64) -> Self {
        Self {
            id: id.into(),
            value,
            decimals: 0,
            unit: String::new(),
            role: Role::Value,
            tone: Tone::Normal,
        }
    }

    /// Digits after the decimal point, at most 6.
    #[must_use]
    pub fn decimals(mut self, decimals: u8) -> Self {
        self.decimals = decimals;
        self
    }

    /// Shown after the number, such as `%`.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl Countdown {
    /// The time left until `until`, Unix time in milliseconds; the host
    /// counts it down without calling the plugin.
    #[must_use]
    pub fn new(until: u64) -> Self {
        Self {
            until,
            done: "0:00".into(),
            role: Role::Value,
        }
    }

    /// Shown once it runs out.
    #[must_use]
    pub fn done(mut self, done: impl Into<String>) -> Self {
        self.done = done.into();
        self
    }

    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
}

impl Sparkline {
    /// Recent values, oldest first, as a line 36 points high.
    #[must_use]
    pub fn new(values: Vec<f32>) -> Self {
        Self { values, height: 36 }
    }

    /// Height in points, 16 to 128.
    #[must_use]
    pub fn height(mut self, height: u16) -> Self {
        self.height = height;
        self
    }
}

/// The builder methods every place has.
macro_rules! parts {
    ($place:ty) => {
        impl $place {
            /// Adds a part after the ones added before it.
            #[must_use]
            pub fn with(mut self, widget: Widget) -> Self {
                self.widgets.push(widget);
                self
            }

            /// A heading for the parts under it.
            #[must_use]
            pub fn heading(self, text: impl Into<String>) -> Self {
                self.with(Widget::Heading(text.into()))
            }

            /// Text in the normal tone.
            #[must_use]
            pub fn text(self, text: impl Into<String>) -> Self {
                self.toned(text, Tone::Normal)
            }

            /// Text in the muted tone.
            #[must_use]
            pub fn muted(self, text: impl Into<String>) -> Self {
                self.toned(text, Tone::Muted)
            }

            /// Text in `tone`.
            #[must_use]
            pub fn toned(self, text: impl Into<String>, tone: Tone) -> Self {
                self.styled(text, Role::Body, tone)
            }

            /// Text in `role` and `tone`: a title, a value, a large value,
            /// a label, or small text.
            #[must_use]
            pub fn styled(self, text: impl Into<String>, role: Role, tone: Tone) -> Self {
                self.with(Widget::Text(Text {
                    text: text.into(),
                    tone,
                    role,
                }))
            }

            /// A button; pressing it arrives as `Event::Clicked` with `id`.
            #[must_use]
            pub fn button(self, id: impl Into<String>, label: impl Into<String>) -> Self {
                self.button_enabled(id, label, true)
            }

            /// A button that the player can press only when `enabled`.
            #[must_use]
            pub fn button_enabled(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                enabled: bool,
            ) -> Self {
                self.action(id, label, ActionKind::Normal, enabled)
            }

            /// An action word of `kind`: the main action of the window, a
            /// side action, or one that cannot be undone.
            #[must_use]
            pub fn action(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                kind: ActionKind,
                enabled: bool,
            ) -> Self {
                self.with(Widget::Button(Button {
                    id: id.into(),
                    label: label.into(),
                    enabled,
                    kind,
                }))
            }

            /// A name and its value on one line.
            #[must_use]
            pub fn fact(self, name: impl Into<String>, value: impl Into<String>) -> Self {
                self.with(Widget::Fact(Fact {
                    name: name.into(),
                    value: value.into(),
                }))
            }

            /// A row of a list; see [`Entry`].
            #[must_use]
            pub fn entry(self, entry: Entry) -> Self {
                self.with(Widget::Entry(entry))
            }

            /// A checkbox; toggling it arrives as `Event::Toggled` with `id`.
            #[must_use]
            pub fn checkbox(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                checked: bool,
            ) -> Self {
                self.with(Widget::Checkbox(Checkbox {
                    id: id.into(),
                    label: label.into(),
                    checked,
                    words: None,
                    hint: String::new(),
                }))
            }

            /// A checkbox that shows the plugin's own two words, `on` for
            /// checked and `off` for not, in place of **On** and **Off**.
            #[must_use]
            pub fn checkbox_words(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                checked: bool,
                on: impl Into<String>,
                off: impl Into<String>,
            ) -> Self {
                self.with(Widget::Checkbox(Checkbox {
                    id: id.into(),
                    label: label.into(),
                    checked,
                    words: Some([on.into(), off.into()]),
                    hint: String::new(),
                }))
            }

            /// A text field with `value` and a `hint` while it is empty;
            /// each change arrives as `Event::Edited` with `id`.
            #[must_use]
            pub fn input(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                value: impl Into<String>,
                hint: impl Into<String>,
            ) -> Self {
                self.with(Widget::TextInput(TextInput {
                    id: id.into(),
                    label: label.into(),
                    value: value.into(),
                    hint: hint.into(),
                }))
            }

            /// A choice of `options` with `selected` shown; a new choice
            /// arrives as `Event::Chosen` with `id`.
            #[must_use]
            pub fn choice(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                options: Vec<String>,
                selected: Option<u32>,
            ) -> Self {
                self.with(Widget::Choice(Choice {
                    id: id.into(),
                    label: label.into(),
                    options,
                    selected,
                    hint: String::new(),
                }))
            }

            /// A few words under the checkbox or choice just added, such as
            /// what the setting changes; after any other part, nothing.
            #[must_use]
            pub fn hint(mut self, text: impl Into<String>) -> Self {
                match self.widgets.last_mut() {
                    Some(
                        Widget::Checkbox(Checkbox { hint, .. })
                        | Widget::Choice(Choice { hint, .. }),
                    ) => {
                        *hint = text.into();
                    }
                    _ => {}
                }
                self
            }

            /// A thin line filled to `fraction`, from 0 to 1, with a label:
            /// a [`Meter`] with a max of 1.
            #[must_use]
            pub fn progress(self, fraction: f32, label: impl Into<String>) -> Self {
                self.meter(Meter::new(f64::from(fraction), 1.0).label(label))
            }

            /// A value as a bar or segments; see [`Meter`].
            #[must_use]
            pub fn meter(self, meter: Meter) -> Self {
                self.with(Widget::Meter(meter))
            }

            /// A value as a ring; see [`Ring`].
            #[must_use]
            pub fn ring(self, ring: Ring) -> Self {
                self.with(Widget::Ring(ring))
            }

            /// A number whose change the host shows; see [`Readout`].
            #[must_use]
            pub fn readout(self, readout: Readout) -> Self {
                self.with(Widget::Readout(readout))
            }

            /// The time left until a moment; see [`Countdown`].
            #[must_use]
            pub fn countdown(self, countdown: Countdown) -> Self {
                self.with(Widget::Countdown(countdown))
            }

            /// Recent values as a line; see [`Sparkline`].
            #[must_use]
            pub fn sparkline(self, sparkline: Sparkline) -> Self {
                self.with(Widget::Sparkline(sparkline))
            }

            /// A square tile with the first letter of `text`.
            #[must_use]
            pub fn tile(self, text: impl Into<String>) -> Self {
                self.with(Widget::Tile(text.into()))
            }

            /// A thin line between the parts before and after it.
            #[must_use]
            pub fn separator(self) -> Self {
                self.with(Widget::Separator)
            }

            /// Takes the free room of its row or column, pushing what
            /// follows to its end.
            #[must_use]
            pub fn spacer(self) -> Self {
                self.with(Widget::Spacer)
            }

            /// A number field the player drags or types into, between
            /// `min` and `max`, with two decimals; use `with` and
            /// [`Number`] for a step or other decimals.
            #[must_use]
            pub fn number(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                value: f64,
                min: f64,
                max: f64,
            ) -> Self {
                self.with(Widget::Number(Number {
                    id: id.into(),
                    label: label.into(),
                    value,
                    min,
                    max,
                    step: 0.0,
                    decimals: 2,
                    slider: false,
                }))
            }

            /// A slider between `min` and `max`, with two decimals.
            #[must_use]
            pub fn slider(
                self,
                id: impl Into<String>,
                label: impl Into<String>,
                value: f64,
                min: f64,
                max: f64,
            ) -> Self {
                self.with(Widget::Number(Number {
                    id: id.into(),
                    label: label.into(),
                    value,
                    min,
                    max,
                    step: 0.0,
                    decimals: 2,
                    slider: true,
                }))
            }

            /// A row of tabs with `selected` chosen; a click on another
            /// arrives as `Event::Chosen`. Add the selected tab's parts
            /// after it.
            #[must_use]
            pub fn tabs(self, id: impl Into<String>, labels: &[&str], selected: u32) -> Self {
                self.with(Widget::Tabs(Tabs {
                    id: id.into(),
                    labels: labels.iter().map(|&label| label.to_owned()).collect(),
                    selected,
                }))
            }

            /// A column of sections at the side of the window and the
            /// chosen section's parts, which `build` adds, beside it; see
            /// [`Split`]. It is the last part of a window.
            #[must_use]
            pub fn split(self, split: Split, build: impl FnOnce(Self) -> Self) -> Self {
                build(self.with(Widget::BeginSplit(split))).with(Widget::EndSplit)
            }

            /// A table: each part (or row) `build` adds fills the next
            /// cell, left to right, row by row.
            #[must_use]
            pub fn table(
                self,
                id: impl Into<String>,
                columns: &[&str],
                build: impl FnOnce(Self) -> Self,
            ) -> Self {
                let begin = Widget::BeginTable(Table {
                    id: id.into(),
                    columns: columns.iter().map(|&column| column.to_owned()).collect(),
                });
                build(self.with(begin)).with(Widget::EndTable)
            }

            /// One of the game's icons by its number, in a square `size`
            /// wide; an icon the game does not have shows as an empty frame.
            #[must_use]
            pub fn icon(self, icon: u32, size: u16) -> Self {
                self.with(Widget::Icon(Icon { icon, size }))
            }

            /// Text shown when the player points at the part added last.
            #[must_use]
            pub fn tooltip(self, text: impl Into<String>) -> Self {
                self.with(Widget::Tooltip(text.into()))
            }

            /// Parts side by side.
            #[must_use]
            pub fn row(self, build: impl FnOnce(Self) -> Self) -> Self {
                build(self.with(Widget::BeginRow)).with(Widget::EndRow)
            }

            /// Parts one under another, such as in a cell of a row or grid.
            #[must_use]
            pub fn column(self, build: impl FnOnce(Self) -> Self) -> Self {
                build(self.with(Widget::BeginColumn)).with(Widget::EndColumn)
            }

            /// Parts in a grid of 2 to 8 equal columns, left to right, row
            /// by row.
            #[must_use]
            pub fn grid(self, columns: u8, build: impl FnOnce(Self) -> Self) -> Self {
                build(self.with(Widget::BeginGrid(columns))).with(Widget::EndGrid)
            }

            /// Parts under a heading the player can collapse.
            #[must_use]
            pub fn section(
                self,
                id: impl Into<String>,
                title: impl Into<String>,
                open: bool,
                build: impl FnOnce(Self) -> Self,
            ) -> Self {
                let begin = Widget::BeginSection(Section {
                    id: id.into(),
                    title: title.into(),
                    open_by_default: open,
                });
                build(self.with(begin)).with(Widget::EndSection)
            }
        }
    };
}

parts!(Window);
parts!(Panel);
parts!(Popup);
parts!(Dialog);

impl Window {
    /// A window with an id the plugin's events name and a title for its
    /// title bar.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            path: Vec::new(),
            status: None,
            widgets: Vec::new(),
            dialog: None,
        }
    }

    /// Asks the player `dialog` over this window, which waits dimmed until
    /// the player answers or closes the dialog. Return it every frame while
    /// the question stands.
    #[must_use]
    pub fn dialog(mut self, dialog: Dialog) -> Self {
        self.dialog = Some(dialog);
        self
    }

    /// Where the player is below the window's top level, outermost first.
    /// The title bar shows it with a way back, which arrives as
    /// `Event::Navigated`.
    #[must_use]
    pub fn path<S: Into<String>>(mut self, parts: impl IntoIterator<Item = S>) -> Self {
        self.path = parts.into_iter().map(Into::into).collect();
        self
    }

    /// One short line in the title bar about what the window is for.
    #[must_use]
    pub fn status(mut self, text: impl Into<String>, tone: Tone) -> Self {
        self.status = Some(Status {
            text: text.into(),
            tone,
        });
        self
    }
}

impl Dialog {
    /// A question with an id its clicks and its closing name, and a title
    /// such as "Trust this publisher?". Add the parts that say what is
    /// asked and buttons that answer it; only buttons take input in it.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            widgets: Vec::new(),
        }
    }
}

impl Panel {
    /// A panel on a plate, starting at the bottom left, shown always; the
    /// player places it, and `name` names it in the layout mode.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            backing: Backing::Plate,
            anchor: Anchor::BottomLeft,
            shown: Shown::Always,
            widgets: Vec::new(),
        }
    }

    /// Straight over the game, with a shadow that keeps it readable.
    #[must_use]
    pub fn bare(mut self) -> Self {
        self.backing = Backing::Bare;
        self
    }

    /// Where it starts before the player moves it.
    #[must_use]
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// When it is shown, unless the player chose otherwise.
    #[must_use]
    pub fn shown(mut self, shown: Shown) -> Self {
        self.shown = shown;
        self
    }
}

impl Popup {
    /// A popup in the corner, with Angelica's notes, for 5 seconds.
    #[must_use]
    pub fn corner() -> Self {
        Self::at(PopupAt::Corner)
    }

    /// A popup above the middle of the screen, its first text large.
    #[must_use]
    pub fn center() -> Self {
        Self::at(PopupAt::Center)
    }

    /// A popup beside the plugin's panel `panel`.
    #[must_use]
    pub fn beside(panel: impl Into<String>) -> Self {
        Self::at(PopupAt::Panel(panel.into()))
    }

    fn at(at: PopupAt) -> Self {
        Self {
            at,
            seconds: 5,
            attention: false,
            widgets: Vec::new(),
        }
    }

    /// How long it stays, 2 to 10 seconds.
    #[must_use]
    pub fn seconds(mut self, seconds: u8) -> Self {
        self.seconds = seconds;
        self
    }

    /// Also flashes the game's button on the taskbar while the game is in
    /// the background.
    #[must_use]
    pub fn attention(mut self) -> Self {
        self.attention = true;
        self
    }
}
