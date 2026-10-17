//! The rules the host applies to places before showing them
//! ([plugin API](../../../docs/formats/plugin-api-v1.md#rules-for-every-place-and-part)).
use crate::{Checkbox, Choice, Entry, MenuEntry, Number, Place, Popup, Split, Tabs, Widget};
use std::collections::BTreeSet;

pub const MAX_WINDOWS: usize = 16;
pub const MAX_PANELS: usize = 8;
pub const MAX_PARTS: usize = 4096;
pub const MAX_POPUP_PARTS: usize = 32;
pub const MAX_TEXT: usize = 4096;
pub const MAX_TOTAL_TEXT: usize = 512 * 1024;
pub const MAX_ID: usize = 128;
pub const MAX_OPTIONS: usize = 1024;
pub const MAX_DEPTH: usize = 16;
pub const MAX_COLUMNS: usize = 16;
pub const MAX_TABS: usize = 32;
pub const MAX_PATH: usize = 4;
pub const MAX_ENTRY_ACTIONS: usize = 4;
pub const MAX_ENTRY_MORE: usize = 8;
pub const MAX_ENTRY_FACTS: usize = 16;
pub const MAX_RING_TEXT: usize = 8;
pub const MAX_PANEL_NAME: usize = 64;
pub const MAX_SPARKLINE: usize = 256;
pub const MAX_SECTIONS: usize = 64;
pub const MAX_MENU_ENTRIES: usize = 4;
pub const MAX_MENU_LABEL: usize = 40;
pub const MAX_WORD: usize = 32;

/// What a `Begin…` opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Row,
    Column,
    Grid,
    Section,
    Table,
    Split,
}

/// Checks the windows and panels `ui` returned.
///
/// # Errors
/// Names the first rule they break.
pub fn places(places: &[Place]) -> Result<(), String> {
    let windows = places
        .iter()
        .filter(|p| matches!(p, Place::Window(_)))
        .count();
    if windows > MAX_WINDOWS {
        return Err(format!("more than {MAX_WINDOWS} windows"));
    }
    let entries = places
        .iter()
        .filter(|p| matches!(p, Place::MenuEntry(_)))
        .count();
    if entries > MAX_MENU_ENTRIES {
        return Err(format!("more than {MAX_MENU_ENTRIES} menu entries"));
    }
    if places.len() - windows - entries > MAX_PANELS {
        return Err(format!("more than {MAX_PANELS} panels"));
    }
    let mut ids = BTreeSet::new();
    let mut parts = 0;
    let mut text = 0;
    for place in places {
        let id = place.id();
        if id.is_empty() || id.len() > MAX_ID || !ids.insert(id) {
            return Err(format!("place id {id:?} is empty, too long, or repeated"));
        }
        parts += place.widgets().len();
        if parts > MAX_PARTS {
            return Err(format!("more than {MAX_PARTS} parts"));
        }
        let mut count = |value: &str, what: &str| {
            text += value.len();
            if value.len() > MAX_TEXT {
                return Err(format!("{what} is longer than {MAX_TEXT} bytes"));
            }
            Ok(())
        };
        match place {
            Place::Window(window) => {
                if window.path.len() > MAX_PATH {
                    return Err(format!("window {id}: a path of more than {MAX_PATH} parts"));
                }
                for part in &window.path {
                    if part.trim().is_empty() {
                        return Err(format!("window {id}: an empty part of the path"));
                    }
                    count(part, "a part of the path")?;
                }
                if let Some(status) = &window.status {
                    count(&status.text, "the status")?;
                }
                count(&window.title, "the title")?;
                if let Some(dialog) = &window.dialog {
                    let dialog_id = dialog.id.as_str();
                    if dialog_id.is_empty() || dialog_id.len() > MAX_ID || !ids.insert(dialog_id) {
                        return Err(format!(
                            "dialog id {dialog_id:?} is empty, too long, or repeated"
                        ));
                    }
                    parts += dialog.widgets.len();
                    if parts > MAX_PARTS {
                        return Err(format!("more than {MAX_PARTS} parts"));
                    }
                    count(&dialog.title, "a dialog's title")?;
                    if let Some(input) = dialog
                        .widgets
                        .iter()
                        .find(|w| takes_input(w) && !matches!(w, Widget::Button(_)))
                    {
                        return Err(format!(
                            "dialog {dialog_id}: only buttons take input in a dialog, so it holds no {}",
                            name(input)
                        ));
                    }
                    widgets(&dialog.widgets, &mut count, false)
                        .map_err(|e| format!("{dialog_id}: {e}"))?;
                }
            }
            Place::Panel(panel) => {
                let name = panel.name.chars().count();
                if name == 0 || name > MAX_PANEL_NAME || panel.name.trim().is_empty() {
                    return Err(format!(
                        "panel {id}: a name of 1 to {MAX_PANEL_NAME} characters"
                    ));
                }
                count(&panel.name, "the name")?;
            }
            Place::MenuEntry(entry) => menu_entry(entry, &mut count)?,
        }
        let splits = matches!(place, Place::Window(_));
        widgets(place.widgets(), &mut count, splits).map_err(|e| format!("{id}: {e}"))?;
    }
    if text > MAX_TOTAL_TEXT {
        return Err(format!("more than {MAX_TOTAL_TEXT} bytes of text"));
    }
    Ok(())
}

