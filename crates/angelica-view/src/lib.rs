//! What a plugin shows ([plugin API](../../../docs/formats/plugin-api-v1.md#places-and-parts)):
//! places (windows, panels, popups) holding parts, as typed values the SDK
//! builds and the host draws, and as the data the `ui` interface carries,
//! a `kind` with named properties.
//!
//! [`data`] turns places into that data and back, and [`check`] holds the
//! rules the host applies before showing anything. The list of kinds and
//! properties lives here once, so the SDK and the host never disagree.
#![forbid(unsafe_code)]

pub mod build;
pub mod check;
pub mod data;
pub mod examples;

/// The tone of a text or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Good,
    Warning,
    Bad,
}

/// What a text is for; the host gives each role its size and weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Body,
    Title,
    Value,
    Big,
    Label,
    Small,
}

/// How an action word looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionKind {
    #[default]
    Normal,
    /// The main action of a window: bold, with an arrow.
    Main,
    /// A side action, in the secondary text color.
    Quiet,
    /// What cannot be undone, in the danger color.
    Danger,
}

/// The mark at the end of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    #[default]
    None,
    /// In use or running.
    On,
    /// Not in use; the entry is drawn muted.
    Off,
    /// Needs the player.
    Attention,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub tone: Tone,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub kind: ActionKind,
}

/// A name and its value on one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub name: String,
    pub value: String,
}

/// An action of an entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub kind: ActionKind,
    pub enabled: bool,
}

/// A row of a list, drawn like the rows of Angelica's own list.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub note: String,
    pub value: String,
    pub mark: Mark,
    pub chosen: bool,
    /// From 0 to 1: a thin line in place of the note.
    pub progress: Option<f32>,
    /// One of the game's icons in place of the letter tile.
    pub icon: Option<u32>,
    /// No tile, and the note under the title.
    pub plain: bool,
    /// Up to 4 actions always shown at the right of the row.
    pub actions: Vec<Action>,
    /// Up to 16 facts under the chosen row.
    pub facts: Vec<Fact>,
    /// Up to 8 actions under the facts.
    pub more: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Checkbox {
    pub id: String,
    pub label: String,
    pub checked: bool,
    /// The plugin's words for checked and not checked, in place of the
    /// host's **On** and **Off**.
    pub words: Option<[String; 2]>,
    /// A few words under the setting.
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextInput {
    pub id: String,
    pub label: String,
    pub value: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub options: Vec<String>,
    pub selected: Option<u32>,
    /// A few words under the setting.
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub open_by_default: bool,
}

/// A number the player sets, between `min` and `max`.
#[derive(Debug, Clone, PartialEq)]
pub struct Number {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    /// The smallest change; 0 for any.
    pub step: f64,
    pub decimals: u8,
    pub slider: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tabs {
    pub id: String,
    pub labels: Vec<String>,
    pub selected: u32,
}

/// A column of sections at the side, and the chosen section's parts
/// beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    pub id: String,
    /// The sections, top to bottom.
    pub labels: Vec<String>,
    /// A short value at the right of each section; empty for none.
    pub values: Vec<String>,
    /// A small heading above each section; empty for none.
    pub groups: Vec<String>,
    pub selected: u32,
    /// The side's width in points.
    pub width: u16,
    /// An action word under the sections; empty for none.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub id: String,
    pub columns: Vec<String>,
}

/// One of the game's icons by its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icon {
    pub icon: u32,
    pub size: u16,
}

/// A value from 0 to `max` as a bar, or as `segments`.
#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    pub label: String,
    pub text: String,
    pub value: f64,
    pub max: f64,
    /// 0 for a bar, otherwise 2 to 32.
    pub segments: u8,
    pub tone: Tone,
}

/// A value from 0 to `max` as a ring with an icon or a short text inside.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub value: f64,
    pub max: f64,
    pub size: u16,
    pub icon: Option<u32>,
    pub text: String,
    pub tone: Tone,
}

/// A number whose change the host shows for a moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Readout {
    pub id: String,
    pub value: f64,
    pub decimals: u8,
    pub unit: String,
    pub role: Role,
    pub tone: Tone,
}

/// The time left until `until`, Unix time in milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Countdown {
    pub until: u64,
    pub done: String,
    pub role: Role,
}

/// Recent values as a line, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Sparkline {
    pub values: Vec<f32>,
    pub height: u16,
}

