//! Game state plugins read (the `state` interface): the game map's state
//! tables, their paths resolved to addresses and offsets for the running
//! build, and reading them through the game's memory. Platform
//! independent: the Windows adapter only supplies the memory reads.
#![forbid(unsafe_code)]
use crate::{
    catalog::Catalog,
    error::{Error, Kind, Result},
    manifest::Permission,
};
use angelica_map::{Map, Representation, ValueKind};
use std::{
    collections::BTreeMap,
    sync::{PoisonError, RwLock},
};

/// A value read from the game.
#[derive(Debug, Clone, PartialEq)]
pub enum StateValue {
    Boolean(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    /// Text as the game stores it, without its terminator.
    Text(Vec<u8>),
}

/// One step of a path: add `offset` (the first step's is an absolute
/// address), then read a pointer there when `follow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub offset: u64,
    pub follow: bool,
}

/// How a field is read in the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPlan {
    pub doc: String,
    /// Steps from the object to the value; absent with the reason when an
    /// entry on the path is absent in this build.
    pub steps: std::result::Result<Vec<Step>, String>,
    pub representation: Representation,
    /// The implicit field of a list table: the element's position.
    pub index: bool,
}

/// How a list table's array is read in the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPlan {
    /// From the object to the array: through a pointer when `follow`.
    pub at: Step,
    pub length: u16,
    pub stride: u32,
    pub pointers: bool,
    /// The offset from the object of the count of used elements, and its
    /// type.
    pub count: Option<(u64, ValueKind)>,
    /// The field whose zero or missing value skips an element.
    pub present: Option<String>,
}

/// A state table resolved for the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePlan {
    pub name: String,
    pub domain: String,
    pub doc: String,
    /// Steps from nothing to the object; absent with the reason when an
    /// entry on the path is absent in this build.
    pub object: std::result::Result<Vec<Step>, String>,
    /// The array whose elements are the rows; `None` for one object.
    pub list: Option<ListPlan>,
    pub fields: BTreeMap<String, FieldPlan>,
}

/// The permission that guards a state domain; a domain this host does not
/// know has none, and its tables are not offered.
#[must_use]
pub fn permission(domain: &str) -> Option<Permission> {
    match domain {
        "self" => Some(Permission::StateSelf),
        _ => None,
    }
}

/// The steps of a path through `catalog`, from an absolute address of the
/// first entry when `first_is_global`.
fn steps(
    map: &Map,
    catalog: &Catalog,
    base: u64,
    path: &[String],
    first_is_global: bool,
) -> std::result::Result<Vec<Step>, String> {
    path.iter()
        .enumerate()
        .map(|(index, name)| {
            let resolved = catalog
                .present
                .get(name)
                .ok_or_else(|| format!("map entry {name} is absent"))?;
            let follow = map.get(name).is_some_and(|entry| match &entry.kind {
                angelica_map::EntryKind::Global { value }
                | angelica_map::EntryKind::Field { value, .. } => *value == ValueKind::Ptr,
                _ => false,
            });
            let offset = match resolved.value {
                angelica_map::Value::Address(rva) if index == 0 && first_is_global => {
                    base + u64::from(rva)
                }
                angelica_map::Value::Offset(offset) => u64::from(offset),
                angelica_map::Value::Address(_) => {
                    return Err(format!("map entry {name} is not a field"));
                }
            };
            // The last step of a field is its value, never followed.
            let last = index + 1 == path.len() && !first_is_global;
            Ok(Step {
                offset,
                follow: follow && !last,
            })
        })
        .collect()
}

/// One field entry as a step: its offset, followed when it is a pointer.
fn field_step(
    map: &Map,
    catalog: &Catalog,
    name: &str,
) -> std::result::Result<(Step, ValueKind), String> {
    let resolved = catalog
        .present
        .get(name)
        .ok_or_else(|| format!("map entry {name} is absent"))?;
    let angelica_map::Value::Offset(offset) = resolved.value else {
        return Err(format!("map entry {name} is not a field"));
    };
    let kind = match map.get(name).map(|entry| &entry.kind) {
        Some(angelica_map::EntryKind::Field { value, .. }) => *value,
        _ => return Err(format!("map entry {name} is not a field")),
    };
    Ok((
        Step {
            offset: u64::from(offset),
            follow: kind == ValueKind::Ptr,
        },
        kind,
    ))
}

