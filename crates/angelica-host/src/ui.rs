//! Checks of the windows a plugin describes (the `ui` interface) before the
//! host draws them.
#![forbid(unsafe_code)]
use crate::sandbox::bindings::angelica::plugin::ui::{Number, Widget, Window};
use std::collections::BTreeSet;

pub const MAX_WINDOWS: usize = 16;
pub const MAX_WIDGETS: usize = 4096;
pub const MAX_TEXT: usize = 4096;
pub const MAX_TOTAL_TEXT: usize = 512 * 1024;
pub const MAX_ID: usize = 128;
pub const MAX_OPTIONS: usize = 1024;
pub const MAX_DEPTH: usize = 16;
pub const MAX_COLUMNS: usize = 16;
pub const MAX_TABS: usize = 32;
pub const MAX_DECIMALS: u8 = 6;
pub const MAX_ICON_SIZE: u16 = 256;

/// What a `begin` opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Row,
    Section,
    Table,
}

/// # Errors
/// Names the first rule a window breaks: too many windows or widgets, text
/// or ids too long, repeated ids, unbalanced rows, sections, and tables, a
/// section or table in a table, a tooltip without a widget before it, or a
/// number, tabs, table, or icon out of their bounds.
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
    let mut open: Vec<Group> = Vec::new();
    let mut previous: Option<&Widget> = None;
    for widget in &window.widgets {
        let id = validate_widget(widget, &mut label)?;
        let in_table = open.contains(&Group::Table);
        match widget {
            Widget::BeginRow => open.push(Group::Row),
            Widget::BeginSection(_) | Widget::BeginTable(_) if in_table => {
                return Err("a table holds no sections or tables".into());
            }
            Widget::BeginSection(_) => open.push(Group::Section),
            Widget::BeginTable(_) => open.push(Group::Table),
            Widget::EndRow | Widget::EndSection | Widget::EndTable => {
                let expected = match widget {
                    Widget::EndRow => Group::Row,
                    Widget::EndSection => Group::Section,
                    _ => Group::Table,
                };
                if open.pop() != Some(expected) {
                    return Err("an end does not match its begin".into());
                }
            }
            Widget::Tooltip(_) if !previous.is_some_and(shows_something) => {
                return Err("a tooltip does not follow a widget".into());
            }
            _ => {}
        }
        previous = Some(widget);
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

/// Whether the player sees the widget itself, so a tooltip can point at it.
fn shows_something(widget: &Widget) -> bool {
    !matches!(
        widget,
        Widget::BeginRow
            | Widget::EndRow
            | Widget::BeginSection(_)
            | Widget::EndSection
            | Widget::BeginTable(_)
            | Widget::EndTable
            | Widget::Tooltip(_)
            | Widget::Separator
    )
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
        Widget::Number(value) => {
            label(&value.label, "a label")?;
            validate_number(value)?;
            Some(&value.id)
        }
        Widget::Tabs(value) => {
            if value.labels.is_empty() || value.labels.len() > MAX_TABS {
                return Err(format!("tabs {} have 1 to {MAX_TABS} labels", value.id));
            }
            for tab in &value.labels {
                label(tab, "a tab")?;
            }
            if value.selected as usize >= value.labels.len() {
                return Err(format!("tabs {} select a missing tab", value.id));
            }
            Some(&value.id)
        }
        Widget::BeginTable(value) => {
            if value.columns.is_empty() || value.columns.len() > MAX_COLUMNS {
                return Err(format!("table {} has 1 to {MAX_COLUMNS} columns", value.id));
            }
            for column in &value.columns {
                label(column, "a column heading")?;
            }
            Some(&value.id)
        }
        Widget::Tooltip(value) => {
            label(value, "a tooltip")?;
            None
        }
        Widget::Icon(value) => {
            if value.size == 0 || value.size > MAX_ICON_SIZE {
                return Err(format!(
                    "icon {} is drawn 1 to {MAX_ICON_SIZE} wide",
                    value.icon
                ));
            }
            None
        }
        Widget::Separator
        | Widget::BeginRow
        | Widget::EndRow
        | Widget::EndSection
        | Widget::EndTable => None,
    }
    .map(String::as_str))
}