/// A part of a place. Containers are a `Begin…` and the matching `End…`
/// around the parts they hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    Heading(String),
    Text(Text),
    Button(Button),
    Checkbox(Checkbox),
    TextInput(TextInput),
    Choice(Choice),
    Separator,
    BeginRow,
    EndRow,
    BeginColumn,
    EndColumn,
    /// A grid of 2 to 8 equal columns.
    BeginGrid(u8),
    EndGrid,
    BeginSection(Section),
    EndSection,
    BeginTable(Table),
    EndTable,
    /// The chosen section's parts follow up to `EndSplit`.
    BeginSplit(Split),
    EndSplit,
    Spacer,
    Number(Number),
    Tabs(Tabs),
    Tooltip(String),
    Icon(Icon),
    Tile(String),
    Fact(Fact),
    Entry(Entry),
    Meter(Meter),
    Ring(Ring),
    Readout(Readout),
    Countdown(Countdown),
    Sparkline(Sparkline),
}

/// One short line in a window's title bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub path: Vec<String>,
    pub status: Option<Status>,
    pub widgets: Vec<Widget>,
    /// The question the window asks: drawn over it while the window waits
    /// dimmed for the answer.
    pub dialog: Option<Dialog>,
}

/// A question a window asks the player: its title, then parts that say what
/// is asked and buttons that answer it. Its clicks name the dialog's id;
/// `Closed` with its id arrives when the player closes it without an answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    pub id: String,
    pub title: String,
    pub widgets: Vec<Widget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backing {
    #[default]
    Plate,
    Bare,
}

/// Where a panel starts on the screen before the player moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    #[default]
    BottomLeft,
    Bottom,
    BottomRight,
}

/// When the host shows a panel, unless the player chose otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shown {
    #[default]
    Always,
    InCombat,
    OutOfCombat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub id: String,
    pub name: String,
    pub backing: Backing,
    pub anchor: Anchor,
    pub shown: Shown,
    pub widgets: Vec<Widget>,
}

/// Where a popup appears.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PopupAt {
    #[default]
    Corner,
    Center,
    /// Beside the plugin's panel with this id.
    Panel(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub at: PopupAt,
    pub seconds: u8,
    pub attention: bool,
    pub widgets: Vec<Widget>,
}

/// What a menu entry of the game is shown for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuFor {
    /// The menu of an item the player carries.
    #[default]
    Item,
}

/// An entry the plugin adds to a menu of the game, such as the one an item
/// opens: its words, which the host shows in the color of plugin names.
/// Choosing it arrives as `Event::MenuChosen`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
    pub target: MenuFor,
}

/// A window, a panel, or a menu entry, as `ui` returns them.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Window(Window),
    Panel(Panel),
    MenuEntry(MenuEntry),
}

impl Place {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Window(window) => &window.id,
            Self::Panel(panel) => &panel.id,
            Self::MenuEntry(entry) => &entry.id,
        }
    }

    #[must_use]
    pub fn widgets(&self) -> &[Widget] {
        match self {
            Self::Window(window) => &window.widgets,
            Self::Panel(panel) => &panel.widgets,
            Self::MenuEntry(_) => &[],
        }
    }
}

/// A part the player acted on: the id of its window or panel and its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetRef {
    pub window: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toggled {
    pub window: String,
    pub id: String,
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edited {
    pub window: String,
    pub id: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub window: String,
    pub id: String,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NumberSet {
    pub window: String,
    pub id: String,
    pub value: f64,
}

/// The player chose a menu entry of the plugin in the game's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuChosen {
    /// The entry's id.
    pub entry: String,
    /// What the menu was opened for: for an item, its row of `Item`.
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Navigated {
    pub window: String,
    pub depth: u32,
}

/// What the player did.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Clicked(WidgetRef),
    Toggled(Toggled),
    Edited(Edited),
    Chosen(Chosen),
    Set(NumberSet),
    /// The player closed the window with this id.
    Closed(String),
    Navigated(Navigated),
    /// The player asked to open the plugin from the Angelica menu.
    OpenRequested,
    MenuChosen(MenuChosen),
}

impl Event {
    /// Whether this is a click on the part `id` in `window`.
    #[must_use]
    pub fn clicked(&self, window: &str, id: &str) -> bool {
        matches!(self, Self::Clicked(target) if target.window == window && target.id == id)
    }
}