/// A list table's array resolved for the build.
fn list_plan(
    map: &Map,
    catalog: &Catalog,
    list: &angelica_map::StateList,
) -> std::result::Result<ListPlan, String> {
    let (at, _) = field_step(map, catalog, &list.at)?;
    let count = list
        .count
        .as_ref()
        .map(|name| field_step(map, catalog, name).map(|(step, kind)| (step.offset, kind)))
        .transpose()?;
    Ok(ListPlan {
        at,
        length: list.length,
        stride: list.stride,
        pointers: list.pointers,
        count,
        present: list.present.clone(),
    })
}

/// The game map's state tables for the running build.
#[derive(Default)]
pub struct StateTables {
    tables: RwLock<BTreeMap<String, TablePlan>>,
}

impl StateTables {
    /// The map's tables resolved against `catalog` for a game loaded at
    /// `base`. Tables of domains this host has no permission for are left
    /// out.
    #[must_use]
    pub fn from_map(map: &Map, catalog: &Catalog, base: u64) -> Self {
        let tables = Self::default();
        for table in map.state_tables() {
            if permission(&table.domain).is_none() {
                continue;
            }
            let list = table
                .list
                .as_ref()
                .map(|list| list_plan(map, catalog, list))
                .transpose();
            let object = steps(map, catalog, base, &table.object, true)
                .and_then(|object| list.as_ref().map(|_| object).map_err(Clone::clone));
            let mut fields: BTreeMap<String, FieldPlan> = table
                .fields
                .values()
                .map(|field| {
                    (
                        field.name.clone(),
                        FieldPlan {
                            doc: field.doc.clone(),
                            steps: steps(map, catalog, base, &field.path, false),
                            representation: field.representation,
                            index: false,
                        },
                    )
                })
                .collect();
            if table.list.is_some() {
                fields.insert(
                    angelica_map::INDEX.to_owned(),
                    FieldPlan {
                        doc: "The element's position in its array, from 0.".into(),
                        steps: Ok(Vec::new()),
                        representation: Representation::Value(ValueKind::U32),
                        index: true,
                    },
                );
            }
            tables.insert(TablePlan {
                name: table.name.clone(),
                domain: table.domain.clone(),
                doc: table.doc.clone(),
                object,
                list: list.ok().flatten(),
                fields,
            });
        }
        tables
    }

    /// Adds or replaces a table.
    pub fn insert(&self, plan: TablePlan) {
        self.tables
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(plan.name.clone(), plan);
    }

