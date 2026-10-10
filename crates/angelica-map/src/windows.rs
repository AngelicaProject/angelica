//! Window schemas: names for elements of the game's own windows, tied to
//! the exact layout files the names were checked against.
//!
//! The game builds a window from a layout file in its archives, which gives
//! every element its id. A schema lists the layout files it knows by their
//! fingerprint and the id of each name in each. A layout file it does not
//! know leaves the window's names absent, so a patch that changes a window
//! never makes a name mean another element.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// What an element is, as far as rules for it need to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    /// An element that shows text.
    Text,
    /// Any other element.
    Other,
}

impl ElementKind {
    /// The name the map uses: `text` or `other`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Other => "other",
        }
    }

    /// The kind a name stands for.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "text" => Some(Self::Text),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

/// 16 lowercase hexadecimal digits: the start of the SHA-256 of a layout
/// file's bytes.
#[must_use]
pub fn window_layout_fingerprint(layout_file: &[u8]) -> String {
    Sha256::digest(layout_file)[..8]
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A named element of a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedElement {
    pub kind: ElementKind,
    pub doc: String,
}

/// One layout file a schema was checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownWindowLayout {
    pub fingerprint: String,
    /// The id of every named element.
    pub elements: BTreeMap<String, u32>,
}

/// The names of a window's elements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSchema {
    /// The window's name in the game.
    pub window: String,
    pub doc: String,
    /// The path of the window's layout file in the game's archives.
    pub layout: String,
    pub elements: BTreeMap<String, NamedElement>,
    pub layouts: Vec<KnownWindowLayout>,
}

impl WindowSchema {
    /// The id of every named element when the game's layout file has
    /// `fingerprint`.
    ///
    /// # Errors
    /// Says that the schema does not know the layout file.
    pub fn resolve(&self, fingerprint: &str) -> Result<BTreeMap<String, u32>, String> {
        self.layouts
            .iter()
            .find(|known| known.fingerprint == fingerprint)
            .map(|known| known.elements.clone())
            .ok_or_else(|| {
                format!(
                    "the map does not know layout {fingerprint} of window {}",
                    self.window
                )
            })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchema {
    window: String,
    doc: String,
    layout: String,
    elements: BTreeMap<String, RawElement>,
    layouts: Vec<RawLayout>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawElement {
    kind: String,
    doc: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    fingerprint: String,
    elements: BTreeMap<String, u32>,
}

/// An element name: a lowercase letter, then lowercase letters, digits, and
/// underscores, at most 64 bytes.
fn is_valid_element_name(name: &str) -> bool {
    name.len() <= 64
        && name.starts_with(|first: char| first.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A window name as the game has it: ASCII letters, digits, and
/// underscores, at most 31 bytes.
fn is_valid_window_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 31
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A layout file's path: `ui/uld/`, a file name of ASCII letters, digits,
/// and underscores, and `.uld`.
fn is_valid_layout_path(path: &str) -> bool {
    path.len() <= 128
        && path
            .strip_prefix("ui/uld/")
            .and_then(|rest| rest.strip_suffix(".uld"))
            .is_some_and(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
}

/// Parses `windows/<window>.toml`.
pub(crate) fn parse(relative: &str, text: &str) -> Result<WindowSchema, String> {
    let named = relative
        .strip_prefix("windows/")
        .and_then(|rest| rest.strip_suffix(".toml"))
        .ok_or("a schema is windows/<window name>.toml")?;
    let raw = toml::from_str::<RawSchema>(text).map_err(|error| error.message().to_owned())?;
    if !is_valid_window_name(&raw.window) || raw.window != named {
        return Err(format!(
            "window {:?} must be a window name and match the file's name",
            raw.window
        ));
    }
    if raw.doc.trim().is_empty() {
        return Err("doc is empty".into());
    }
    if !is_valid_layout_path(&raw.layout) {
        return Err(format!(
            "layout {:?} must be a layout file, ui/uld/<name>.uld",
            raw.layout
        ));
    }
    if raw.elements.is_empty() {
        return Err("a schema names at least one element".into());
    }
    let mut elements = BTreeMap::new();
    for (name, element) in raw.elements {
        if !is_valid_element_name(&name) {
            return Err(format!("{name:?} is not a valid element name"));
        }
        let kind = ElementKind::parse(&element.kind)
            .ok_or_else(|| format!("element {name}: {:?} is not an element kind", element.kind))?;
        if element.doc.trim().is_empty() {
            return Err(format!("element {name}: doc is empty"));
        }
        elements.insert(
            name,
            NamedElement {
                kind,
                doc: element.doc,
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
        if layout.elements.keys().ne(elements.keys()) {
            return Err(format!(
                "layout {} gives an id to exactly the named elements",
                layout.fingerprint
            ));
        }
        let ids: BTreeSet<_> = layout.elements.values().collect();
        if ids.len() != layout.elements.len() || ids.contains(&0) {
            return Err(format!(
                "layout {} gives every name its own id above zero",
                layout.fingerprint
            ));
        }
        layouts.push(KnownWindowLayout {
            fingerprint: layout.fingerprint,
            elements: layout.elements,
        });
    }
    Ok(WindowSchema {
        window: raw.window,
        doc: raw.doc,
        layout: raw.layout,
        elements,
        layouts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"
window = "_Exp"
doc = "The experience bar."
layout = "ui/uld/Exp.uld"

[elements.text]
kind = "text"
doc = "The class, level, and experience."

[elements.bar]
kind = "other"
doc = "The bar itself."

[[layouts]]
fingerprint = "0123456789abcdef"
elements = { text = 4, bar = 6 }
"#;

    #[test]
    fn a_schema_gives_names_their_ids_in_a_known_layout_only() {
        let schema = parse("windows/_Exp.toml", SCHEMA).unwrap();
        assert_eq!(schema.layout, "ui/uld/Exp.uld");
        assert_eq!(schema.elements["text"].kind, ElementKind::Text);
        let ids = schema.resolve("0123456789abcdef").unwrap();
        assert_eq!(ids["text"], 4);
        assert_eq!(ids["bar"], 6);
        assert!(
            schema
                .resolve("fedcba9876543210")
                .unwrap_err()
                .contains("does not know layout")
        );
    }

    #[test]
    fn a_fingerprint_follows_every_byte_of_the_layout_file() {
        let fingerprint = window_layout_fingerprint(b"uldh0100");
        assert_eq!(fingerprint.len(), 16);
        assert_eq!(fingerprint, window_layout_fingerprint(b"uldh0100"));
        assert_ne!(fingerprint, window_layout_fingerprint(b"uldh0101"));
    }

    #[test]
    fn broken_schemas_are_refused() {
        let broken = [
            ("windows/Other.toml", SCHEMA.to_owned(), "match the file"),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("ui/uld/Exp.uld", "ui/icon/000000/000001.tex"),
                "layout file",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("kind = \"other\"", "kind = \"image\""),
                "element kind",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("bar = 6", "bar = 4"),
                "its own id",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("bar = 6", "bar = 0"),
                "its own id",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace(", bar = 6", ""),
                "exactly the named elements",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("0123456789abcdef", "0123456789ABCDEF"),
                "16 lowercase hexadecimal",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("The experience bar.", " "),
                "doc is empty",
            ),
            (
                "windows/_Exp.toml",
                SCHEMA.replace("[elements.text]", "[elements.Text]"),
                "valid element name",
            ),
        ];
        for (path, text, reason) in broken {
            let error = parse(path, &text).unwrap_err();
            assert!(error.contains(reason), "{error} lacks {reason}");
        }
    }
}