/// Checks a popup: its parts take no input.
///
/// # Errors
/// Names the first rule it breaks.
pub fn popup(popup: &Popup) -> Result<(), String> {
    if popup.widgets.len() > MAX_POPUP_PARTS {
        return Err(format!("a popup has at most {MAX_POPUP_PARTS} parts"));
    }
    if let Some(input) = popup.widgets.iter().find(|w| takes_input(w)) {
        return Err(format!(
            "a popup takes no input, so it holds no {}",
            name(input)
        ));
    }
    let mut count = |value: &str, what: &str| {
        if value.len() > MAX_TEXT {
            return Err(format!("{what} is longer than {MAX_TEXT} bytes"));
        }
        Ok(())
    };
    widgets(&popup.widgets, &mut count, false)
}

/// Whether a part takes the player's input or is collapsed by the player.
fn takes_input(widget: &Widget) -> bool {
    matches!(
        widget,
        Widget::Button(_)
            | Widget::Checkbox(_)
            | Widget::TextInput(_)
            | Widget::Choice(_)
            | Widget::Number(_)
            | Widget::Tabs(_)
            | Widget::Entry(_)
            | Widget::BeginSection(_)
            | Widget::Tooltip(_)
            | Widget::BeginTable(_)
            | Widget::BeginSplit(_)
    )
}

fn name(widget: &Widget) -> &'static str {
    match widget {
        Widget::Button(_) => "button",
        Widget::Checkbox(_) => "checkbox",
        Widget::TextInput(_) => "text field",
        Widget::Choice(_) => "choice",
        Widget::Number(_) => "number",
        Widget::Tabs(_) => "tabs",
        Widget::Entry(_) => "entry",
        Widget::BeginSection(_) => "section",
        Widget::Tooltip(_) => "tooltip",
        Widget::BeginTable(_) => "table",
        Widget::BeginSplit(_) => "split",
        _ => "part",
    }
}