    /// Every table, sorted by name.
    #[must_use]
    pub fn tables(&self) -> Vec<TablePlan> {
        self.tables
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// A table and the plans of `fields`, in the order given.
    ///
    /// # Errors
    /// `not-found` for an unknown table or field; `unavailable` when the
    /// table's object or a field is absent in this build.
    pub fn plan(&self, table: &str, fields: &[String]) -> Result<(TablePlan, Vec<FieldPlan>)> {
        let tables = self.tables.read().unwrap_or_else(PoisonError::into_inner);
        let plan = tables.get(table).ok_or_else(|| {
            Error::new(
                Kind::NotFound,
                format!("the game map has no state table {table}"),
            )
        })?;
        if let Err(reason) = &plan.object {
            return Err(Error::new(
                Kind::Unavailable,
                format!("state table {table}: {reason}"),
            ));
        }
        let fields = fields
            .iter()
            .map(|name| {
                let field = plan.fields.get(name).ok_or_else(|| {
                    Error::new(
                        Kind::NotFound,
                        format!("state table {table} has no field {name}"),
                    )
                })?;
                if let Err(reason) = &field.steps {
                    return Err(Error::new(
                        Kind::Unavailable,
                        format!("state table {table}, field {name}: {reason}"),
                    ));
                }
                Ok(field.clone())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((plan.clone(), fields))
    }
}

/// Reads of the game's memory that fail instead of faulting.
pub trait Memory {
    /// Fills `buffer` from `address`; false when the range is not readable.
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool;
}

fn pointer(memory: &dyn Memory, address: u64) -> Option<u64> {
    let mut bytes = [0u8; 8];
    memory
        .read(address, &mut bytes)
        .then(|| u64::from_le_bytes(bytes))
        .filter(|&value| value != 0)
}

/// Walks `steps` from `start`: `None` when a pointer on the way is zero or
/// unreadable.
fn walk(memory: &dyn Memory, start: u64, steps: &[Step]) -> Option<u64> {
    let mut address = start;
    for step in steps {
        address = address.checked_add(step.offset)?;
        if step.follow {
            address = pointer(memory, address)?;
        }
    }
    Some(address)
}

fn value(memory: &dyn Memory, address: u64, representation: Representation) -> Option<StateValue> {
    let read = |length: usize| {
        let mut bytes = vec![0u8; length];
        memory.read(address, &mut bytes).then_some(bytes)
    };
    let fixed = |length: usize| -> Option<[u8; 8]> {
        let bytes = read(length)?;
        let mut out = [0u8; 8];
        out[..length].copy_from_slice(&bytes);
        Some(out)
    };
    Some(match representation {
        Representation::Text(length) => {
            let mut bytes = read(usize::from(length))?;
            if let Some(end) = bytes.iter().position(|&b| b == 0) {
                bytes.truncate(end);
            }
            StateValue::Text(bytes)
        }
        Representation::Value(kind) => match kind {
            ValueKind::Bool => StateValue::Boolean(fixed(1)?[0] != 0),
            ValueKind::U8 => StateValue::Unsigned(u64::from(fixed(1)?[0])),
            ValueKind::U16 => StateValue::Unsigned(u64::from_le_bytes(fixed(2)?)),
            ValueKind::U32 => StateValue::Unsigned(u64::from_le_bytes(fixed(4)?)),
            ValueKind::U64 => StateValue::Unsigned(u64::from_le_bytes(fixed(8)?)),
            ValueKind::I8 => StateValue::Signed(i64::from(i8::from_le_bytes([fixed(1)?[0]]))),
            ValueKind::I16 => {
                let b = fixed(2)?;
                StateValue::Signed(i64::from(i16::from_le_bytes([b[0], b[1]])))
            }
            ValueKind::I32 => {
                let b = fixed(4)?;
                StateValue::Signed(i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            }
            ValueKind::I64 => StateValue::Signed(i64::from_le_bytes(fixed(8)?)),
            ValueKind::F32 => {
                let b = fixed(4)?;
                StateValue::Float(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            }
            ValueKind::F64 => StateValue::Float(f64::from_le_bytes(fixed(8)?)),
            ValueKind::Ptr | ValueKind::Struct | ValueKind::Void => return None,
        },
    })
}

/// The value of `field` in the object or element at `start`, the element
/// at `index` of its array.
fn field_value(
    memory: &dyn Memory,
    start: u64,
    index: usize,
    field: &FieldPlan,
) -> Option<StateValue> {
    if field.index {
        return Some(StateValue::Unsigned(index as u64));
    }
    let steps = field.steps.as_ref().ok()?;
    let address = walk(memory, start, steps)?;
    value(memory, address, field.representation)
}

/// The addresses and positions of a list's used elements.
fn elements(
    memory: &dyn Memory,
    table: &TablePlan,
    list: &ListPlan,
    object: u64,
) -> Vec<(usize, u64)> {
    let Some(base) = walk(memory, object, &[list.at]) else {
        return Vec::new();
    };
    let mut used = usize::from(list.length);
    if let Some((offset, kind)) = list.count {
        let count = object
            .checked_add(offset)
            .and_then(|address| value(memory, address, Representation::Value(kind)));
        used = match count {
            Some(StateValue::Unsigned(count)) => used.min(usize::try_from(count).unwrap_or(used)),
            Some(StateValue::Signed(count)) => used.min(usize::try_from(count).unwrap_or(0)),
            _ => 0,
        };
    }
    let present = list
        .present
        .as_ref()
        .and_then(|name| table.fields.get(name));
    (0..used)
        .filter_map(|index| {
            let mut element = base.checked_add(u64::from(list.stride) * index as u64)?;
            if list.pointers {
                element = pointer(memory, element)?;
            }
            if let Some(present) = present {
                match field_value(memory, element, index, present)? {
                    StateValue::Unsigned(0)
                    | StateValue::Signed(0)
                    | StateValue::Boolean(false) => {
                        return None;
                    }
                    StateValue::Text(text) if text.is_empty() => return None,
                    _ => {}
                }
            }
            Some((index, element))
        })
        .collect()
}

/// The rows of a table with the values of `fields`: for a table of one
/// object, none while the object does not exist and one otherwise; for a
/// list, one per used element. A value is `None` when a pointer on its path
/// is zero or its memory is unreadable.
#[must_use]
pub fn read(
    memory: &dyn Memory,
    table: &TablePlan,
    fields: &[FieldPlan],
) -> Vec<Vec<Option<StateValue>>> {
    let Ok(object_steps) = &table.object else {
        return Vec::new();
    };
    let Some(object) = walk(memory, 0, object_steps) else {
        return Vec::new();
    };
    let rows = match &table.list {
        None => vec![(0, object)],
        Some(list) => elements(memory, table, list, object),
    };
    rows.into_iter()
        .map(|(index, start)| {
            fields
                .iter()
                .map(|field| field_value(memory, start, index, field))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Memory of one buffer at address 0x1000.
    struct Buffer(Vec<u8>);

    impl Memory for Buffer {
        fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
            let Some(start) = address
                .checked_sub(0x1000)
                .and_then(|a| usize::try_from(a).ok())
            else {
                return false;
            };
            match self.0.get(start..start + buffer.len()) {
                Some(bytes) => {
                    buffer.copy_from_slice(bytes);
                    true
                }
                None => false,
            }
        }
    }

    const MAP: &str = r#"
[global."object.local_player"]
locator = { signature = "C3" }
type = "ptr"

[field."object.character.data"]
of = "object.character"
type = "struct"
offset = 0x20

[field."object.character_data.level"]
of = "object.character_data"
type = "u8"
offset = 0x2

[field."object.character_data.health"]
of = "object.character_data"
type = "u32"
offset = 0x4

[field."object.character.name"]
of = "object.character"
type = "struct"
offset = 0x0

[field."object.character.x"]
of = "object.character"
type = "f32"
offset = 0x10

[field."object.character.target"]
of = "object.character"
type = "ptr"
offset = 0x18

[field."object.character.missing"]
of = "object.character"
type = "u8"
offset = 0x1
"#;

    const SELF: &str = r#"
[tables.self]
doc = "The player."
object = ["object.local_player"]

[tables.self.fields.level]
doc = "Level."
path = ["object.character.data", "object.character_data.level"]

[tables.self.fields.health]
doc = "HP."
path = ["object.character.data", "object.character_data.health"]

[tables.self.fields.name]
doc = "Name."
path = ["object.character.name"]
text = 8

[tables.self.fields.x]
doc = "X."
path = ["object.character.x"]

[tables.self.fields.target_level]
doc = "The target's level."
path = ["object.character.target", "object.character.data", "object.character_data.level"]

[tables.self.fields.missing]
doc = "Absent."
path = ["object.character.missing"]
"#;

    fn tables() -> StateTables {
        let map = Map::from_sources(
            "format = 1",
            &[
                ("entries/object.toml".into(), MAP.into()),
                ("state/self.toml".into(), SELF.into()),
            ],
        )
        .unwrap();
        let mut catalog = Catalog::default();
        for name in [
            "object.local_player",
            "object.character.data",
            "object.character_data.level",
            "object.character_data.health",
            "object.character.name",
            "object.character.x",
            "object.character.target",
        ] {
            let entry = map.get(name).unwrap().clone();
            let value = match &entry.kind {
                angelica_map::EntryKind::Field { offset, .. } => {
                    angelica_map::Value::Offset(offset.unwrap())
                }
                // The global holding the player's pointer is at 0x1000.
                _ => angelica_map::Value::Address(0x1000 - 0x800),
            };
            catalog.present.insert(
                name.into(),
                crate::catalog::ResolvedEntry {
                    entry,
                    value,
                    function_range: None,
                },
            );
        }
        StateTables::from_map(&map, &catalog, 0x800)
    }

    /// The global at 0x1000 points to the player at 0x1010.
    fn memory(player: bool) -> Buffer {
        let mut bytes = vec![0u8; 0x60];
        if player {
            bytes[..8].copy_from_slice(&0x1010u64.to_le_bytes());
        }
        let player = 0x10;
        bytes[player..player + 6].copy_from_slice(b"Alisae");
        bytes[player + 0x10..player + 0x14].copy_from_slice(&1.5f32.to_le_bytes());
        bytes[player + 0x22] = 90;
        bytes[player + 0x24..player + 0x28].copy_from_slice(&12345u32.to_le_bytes());
        Buffer(bytes)
    }

    fn names(fields: &[&str]) -> Vec<String> {
        fields.iter().map(|f| (*f).to_owned()).collect()
    }

    #[test]
    fn paths_are_walked_through_pointers_and_structs() {
        let tables = tables();
        let (table, fields) = tables
            .plan(
                "self",
                &names(&["level", "health", "name", "x", "target_level"]),
            )
            .unwrap();
        let rows = read(&memory(true), &table, &fields);
        assert_eq!(
            rows,
            [vec![
                Some(StateValue::Unsigned(90)),
                Some(StateValue::Unsigned(12345)),
                Some(StateValue::Text(b"Alisae".to_vec())),
                Some(StateValue::Float(1.5)),
                None,
            ]],
            "the target pointer is zero"
        );
        assert!(
            read(&memory(false), &table, &fields).is_empty(),
            "no player"
        );
    }

    const LIST_ENTRIES: &str = r#"
[global."object.local_player"]
locator = { signature = "C3" }
type = "ptr"

[field."object.character.statuses"]
of = "object.character"
type = "struct"
offset = 0x10

[field."object.character.status_count"]
of = "object.character"
type = "u8"
offset = 0x8

[field."object.status.id"]
of = "object.status"
type = "u16"
offset = 0x0

[field."object.status.time"]
of = "object.status"
type = "f32"
offset = 0x4
"#;

    const LIST: &str = r#"
[tables."self.statuses"]
doc = "Statuses."
object = ["object.local_player"]
list = { at = "object.character.statuses", length = 4, stride = 8, count = "object.character.status_count", present = "id" }

[tables."self.statuses".fields.id]
doc = "Id."
path = ["object.status.id"]

[tables."self.statuses".fields.time]
doc = "Time."
path = ["object.status.time"]
"#;

    #[test]
    fn lists_read_one_row_per_used_element() {
        let map = Map::from_sources(
            "format = 1",
            &[
                ("entries/object.toml".into(), LIST_ENTRIES.into()),
                ("state/self.toml".into(), LIST.into()),
            ],
        )
        .unwrap();
        let mut catalog = Catalog::default();
        for entry in map.entries() {
            let value = match &entry.kind {
                angelica_map::EntryKind::Field { offset, .. } => {
                    angelica_map::Value::Offset(offset.unwrap())
                }
                _ => angelica_map::Value::Address(0x1000 - 0x800),
            };
            catalog.present.insert(
                entry.name.clone(),
                crate::catalog::ResolvedEntry {
                    entry: entry.clone(),
                    value,
                    function_range: None,
                },
            );
        }
        let tables = StateTables::from_map(&map, &catalog, 0x800);

        // The player at 0x1008: three used statuses of four, the second
        // empty; the fourth is past the count.
        let mut bytes = vec![0u8; 0x60];
        bytes[..8].copy_from_slice(&0x1008u64.to_le_bytes());
        let player = 0x8;
        bytes[player + 0x8] = 3;
        for (index, id, time) in [
            (0usize, 48u16, 12.5f32),
            (1, 0, 0.0),
            (2, 1199, 30.0),
            (3, 9, 1.0),
        ] {
            let at = player + 0x10 + index * 8;
            bytes[at..at + 2].copy_from_slice(&id.to_le_bytes());
            bytes[at + 4..at + 8].copy_from_slice(&time.to_le_bytes());
        }
        let (table, fields) = tables
            .plan("self.statuses", &names(&["index", "id", "time"]))
            .unwrap();
        assert_eq!(
            read(&Buffer(bytes), &table, &fields),
            [
                vec![
                    Some(StateValue::Unsigned(0)),
                    Some(StateValue::Unsigned(48)),
                    Some(StateValue::Float(12.5)),
                ],
                vec![
                    Some(StateValue::Unsigned(2)),
                    Some(StateValue::Unsigned(1199)),
                    Some(StateValue::Float(30.0)),
                ],
            ]
        );
        assert!(tables.tables()[0].fields.contains_key("index"), "listed");
    }

    #[test]
    fn unknown_and_absent_names_are_errors() {
        let tables = tables();
        let error = tables.plan("party", &[]).unwrap_err();
        assert_eq!(error.kind, Kind::NotFound);
        let error = tables.plan("self", &names(&["mana"])).unwrap_err();
        assert_eq!(error.kind, Kind::NotFound);
        let error = tables.plan("self", &names(&["missing"])).unwrap_err();
        assert_eq!(error.kind, Kind::Unavailable);
        assert!(
            error.message.contains("object.character.missing is absent"),
            "{}",
            error.message
        );
    }
}
