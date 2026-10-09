//! State tables: game state plugins may read, as paths through map entries.
//!
//! A table belongs to a domain, the file it is in (`state/<domain>.toml`),
//! and each domain has its own read permission in the host. A table's
//! `object` path leads from a `global` entry to the object the table
//! describes; each field's `path` leads from that object to a value. A step
//! whose entry has the type `ptr` is followed through the pointer; a step of
//! type `struct` stays at the address. The last step of a field is the
//! value, read by its entry's type, or inline text when `text` is given.
//!
//! A list table describes the elements of an array in the object instead of
//! the object itself: one row per element, and each field's path leads from
//! the element.
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{EntryKind, ValueKind};

/// How a field's value is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Representation {
    /// A value of the last entry's type.
    Value(ValueKind),
    /// NUL-terminated UTF-8 of at most this many bytes, stored inline.
    Text(u16),
}

/// A field of a state table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateField {
    pub name: String,
    pub doc: String,
    /// Entry names from the table's object to the value.
    pub path: Vec<String>,
    pub representation: Representation,
}

/// The array a list table reads its rows from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateList {
    /// A field of the object: the array (`struct`) or a pointer to it
    /// (`ptr`).
    pub at: String,
    /// The most elements read.
    pub length: u16,
    /// Bytes from one element to the next.
    pub stride: u32,
    /// Whether each element is a pointer to the row's object; zero pointers
    /// are skipped.
    pub pointers: bool,
    /// A field of the object holding how many elements are used.
    pub count: Option<String>,
    /// A field of the table: an element whose value of it is zero or none
    /// is skipped.
    pub present: Option<String>,
}

/// The implicit field of a list table: the element's position in its array.
pub const INDEX: &str = "index";

/// A table of a state domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTable {
    /// `<domain>` or `<domain>.<subject>`.
    pub name: String,
    pub domain: String,
    pub doc: String,
    /// Entry names from a global to the object; the table has one row while
    /// the path ends at a non-zero address, none otherwise.
    pub object: Vec<String>,
    /// The array whose elements are the rows; `None` for a table of one
    /// object.
    pub list: Option<StateList>,
    pub fields: BTreeMap<String, StateField>,
}