/// Checks the parts of one place; only a window's own parts hold a split.
fn widgets(
    widgets: &[Widget],
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
    splits: bool,
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut open: Vec<Group> = Vec::new();
    let mut previous: Option<&Widget> = None;
    let mut split_ended = false;
    for widget in widgets {
        if split_ended {
            return Err("a split is the last part of its window".into());
        }
        let id = widget_rules(widget, count)?;
        let in_table = open.contains(&Group::Table);
        match widget {
            Widget::BeginSplit(_) if !splits => {
                return Err("only a window holds a split".into());
            }
            Widget::BeginSplit(_) if !open.is_empty() => {
                return Err("a split is a window's own part, in no container".into());
            }
            Widget::BeginSplit(_) => open.push(Group::Split),
            Widget::BeginRow => open.push(Group::Row),
            Widget::BeginColumn => open.push(Group::Column),
            Widget::BeginGrid(_) => open.push(Group::Grid),
            Widget::BeginSection(_) | Widget::BeginTable(_) if in_table => {
                return Err("a table holds no sections or tables".into());
            }
            Widget::BeginSection(_) => open.push(Group::Section),
            Widget::BeginTable(_) => open.push(Group::Table),
            Widget::EndRow
            | Widget::EndColumn
            | Widget::EndGrid
            | Widget::EndSection
            | Widget::EndTable
            | Widget::EndSplit => {
                let expected = match widget {
                    Widget::EndRow => Group::Row,
                    Widget::EndColumn => Group::Column,
                    Widget::EndGrid => Group::Grid,
                    Widget::EndSection => Group::Section,
                    Widget::EndSplit => Group::Split,
                    _ => Group::Table,
                };
                if open.pop() != Some(expected) {
                    return Err("an end does not match its container".into());
                }
                split_ended = expected == Group::Split;
            }
            Widget::Tooltip(_) if !previous.is_some_and(shows_something) => {
                return Err("a tooltip does not follow a part".into());
            }
            _ => {}
        }
        previous = Some(widget);
        if open.len() > MAX_DEPTH {
            return Err(format!("containers nest deeper than {MAX_DEPTH}"));
        }
        let actions = match widget {
            Widget::Entry(entry) => Some(entry.actions.iter().chain(&entry.more)),
            _ => None,
        };
        for id in id.into_iter().chain(
            actions
                .into_iter()
                .flatten()
                .map(|action| action.id.as_str()),
        ) {
            if id.is_empty() || id.len() > MAX_ID || !ids.insert(id) {
                return Err(format!("part id {id:?} is empty, too long, or repeated"));
            }
        }
    }
    if !open.is_empty() {
        return Err("a container is not ended".into());
    }
    Ok(())
}

/// Whether the player sees the part itself, so a tooltip can point at it.
fn shows_something(widget: &Widget) -> bool {
    !matches!(
        widget,
        Widget::BeginRow
            | Widget::EndRow
            | Widget::BeginColumn
            | Widget::EndColumn
            | Widget::BeginGrid(_)
            | Widget::EndGrid
            | Widget::BeginSection(_)
            | Widget::EndSection
            | Widget::BeginTable(_)
            | Widget::EndTable
            | Widget::BeginSplit(_)
            | Widget::EndSplit
            | Widget::Tooltip(_)
            | Widget::Separator
            | Widget::Spacer
    )
}

fn entry(
    value: &Entry,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    count(&value.title, "an entry's title")?;
    count(&value.note, "an entry's note")?;
    count(&value.value, "an entry's value")?;
    if value.actions.len() > MAX_ENTRY_ACTIONS
        || value.more.len() > MAX_ENTRY_MORE
        || value.facts.len() > MAX_ENTRY_FACTS
    {
        return Err(format!(
            "entry {} has more than {MAX_ENTRY_ACTIONS} actions in its row, {MAX_ENTRY_MORE} under it, or {MAX_ENTRY_FACTS} facts",
            value.id
        ));
    }
    if value
        .progress
        .is_some_and(|fraction| !(0.0..=1.0).contains(&fraction))
    {
        return Err(format!("entry {} has a progress outside 0 to 1", value.id));
    }
    for fact in &value.facts {
        count(&fact.name, "a fact's name")?;
        count(&fact.value, "a fact's value")?;
    }
    for action in value.actions.iter().chain(&value.more) {
        count(&action.label, "a label")?;
    }
    Ok(())
}

