//! Sheet schemas: names for the columns of the game's Excel sheets, tied to
//! the exact layouts of the sheet the names were checked against.
//!
//! A sheet's layout — its variant and every column's type and offset — comes
//! from the game's sheet header. A schema lists the layouts it knows by their
//! fingerprint and the column index of each name in each layout. A layout it
//! does not know leaves the sheet's names absent, so a patch that moves
//! columns never makes a name read another column.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The type of a column, as the game's sheet header gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColumnType {
    Text,
    Bool,
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Int64,
    Uint64,
    /// One bit, 0 to 7, of a byte.
    PackedBool(u8),
}

impl ColumnType {
    /// The name the map uses: `text`, `uint16`, `packed_bool_3`, and so on.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Text => "text".into(),
            Self::Bool => "bool".into(),
            Self::Int8 => "int8".into(),
            Self::Uint8 => "uint8".into(),
            Self::Int16 => "int16".into(),
            Self::Uint16 => "uint16".into(),
            Self::Int32 => "int32".into(),
            Self::Uint32 => "uint32".into(),
            Self::Float32 => "float32".into(),
            Self::Int64 => "int64".into(),
            Self::Uint64 => "uint64".into(),
            Self::PackedBool(bit) => format!("packed_bool_{bit}"),
        }
    }

    /// The type a name stands for.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "text" => Self::Text,
            "bool" => Self::Bool,
            "int8" => Self::Int8,
            "uint8" => Self::Uint8,
            "int16" => Self::Int16,
            "uint16" => Self::Uint16,
            "int32" => Self::Int32,
            "uint32" => Self::Uint32,
            "float32" => Self::Float32,
            "int64" => Self::Int64,
            "uint64" => Self::Uint64,
            _ => {
                let bit = name.strip_prefix("packed_bool_")?.parse::<u8>().ok()?;
                if bit > 7 {
                    return None;
                }
                Self::PackedBool(bit)
            }
        })
    }
}

/// A sheet's layout in one game build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetLayout {
    /// Whether rows have subrows.
    pub subrows: bool,
    /// Every column's type and byte offset, in column order.
    pub columns: Vec<(ColumnType, u16)>,
}

impl SheetLayout {
    /// 16 lowercase hexadecimal digits: the start of the SHA-256 of
    /// `rows` or `subrows`, then `;<type>@<offset>` for every column.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut text = String::from(if self.subrows { "subrows" } else { "rows" });
        for (column_type, offset) in &self.columns {
            let _ = write!(text, ";{}@{offset}", column_type.name());
        }
        let digest = Sha256::digest(text.as_bytes());
        digest[..8].iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
    }
}

/// A named column of a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedColumn {
    pub column_type: ColumnType,
    pub doc: String,
}

/// One layout a schema was checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownLayout {
    pub fingerprint: String,
    /// The index of every named column.
    pub columns: BTreeMap<String, u16>,
}

/// The names of a sheet's columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetSchema {
    pub sheet: String,
    pub doc: String,
    pub columns: BTreeMap<String, NamedColumn>,
    pub layouts: Vec<KnownLayout>,
}

