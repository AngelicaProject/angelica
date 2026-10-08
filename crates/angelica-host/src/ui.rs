//! Checks of the windows a plugin describes (the `ui` interface) before the
//! host draws them.
#![forbid(unsafe_code)]
use crate::sandbox::bindings::angelica::plugin::ui::{Widget, Window};
use std::collections::BTreeSet;

pub const MAX_WINDOWS: usize = 16;
pub const MAX_WIDGETS: usize = 4096;
pub const MAX_TEXT: usize = 4096;
pub const MAX_TOTAL_TEXT: usize = 512 * 1024;
pub const MAX_ID: usize = 128;
pub const MAX_OPTIONS: usize = 1024;
pub const MAX_DEPTH: usize = 16;

/// # Errors
/// Names the first rule a window breaks: too many windows or widgets, text
/// or ids too long, repeated ids, or unbalanced rows and sections.
pub fn validate(windows: &[Window]) -> Result<(), String> {
    if windows.len() > MAX_WINDOWS {
        return Err(format!("more than {MAX_WINDOWS} windows"));
    }
    let mut window_ids = BTreeSet::new();
    let mut widgets = 0;
    let mut text = 0;
    for window in windows {
        if window.id.is_empty()
            || window.id.len() > MAX_ID
            || !window_ids.insert(window.id.as_str())
        {
            return Err(format!(
                "window id {:?} is empty, too long, or repeated",
                window.id
            ));
        }
        widgets += window.widgets.len();
        if widgets > MAX_WIDGETS {
            return Err(format!("more than {MAX_WIDGETS} widgets"));
        }
        text += validate_window(window).map_err(|e| format!("window {}: {e}", window.id))?;
    }
    if text > MAX_TOTAL_TEXT {
        return Err(format!("more than {MAX_TOTAL_TEXT} bytes of text"));
    }
    Ok(())
}

/// Checks one window and returns the bytes of text it shows.
fn validate_window(window: &Window) -> Result<usize, String> {
    let mut text = 0;
    let mut label = |value: &str, what: &str| {
        text += value.len();
        if value.len() > MAX_TEXT {
            return Err(format!("{what} is longer than {MAX_TEXT} bytes"));
        }
        Ok(())
    };
    label(&window.title, "the title")?;
    let mut ids = BTreeSet::new();
    let mut open: Vec<bool> = Vec::new();
    for widget in &window.widgets {
        let id = validate_widget(widget, &mut label)?;
        match widget {
            Widget::BeginRow => open.push(false),
            Widget::BeginSection(_) => open.push(true),
            Widget::EndRow | Widget::EndSection
                if open.pop() != Some(matches!(widget, Widget::EndSection)) =>
            {
                return Err("an end does not match its begin".into());
            }
            _ => {}
        }
        if open.len() > MAX_DEPTH {
            return Err(format!("rows and sections nest deeper than {MAX_DEPTH}"));
        }
        if let Some(id) = id
            && (id.is_empty() || id.len() > MAX_ID || !ids.insert(id))
        {
            return Err(format!("widget id {id:?} is empty, too long, or repeated"));
        }
    }
    if !open.is_empty() {
        return Err("a row or section is not ended".into());
    }
    Ok(text)
}

/// Checks a widget's text and returns its id, if it has one.
fn validate_widget<'a>(
    widget: &'a Widget,
    label: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<Option<&'a str>, String> {
    Ok(match widget {
        Widget::Heading(value) => {
            label(value, "a heading")?;
            None
        }
        Widget::Text(value) => {
            label(&value.text, "a text")?;
            None
        }
        Widget::Button(value) => {
            label(&value.label, "a label")?;
            Some(&value.id)
        }
        Widget::Checkbox(value) => {
            label(&value.label, "a label")?;
            Some(&value.id)
        }
        Widget::TextInput(value) => {
            label(&value.label, "a label")?;
            label(&value.value, "a value")?;
            label(&value.hint, "a hint")?;
            Some(&value.id)
        }
        Widget::Choice(value) => {
            label(&value.label, "a label")?;
            if value.options.len() > MAX_OPTIONS {
                return Err(format!("a choice has more than {MAX_OPTIONS} options"));
            }
            for option in &value.options {
                label(option, "an option")?;
            }
            if value
                .selected
                .is_some_and(|index| index as usize >= value.options.len())
            {
                return Err(format!("choice {} selects a missing option", value.id));
            }
            Some(&value.id)
        }
        Widget::Progress(value) => {
            label(&value.label, "a label")?;
            None
        }
        Widget::BeginSection(value) => {
            label(&value.title, "a section title")?;
            Some(&value.id)
        }
        Widget::Separator | Widget::BeginRow | Widget::EndRow | Widget::EndSection => None,
    }
    .map(String::as_str))
}