fn menu_entry(
    entry: &MenuEntry,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    let length = entry.label.chars().count();
    if entry.label.trim().is_empty()
        || length > MAX_MENU_LABEL
        || entry.label.chars().any(char::is_control)
    {
        return Err(format!(
            "menu entry {}: a label of 1 to {MAX_MENU_LABEL} characters without control characters",
            entry.id
        ));
    }
    count(&entry.label, "a label")
}

fn tabs(
    value: &Tabs,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    if value.labels.is_empty() || value.labels.len() > MAX_TABS {
        return Err(format!("tabs {} have 1 to {MAX_TABS} labels", value.id));
    }
    for tab in &value.labels {
        count(tab, "a tab")?;
    }
    if value.selected as usize >= value.labels.len() {
        return Err(format!("tabs {} select a missing tab", value.id));
    }
    Ok(())
}

fn checkbox(
    value: &Checkbox,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    count(&value.label, "a label")?;
    for word in value.words.iter().flatten() {
        let length = word.chars().count();
        if word.trim().is_empty() || length > MAX_WORD {
            return Err(format!(
                "checkbox {} has words of 1 to {MAX_WORD} characters",
                value.id
            ));
        }
        count(word, "a word")?;
    }
    Ok(())
}

fn split(
    value: &Split,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    let sections = value.labels.len();
    if sections == 0 || sections > MAX_SECTIONS {
        return Err(format!(
            "split {} has 1 to {MAX_SECTIONS} sections",
            value.id
        ));
    }
    if value.values.len() > sections || value.groups.len() > sections {
        return Err(format!(
            "split {} has more values or groups than sections",
            value.id
        ));
    }
    if value.selected as usize >= sections {
        return Err(format!("split {} selects a missing section", value.id));
    }
    if !(96..=320).contains(&value.width) {
        return Err(format!("split {} is 96 to 320 wide", value.id));
    }
    for text in value
        .labels
        .iter()
        .chain(&value.values)
        .chain(&value.groups)
    {
        count(text, "a section")?;
    }
    count(&value.action, "an action")?;
    Ok(())
}

fn choice(
    value: &Choice,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    count(&value.label, "a label")?;
    if value.options.len() > MAX_OPTIONS {
        return Err(format!("a choice has more than {MAX_OPTIONS} options"));
    }
    for option in &value.options {
        count(option, "an option")?;
    }
    if value
        .selected
        .is_some_and(|index| index as usize >= value.options.len())
    {
        return Err(format!("choice {} selects a missing option", value.id));
    }
    Ok(())
}

fn number(value: &Number) -> Result<(), String> {
    let finite = [value.value, value.min, value.max, value.step]
        .iter()
        .all(|number| number.is_finite());
    if !finite
        || value.min > value.max
        || !(value.min..=value.max).contains(&value.value)
        || value.step < 0.0
        || value.decimals > 6
    {
        return Err(format!(
            "number {} needs finite min <= value <= max, a step of at least 0, and at most 6 decimals",
            value.id
        ));
    }
    Ok(())
}

/// A value from 0 to `max`.
fn fraction(what: &str, value: f64, max: f64) -> Result<(), String> {
    if !value.is_finite() || !max.is_finite() || max <= 0.0 {
        return Err(format!("a {what} needs a finite value and a max above 0"));
    }
    Ok(())
}