impl StateTable {
    /// Every entry the table's object and fields refer to.
    pub fn references(&self) -> impl Iterator<Item = &str> {
        self.object
            .iter()
            .chain(
                self.list
                    .iter()
                    .flat_map(|list| std::iter::once(&list.at).chain(&list.count)),
            )
            .chain(self.fields.values().flat_map(|field| field.path.iter()))
            .map(String::as_str)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    tables: BTreeMap<String, RawTable>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    doc: String,
    object: Vec<String>,
    list: Option<RawList>,
    fields: BTreeMap<String, RawField>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawList {
    at: String,
    length: u16,
    stride: Option<u32>,
    #[serde(default)]
    pointers: bool,
    count: Option<String>,
    present: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawField {
    doc: String,
    path: Vec<String>,
    text: Option<u16>,
}

/// A lowercase letter, then lowercase letters, digits, and underscores, at
/// most 64 bytes.
fn is_valid_word(word: &str) -> bool {
    word.len() <= 64
        && word.starts_with(|first: char| first.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Parses and checks a table's `list` without looking at the entries.
fn parse_list(
    name: &str,
    list: RawList,
    fields: &BTreeMap<String, RawField>,
) -> Result<StateList, String> {
    if !(1..=1024).contains(&list.length) {
        return Err(format!("table {name}: a list has 1 to 1024 elements"));
    }
    let stride = match (list.pointers, list.stride) {
        (true, None | Some(8)) => 8,
        (true, Some(_)) => {
            return Err(format!(
                "table {name}: a list of pointers has a stride of 8"
            ));
        }
        (false, Some(stride)) if (1..=0x1_0000).contains(&stride) => stride,
        (false, _) => {
            return Err(format!(
                "table {name}: a list has a stride of 1 to 65536 bytes"
            ));
        }
    };
    if list
        .present
        .as_ref()
        .is_some_and(|present| !fields.contains_key(present))
    {
        return Err(format!("table {name}: present names none of its fields"));
    }
    Ok(StateList {
        at: list.at,
        length: list.length,
        stride,
        pointers: list.pointers,
        count: list.count,
        present: list.present,
    })
}

/// Parses `state/<domain>.toml` without looking at the entries.
pub(crate) fn parse(relative: &str, text: &str) -> Result<Vec<StateTable>, String> {
    let domain = relative
        .strip_prefix("state/")
        .and_then(|rest| rest.strip_suffix(".toml"))
        .filter(|domain| is_valid_word(domain))
        .ok_or("state tables are in state/<domain>.toml, the domain a lowercase word")?;
    let raw = toml::from_str::<RawFile>(text).map_err(|error| error.message().to_owned())?;
    if raw.tables.is_empty() {
        return Err("the file defines no table".into());
    }
    let mut tables = Vec::new();
    for (name, table) in raw.tables {
        let mut parts = name.split('.');
        if parts.next() != Some(domain) || parts.clone().count() > 1 || !parts.all(is_valid_word) {
            return Err(format!(
                "table {name:?} is named `{domain}` or `{domain}.<subject>`"
            ));
        }
        if table.doc.trim().is_empty() {
            return Err(format!("table {name}: doc is empty"));
        }
        if table.object.is_empty() {
            return Err(format!("table {name}: the object path is empty"));
        }
        if table.fields.is_empty() {
            return Err(format!("table {name}: no fields"));
        }
        let list = table
            .list
            .map(|list| parse_list(&name, list, &table.fields))
            .transpose()?;
        let mut fields = BTreeMap::new();
        for (field_name, field) in table.fields {
            if field_name == INDEX && list.is_some() {
                return Err(format!("table {name}: `{INDEX}` is the element's position"));
            }
            if !is_valid_word(&field_name) {
                return Err(format!(
                    "table {name}: {field_name:?} is not a valid field name"
                ));
            }
            if field.doc.trim().is_empty() {
                return Err(format!("table {name}, field {field_name}: doc is empty"));
            }
            if field.path.is_empty() {
                return Err(format!(
                    "table {name}, field {field_name}: the path is empty"
                ));
            }
            if field
                .text
                .is_some_and(|length| length == 0 || length > 4096)
            {
                return Err(format!(
                    "table {name}, field {field_name}: text is 1 to 4096 bytes"
                ));
            }
            fields.insert(
                field_name.clone(),
                StateField {
                    name: field_name,
                    doc: field.doc,
                    path: field.path,
                    // Checked against the entries in `check`.
                    representation: field
                        .text
                        .map_or(Representation::Value(ValueKind::Void), Representation::Text),
                },
            );
        }
        tables.push(StateTable {
            name,
            domain: domain.to_owned(),
            doc: table.doc,
            object: table.object,
            list,
            fields,
        });
    }
    Ok(tables)
}

/// Checks a table's paths against the entries and fills in each field's
/// value type.
pub(crate) fn check(
    table: &mut StateTable,
    entries: &BTreeMap<String, crate::Entry>,
) -> Result<(), String> {
    let kind = |name: &str| -> Result<&EntryKind, String> {
        entries
            .get(name)
            .map(|entry| &entry.kind)
            .ok_or_else(|| format!("table {}: `{name}` is not an entry", table.name))
    };
    let step_value = |name: &str, first: bool| -> Result<ValueKind, String> {
        match (kind(name)?, first) {
            (EntryKind::Global { value }, true) | (EntryKind::Field { value, .. }, false) => {
                Ok(*value)
            }
            (_, true) => Err(format!(
                "table {}: the object path starts with a global, not `{name}`",
                table.name
            )),
            (_, false) => Err(format!("table {}: `{name}` is not a field", table.name)),
        }
    };
    for (index, name) in table.object.iter().enumerate() {
        let value = step_value(name, index == 0)?;
        if !matches!(value, ValueKind::Ptr | ValueKind::Struct) {
            return Err(format!(
                "table {}: object step `{name}` is neither ptr nor struct",
                table.name
            ));
        }
    }
    if let Some(list) = &table.list {
        let at = step_value(&list.at, false)?;
        if !matches!(at, ValueKind::Ptr | ValueKind::Struct) {
            return Err(format!(
                "table {}: the list `{}` is neither ptr nor struct",
                table.name, list.at
            ));
        }
        if let Some(count) = &list.count
            && !matches!(
                step_value(count, false)?,
                ValueKind::U8
                    | ValueKind::U16
                    | ValueKind::U32
                    | ValueKind::I8
                    | ValueKind::I16
                    | ValueKind::I32
            )
        {
            return Err(format!(
                "table {}: the count `{count}` is not an integer of at most 32 bits",
                table.name
            ));
        }
    }
    for field in table.fields.values_mut() {
        let (last, steps) = field.path.split_last().expect("paths are not empty");
        for name in steps {
            let value = step_value(name, false)?;
            if !matches!(value, ValueKind::Ptr | ValueKind::Struct) {
                return Err(format!(
                    "table {}, field {}: step `{name}` is neither ptr nor struct",
                    table.name, field.name
                ));
            }
        }
        let value = step_value(last, false)?;
        field.representation = match field.representation {
            Representation::Text(length) if value == ValueKind::Struct => {
                Representation::Text(length)
            }
            Representation::Text(_) => {
                return Err(format!(
                    "table {}, field {}: text is read from a struct field",
                    table.name, field.name
                ));
            }
            Representation::Value(_) if matches!(value, ValueKind::Ptr | ValueKind::Struct) => {
                return Err(format!(
                    "table {}, field {}: `{last}` is not a value; text needs `text`",
                    table.name, field.name
                ));
            }
            Representation::Value(_) => Representation::Value(value),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Map;

    const ENTRIES: &str = r#"
[global."object.local_player"]
locator = { signature = "48 8B 05 ?? ?? ?? ??", at = 3, follow = "rel32" }
type = "ptr"

[field."object.character.data"]
of = "object.character"
type = "struct"
offset = 0x1A0

[field."object.character_data.level"]
of = "object.character_data"
type = "u8"
offset = 0x2B

[field."object.character.name"]
of = "object.character"
type = "struct"
offset = 0x30

[field."object.character.home"]
of = "object.character"
type = "ptr"
offset = 0x8
"#;

    const SELF: &str = r#"
[tables.self]
doc = "The player's character."
object = ["object.local_player"]

[tables.self.fields.level]
doc = "The level."
path = ["object.character.data", "object.character_data.level"]

[tables.self.fields.name]
doc = "The name."
path = ["object.character.name"]
text = 64
"#;

    fn map(state: &str) -> Result<Map, crate::MapError> {
        Map::from_sources(
            "format = 1",
            &[
                ("entries/object.toml".into(), ENTRIES.into()),
                ("state/self.toml".into(), state.into()),
            ],
        )
    }

    #[test]
    fn tables_resolve_their_value_types_from_the_entries() {
        let map = map(SELF).unwrap();
        let table = map.state_table("self").unwrap();
        assert_eq!(table.domain, "self");
        assert_eq!(
            table.fields["level"].representation,
            super::Representation::Value(crate::ValueKind::U8)
        );
        assert_eq!(
            table.fields["name"].representation,
            super::Representation::Text(64)
        );
    }

    const LIST: &str = r#"
[tables."self.statuses"]
doc = "Statuses."
object = ["object.local_player"]
list = { at = "object.character.data", length = 30, stride = 16, present = "level" }

[tables."self.statuses".fields.level]
doc = "Level."
path = ["object.character_data.level"]
"#;

    #[test]
    fn list_tables_describe_their_array() {
        let loaded = map(&format!("{SELF}{LIST}")).unwrap();
        let list = loaded
            .state_table("self.statuses")
            .unwrap()
            .list
            .clone()
            .unwrap();
        assert_eq!((list.length, list.stride, list.pointers), (30, 16, false));
        assert_eq!(list.present.as_deref(), Some("level"));
        for (from, to) in [
            ("length = 30", "length = 0"),
            ("stride = 16", "stride = 0"),
            ("stride = 16", "pointers = true, stride = 16"),
            ("present = \"level\"", "present = \"other\""),
            (
                "at = \"object.character.data\"",
                "at = \"object.character_data.level\"",
            ),
            ("present = \"level\"", "count = \"object.character.name\""),
            (
                "[tables.\"self.statuses\".fields.level]",
                "[tables.\"self.statuses\".fields.index]",
            ),
        ] {
            assert!(
                map(&format!("{SELF}{}", LIST.replace(from, to))).is_err(),
                "{to}"
            );
        }
        assert!(
            map(&format!(
                "{SELF}{}",
                LIST.replace("stride = 16, ", "pointers = true, ")
            ))
            .is_ok()
        );
    }

    #[test]
    fn invalid_tables_are_refused() {
        for (from, to) in [
            ("[tables.self]", "[tables.party]"),
            ("[tables.self]\n", "[tables.self]\nextra = 1\n"),
            (
                "object = [\"object.local_player\"]",
                "object = [\"object.character.data\"]",
            ),
            (
                "object = [\"object.local_player\"]",
                "object = [\"object.nothing\"]",
            ),
            ("text = 64", ""),
            ("text = 64", "text = 0"),
            (
                "path = [\"object.character.data\", \"object.character_data.level\"]",
                "path = [\"object.character_data.level\", \"object.character.data\"]",
            ),
            (
                "path = [\"object.character.data\", \"object.character_data.level\"]",
                "path = [\"object.character.data\", \"object.character_data.level\"]\ntext = 8",
            ),
            ("doc = \"The level.\"", "doc = \"\""),
            ("[tables.self.fields.level]", "[tables.self.fields.Level]"),
        ] {
            assert!(map(&SELF.replace(from, to)).is_err(), "{to}");
        }
        let subject = SELF
            .replace("[tables.self]", "[tables.\"self.home\"]")
            .replace("[tables.self.fields", "[tables.\"self.home\".fields");
        assert_eq!(
            map(&subject)
                .unwrap()
                .state_table("self.home")
                .unwrap()
                .domain,
            "self"
        );
    }
}
