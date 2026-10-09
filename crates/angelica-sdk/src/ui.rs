//! Windows for the `ui` interface, with builder methods.
//!
//! ```ignore
//! Window::new("main", "Translations")
//!     .heading("Packs")
//!     .tabs("view", &["Installed", "Available"], self.tab)
//!     .table("packs", &["Pack", "Size"], |table| {
//!         table.text("Russian").tooltip("Community translation").text("12 MB")
//!     })
//!     .slider("opacity", "Opacity", self.opacity, 0.0, 1.0)
//!     .row(|row| row.button("import", "Import").button("remove", "Remove"))
//!     .section("details", "Details", false, |section| section.muted("..."))
//! ```
pub use crate::bindings::angelica::plugin::ui::*;

impl Window {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            widgets: Vec::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, widget: Widget) -> Self {
        self.widgets.push(widget);
        self
    }

    #[must_use]
    pub fn heading(self, text: impl Into<String>) -> Self {
        self.with(Widget::Heading(text.into()))
    }

    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.toned(text, Tone::Normal)
    }

    #[must_use]
    pub fn muted(self, text: impl Into<String>) -> Self {
        self.toned(text, Tone::Muted)
    }

    #[must_use]
    pub fn toned(self, text: impl Into<String>, tone: Tone) -> Self {
        self.with(Widget::Text(Text {
            text: text.into(),
            tone,
        }))
    }

    #[must_use]
    pub fn button(self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.button_enabled(id, label, true)
    }

    #[must_use]
    pub fn button_enabled(
        self,
        id: impl Into<String>,
        label: impl Into<String>,
        enabled: bool,
    ) -> Self {
        self.with(Widget::Button(Button {
            id: id.into(),
            label: label.into(),
            enabled,
        }))
    }

    #[must_use]
    pub fn checkbox(self, id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        self.with(Widget::Checkbox(Checkbox {
            id: id.into(),
            label: label.into(),
            checked,
        }))
    }

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
        }))
    }

    #[must_use]
    pub fn progress(self, fraction: f32, label: impl Into<String>) -> Self {
        self.with(Widget::Progress(Progress {
            fraction,
            label: label.into(),
        }))
    }

    #[must_use]
    pub fn separator(self) -> Self {
        self.with(Widget::Separator)
    }

    /// A number field the player drags or types into, between `min` and
    /// `max`, with two decimals; use [`Window::with`] and [`Number`] for a
    /// step or other decimals.
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

    /// A row of tabs with `selected` chosen; a click on another arrives as
    /// [`Event::Chosen`]. Add the selected tab's widgets after it.
    #[must_use]
    pub fn tabs(self, id: impl Into<String>, labels: &[&str], selected: u32) -> Self {
        self.with(Widget::Tabs(Tabs {
            id: id.into(),
            labels: labels.iter().map(|&label| label.to_owned()).collect(),
            selected,
        }))
    }

    /// A table: each widget (or row) `build` adds fills the next cell, left
    /// to right, row by row.
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

    /// One of the game's icons by its number, in a square `size` wide; an
    /// icon the game does not have shows as an empty frame.
    #[must_use]
    pub fn icon(self, icon: u32, size: u16) -> Self {
        self.with(Widget::Icon(Icon { icon, size }))
    }

    /// Text shown when the player points at the widget added last.
    #[must_use]
    pub fn tooltip(self, text: impl Into<String>) -> Self {
        self.with(Widget::Tooltip(text.into()))
    }

    /// Widgets side by side.
    #[must_use]
    pub fn row(self, build: impl FnOnce(Self) -> Self) -> Self {
        build(self.with(Widget::BeginRow)).with(Widget::EndRow)
    }

    /// Widgets under a heading the player can collapse.
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

impl Event {
    /// Whether this is a click on the button `id` in `window`.
    #[must_use]
    pub fn clicked(&self, window: &str, id: &str) -> bool {
        matches!(self, Self::Clicked(target) if target.window == window && target.id == id)
    }
}