fn validate_number(value: &Number) -> Result<(), String> {
    let finite = [value.value, value.min, value.max, value.step]
        .iter()
        .all(|number| number.is_finite());
    if !finite
        || value.min > value.max
        || !(value.min..=value.max).contains(&value.value)
        || value.step < 0.0
        || value.decimals > MAX_DECIMALS
    {
        return Err(format!(
            "number {} needs finite min <= value <= max, a step of at least 0, and at most {MAX_DECIMALS} decimals",
            value.id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::bindings::angelica::plugin::ui::{Icon, Table, Tabs, Text, Tone};

    fn window(widgets: Vec<Widget>) -> Vec<Window> {
        vec![Window {
            id: "main".into(),
            title: "Main".into(),
            widgets,
        }]
    }

    fn text(value: &str) -> Widget {
        Widget::Text(Text {
            text: value.into(),
            tone: Tone::Normal,
        })
    }

    fn number(value: f64, min: f64, max: f64) -> Widget {
        Widget::Number(Number {
            id: "volume".into(),
            label: "Volume".into(),
            value,
            min,
            max,
            step: 0.5,
            decimals: 1,
            slider: true,
        })
    }

    fn table(columns: &[&str]) -> Widget {
        Widget::BeginTable(Table {
            id: "items".into(),
            columns: columns.iter().map(|&c| c.into()).collect(),
        })
    }

    #[test]
    fn numbers_tabs_tables_and_tooltips_follow_their_rules() {
        let valid = window(vec![
            number(2.0, 0.0, 10.0),
            Widget::Tooltip("How loud".into()),
            Widget::Icon(Icon {
                icon: 65002,
                size: 40,
            }),
            Widget::Tooltip("Gil".into()),
            Widget::Tabs(Tabs {
                id: "tabs".into(),
                labels: vec!["One".into(), "Two".into()],
                selected: 1,
            }),
            table(&["Item", "Count"]),
            text("Potion"),
            Widget::Tooltip("Restores HP".into()),
            Widget::BeginRow,
            text("3"),
            Widget::EndRow,
            Widget::EndTable,
        ]);
        assert_eq!(validate(&valid), Ok(()));

        for (widgets, why) in [
            (vec![number(11.0, 0.0, 10.0)], "value past max"),
            (vec![number(f64::NAN, 0.0, 10.0)], "not finite"),
            (vec![number(1.0, 5.0, 0.0)], "min past max"),
            (vec![Widget::Tooltip("alone".into())], "tooltip first"),
            (
                vec![
                    Widget::BeginRow,
                    Widget::Tooltip("x".into()),
                    Widget::EndRow,
                ],
                "tooltip after a begin",
            ),
            (vec![table(&[]), Widget::EndTable], "no columns"),
            (
                vec![table(&["a"; 17]), Widget::EndTable],
                "too many columns",
            ),
            (
                vec![
                    table(&["a"]),
                    table(&["b"]),
                    Widget::EndTable,
                    Widget::EndTable,
                ],
                "table in a table",
            ),
            (vec![table(&["a"]), Widget::EndRow], "mismatched end"),
            (
                vec![Widget::Icon(Icon { icon: 1, size: 0 })],
                "icon of no size",
            ),
            (
                vec![Widget::Icon(Icon { icon: 1, size: 257 })],
                "icon too large",
            ),
            (
                vec![Widget::Tabs(Tabs {
                    id: "tabs".into(),
                    labels: vec!["One".into()],
                    selected: 1,
                })],
                "missing tab",
            ),
        ] {
            assert!(validate(&window(widgets)).is_err(), "{why}");
        }
    }
}
