//! Windows for the `ui` interface, with builder methods.
//!
//! ```ignore
//! Window::new("main", "Translations")
//!     .heading("Packs")
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