/// Checks a part's own rules and texts and returns its id, if it has one.
fn widget_rules<'a>(
    widget: &'a Widget,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<Option<&'a str>, String> {
    if value_rules(widget, count)? {
        return Ok(None);
    }
    Ok(match widget {
        Widget::Heading(value) | Widget::Tooltip(value) | Widget::Tile(value) => {
            count(value, "a text")?;
            None
        }
        Widget::Text(value) => {
            count(&value.text, "a text")?;
            None
        }
        Widget::Button(value) => {
            count(&value.label, "a label")?;
            Some(&value.id)
        }
        Widget::Checkbox(value) => {
            checkbox(value, count)?;
            Some(&value.id)
        }
        Widget::TextInput(value) => {
            count(&value.label, "a label")?;
            count(&value.value, "a value")?;
            count(&value.hint, "a hint")?;
            Some(&value.id)
        }
        Widget::Choice(value) => {
            choice(value, count)?;
            Some(&value.id)
        }
        Widget::BeginSection(value) => {
            count(&value.title, "a section title")?;
            Some(&value.id)
        }
        Widget::Number(value) => {
            count(&value.label, "a label")?;
            number(value)?;
            Some(&value.id)
        }
        Widget::Tabs(value) => {
            tabs(value, count)?;
            Some(&value.id)
        }
        Widget::BeginSplit(value) => {
            split(value, count)?;
            Some(&value.id)
        }
        Widget::BeginTable(value) => {
            if value.columns.is_empty() || value.columns.len() > MAX_COLUMNS {
                return Err(format!("table {} has 1 to {MAX_COLUMNS} columns", value.id));
            }
            for column in &value.columns {
                count(column, "a column heading")?;
            }
            Some(&value.id)
        }
        Widget::Fact(value) => {
            count(&value.name, "a fact's name")?;
            count(&value.value, "a fact's value")?;
            None
        }
        Widget::Entry(value) => {
            entry(value, count)?;
            Some(&value.id)
        }
        Widget::Readout(value) => {
            if !value.value.is_finite() || value.decimals > 6 {
                return Err(format!(
                    "readout {} needs a finite value and at most 6 decimals",
                    value.id
                ));
            }
            count(&value.unit, "a unit")?;
            Some(&value.id)
        }
        Widget::BeginGrid(_)
        | Widget::Icon(_)
        | Widget::Meter(_)
        | Widget::Ring(_)
        | Widget::Countdown(_)
        | Widget::Sparkline(_)
        | Widget::Separator
        | Widget::Spacer
        | Widget::BeginRow
        | Widget::EndRow
        | Widget::BeginColumn
        | Widget::EndColumn
        | Widget::EndGrid
        | Widget::EndSection
        | Widget::EndTable
        | Widget::EndSplit => None,
    }
    .map(String::as_str))
}

/// Checks a part that shows a value; returns whether `widget` is one.
fn value_rules(
    widget: &Widget,
    count: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<bool, String> {
    Ok(match widget {
        Widget::BeginGrid(columns) => {
            if !(2..=8).contains(columns) {
                return Err("a grid has 2 to 8 columns".into());
            }
            true
        }
        Widget::Icon(value) => {
            if value.size == 0 || value.size > 256 {
                return Err(format!("icon {} is drawn 1 to 256 wide", value.icon));
            }
            true
        }
        Widget::Meter(value) => {
            fraction("meter", value.value, value.max)?;
            if value.segments == 1 || value.segments > 32 {
                return Err("a meter has 0 or 2 to 32 segments".into());
            }
            count(&value.label, "a label")?;
            count(&value.text, "a text")?;
            true
        }
        Widget::Ring(value) => {
            fraction("ring", value.value, value.max)?;
            if !(16..=128).contains(&value.size) {
                return Err("a ring is 16 to 128 wide".into());
            }
            if value.text.chars().count() > MAX_RING_TEXT {
                return Err(format!(
                    "a ring's text has at most {MAX_RING_TEXT} characters"
                ));
            }
            true
        }
        Widget::Countdown(value) => {
            count(&value.done, "a text")?;
            true
        }
        Widget::Sparkline(value) => {
            if !(2..=MAX_SPARKLINE).contains(&value.values.len())
                || value.values.iter().any(|v| !v.is_finite())
                || !(16..=128).contains(&value.height)
            {
                return Err(format!(
                    "a sparkline has 2 to {MAX_SPARKLINE} finite values and a height of 16 to 128"
                ));
            }
            true
        }
        _ => false,
    })
}