impl SheetSchema {
    /// The index of every named column in `layout`.
    ///
    /// # Errors
    /// Says why the names do not apply: the schema does not know the
    /// layout, or a column does not have its type there.
    pub fn resolve(&self, layout: &SheetLayout) -> Result<BTreeMap<String, u16>, String> {
        let fingerprint = layout.fingerprint();
        let known = self
            .layouts
            .iter()
            .find(|known| known.fingerprint == fingerprint)
            .ok_or_else(|| {
                format!(
                    "the map does not know layout {fingerprint} of sheet {}",
                    self.sheet
                )
            })?;
        for (name, index) in &known.columns {
            let expected = self.columns[name].column_type;
            match layout.columns.get(usize::from(*index)) {
                Some((found, _)) if *found == expected => {}
                Some((found, _)) => {
                    return Err(format!(
                        "column {name} of sheet {} is {} at {index}, not {}",
                        self.sheet,
                        found.name(),
                        expected.name()
                    ));
                }
                None => {
                    return Err(format!(
                        "sheet {} has no column {index} for {name}",
                        self.sheet
                    ));
                }
            }
        }
        Ok(known.columns.clone())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchema {
    sheet: String,
    doc: String,
    columns: BTreeMap<String, RawColumn>,
    layouts: Vec<RawLayout>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawColumn {
    #[serde(rename = "type")]
    column_type: String,
    doc: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    fingerprint: String,
    columns: BTreeMap<String, u16>,
}

/// A column name: a lowercase letter, then lowercase letters, digits, and
/// underscores, at most 64 bytes.
fn is_valid_column_name(name: &str) -> bool {
    name.len() <= 64
        && name.starts_with(|first: char| first.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A sheet name as the game lists it: ASCII letters, digits, and
/// underscores in parts separated by `/`.
fn is_valid_sheet_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.split('/').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Parses `sheets/<sheet>.toml`.
///
/// # Errors
///
/// Says which rule of the schema the text breaks.
pub fn parse(relative: &str, text: &str) -> Result<SheetSchema, String> {
    let named = relative
        .strip_prefix("sheets/")
        .and_then(|rest| rest.strip_suffix(".toml"))
        .ok_or("a schema is sheets/<sheet name>.toml")?;
    let raw = toml::from_str::<RawSchema>(text).map_err(|error| error.message().to_owned())?;
    if !is_valid_sheet_name(&raw.sheet) || raw.sheet != named {
        return Err(format!(
            "sheet {:?} must be a sheet name and match the file's path",
            raw.sheet
        ));
    }
    if raw.doc.trim().is_empty() {
        return Err("doc is empty".into());
    }
    if raw.columns.is_empty() {
        return Err("a schema names at least one column".into());
    }
    let mut columns = BTreeMap::new();
    for (name, column) in raw.columns {
        if !is_valid_column_name(&name) {
            return Err(format!("{name:?} is not a valid column name"));
        }
        let column_type = ColumnType::parse(&column.column_type).ok_or_else(|| {
            format!(
                "column {name}: {:?} is not a column type",
                column.column_type
            )
        })?;
        if column.doc.trim().is_empty() {
            return Err(format!("column {name}: doc is empty"));
        }
        columns.insert(
            name,
            NamedColumn {
                column_type,
                doc: column.doc,
            },
        );
    }
    if raw.layouts.is_empty() {
        return Err("a schema lists at least one layout".into());
    }
    let mut fingerprints = BTreeSet::new();
    let mut layouts = Vec::new();
    for layout in raw.layouts {
        if layout.fingerprint.len() != 16
            || !layout
                .fingerprint
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(format!(
                "layout {:?}: a fingerprint is 16 lowercase hexadecimal digits",
                layout.fingerprint
            ));
        }
        if !fingerprints.insert(layout.fingerprint.clone()) {
            return Err(format!("layout {} is listed twice", layout.fingerprint));
        }
        if layout.columns.keys().ne(columns.keys()) {
            return Err(format!(
                "layout {} gives an index to exactly the named columns",
                layout.fingerprint
            ));
        }
        let indexes: BTreeSet<_> = layout.columns.values().collect();
        if indexes.len() != layout.columns.len() {
            return Err(format!(
                "layout {} gives two names the same index",
                layout.fingerprint
            ));
        }
        layouts.push(KnownLayout {
            fingerprint: layout.fingerprint,
            columns: layout.columns,
        });
    }
    Ok(SheetSchema {
        sheet: raw.sheet,
        doc: raw.doc,
        columns,
        layouts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> SheetLayout {
        SheetLayout {
            subrows: false,
            columns: vec![
                (ColumnType::Text, 0),
                (ColumnType::Uint16, 8),
                (ColumnType::PackedBool(2), 12),
            ],
        }
    }

    fn schema(fingerprint: &str) -> String {
        format!(
            r#"
sheet = "Item"
doc = "Items."

[columns.singular]
type = "text"
doc = "The name."

[columns.icon]
type = "uint16"
doc = "The icon."

[[layouts]]
fingerprint = "{fingerprint}"
columns = {{ singular = 0, icon = 1 }}
"#
        )
    }

    #[test]
    fn types_have_stable_names() {
        for column_type in [
            ColumnType::Text,
            ColumnType::Bool,
            ColumnType::Int8,
            ColumnType::Uint64,
            ColumnType::Float32,
            ColumnType::PackedBool(7),
        ] {
            assert_eq!(ColumnType::parse(&column_type.name()), Some(column_type));
        }
        assert_eq!(ColumnType::parse("packed_bool_8"), None);
        assert_eq!(ColumnType::parse("string"), None);
    }

    #[test]
    fn fingerprints_change_with_any_column() {
        let base = layout().fingerprint();
        assert_eq!(base.len(), 16);
        assert_eq!(base, layout().fingerprint(), "stable");
        let mut moved = layout();
        moved.columns[1].1 = 10;
        let mut retyped = layout();
        retyped.columns[1].0 = ColumnType::Int16;
        let mut subrows = layout();
        subrows.subrows = true;
        let mut inserted = layout();
        inserted.columns.insert(1, (ColumnType::Uint8, 4));
        for other in [moved, retyped, subrows, inserted] {
            assert_ne!(other.fingerprint(), base);
        }
    }

    #[test]
    fn names_resolve_only_in_a_known_layout() {
        let parsed = parse("sheets/Item.toml", &schema(&layout().fingerprint())).unwrap();
        let columns = parsed.resolve(&layout()).unwrap();
        assert_eq!(columns["singular"], 0);
        assert_eq!(columns["icon"], 1);

        let mut patched = layout();
        patched.columns.insert(0, (ColumnType::Uint32, 0));
        let error = parsed.resolve(&patched).unwrap_err();
        assert!(error.contains("does not know layout"), "{error}");
    }

    #[test]
    fn a_layout_whose_types_disagree_is_refused() {
        let mut wrong = layout();
        wrong.columns.swap(0, 1);
        let parsed = parse("sheets/Item.toml", &schema(&wrong.fingerprint())).unwrap();
        let error = parsed.resolve(&wrong).unwrap_err();
        assert!(
            error.contains("column icon of sheet Item is text at 1, not uint16"),
            "{error}"
        );
    }

    #[test]
    fn invalid_schemas_are_refused() {
        let good = schema("0123456789abcdef");
        assert!(parse("sheets/Item.toml", &good).is_ok());
        for (path, text) in [
            ("sheets/Other.toml", good.clone()),
            ("sheets/Item.toml", good.replace("singular = 0, ", "")),
            ("sheets/Item.toml", good.replace("icon = 1", "icon = 0")),
            ("sheets/Item.toml", good.replace("0123456789abcdef", "XYZ")),
            ("sheets/Item.toml", good.replace("\"uint16\"", "\"u16\"")),
            (
                "sheets/Item.toml",
                good.replace("[columns.icon]", "[columns.Icon]"),
            ),
            (
                "sheets/Item.toml",
                good.replace("doc = \"The icon.\"", "doc = \" \""),
            ),
            (
                "sheets/Item.toml",
                good.replace("doc = \"Items.\"", "doc = \"Items.\"\nextra = 1"),
            ),
            (
                "sheets/Item.toml",
                format!(
                    "{good}\n[[layouts]]\nfingerprint = \"0123456789abcdef\"\ncolumns = {{ singular = 0, icon = 1 }}\n"
                ),
            ),
        ] {
            assert!(parse(path, &text).is_err(), "{path}: {text}");
        }
    }
}
