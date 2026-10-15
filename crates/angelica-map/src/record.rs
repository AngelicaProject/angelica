use std::fmt::Write as _;

use crate::Resolution;
use crate::{Absence, EntryKind, Map, Value};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecord {
    game_version: String,
    exe_sha256: String,
    entries_sha256: String,
    present: BTreeMap<String, u32>,
    absent: BTreeMap<String, String>,
    constant_offsets: Constants,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Constants {
    names: Vec<String>,
}

/// The result of resolving a map against one game executable, as committed
/// to `builds/<game version>.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRecord {
    pub game_version: String,
    pub exe_sha256: String,
    pub entries_sha256: String,
    pub resolution: Resolution,
}

impl BuildRecord {
    /// Checks record identity without resolving or scanning code.
    /// # Errors
    /// Rejects malformed record data.
    pub fn matches(text: &str, executable: &str, entries: &str) -> Result<bool, String> {
        let raw: RawRecord = toml::from_str(text).map_err(|e| e.to_string())?;
        Ok(raw.exe_sha256 == executable && raw.entries_sha256 == entries)
    }

    /// The constant offsets a record confirms for `executable`, with the
    /// values they were confirmed with; none when the record is of another
    /// executable.
    /// # Errors
    /// Rejects malformed record data.
    pub fn confirmed_offsets(
        text: &str,
        executable: &str,
    ) -> Result<BTreeMap<String, u32>, String> {
        let raw: RawRecord = toml::from_str(text).map_err(|e| e.to_string())?;
        if raw.exe_sha256 != executable {
            return Ok(BTreeMap::new());
        }
        raw.constant_offsets
            .names
            .into_iter()
            .map(|name| match raw.present.get(&name) {
                Some(value) => Ok((name, *value)),
                None => Err(format!("build record confirms absent entry {name}")),
            })
            .collect()
    }

    /// Reads an authenticated record, validating its completeness and image bounds.
    /// # Errors
    /// Rejects mismatched map hashes, missing/extra entries and invalid recorded addresses.
    pub fn from_toml(
        text: &str,
        map: &Map,
        image: &angelica_pe::Image<'_>,
    ) -> Result<Self, String> {
        Self::read(text, map, Some(image))
    }

    /// Reads a record against the map without the executable: every rule
    /// of [`BuildRecord::from_toml`] except that recorded addresses lie in
    /// the executable's image.
    /// # Errors
    /// Rejects mismatched map hashes, missing/extra entries, and constant
    /// offsets the map does not give.
    pub fn check(text: &str, map: &Map) -> Result<Self, String> {
        Self::read(text, map, None)
    }

    fn read(text: &str, map: &Map, image: Option<&angelica_pe::Image<'_>>) -> Result<Self, String> {
        let raw: RawRecord = toml::from_str(text).map_err(|e| e.to_string())?;
        if raw.game_version.trim().is_empty() {
            return Err("build record has no game_version".into());
        }
        if !valid_hash(&raw.exe_sha256) {
            return Err("build record exe_sha256 is not a SHA-256 hash".into());
        }
        if raw.entries_sha256 != map.entries_sha256() {
            return Err(format!(
                "build record is of other entries: its entries_sha256 is {}, the map's is {}",
                raw.entries_sha256,
                map.entries_sha256()
            ));
        }
        for name in raw.present.keys().chain(raw.absent.keys()) {
            if map.get(name).is_none() {
                return Err(format!("build record names unknown entry {name}"));
            }
        }
        let mut results = BTreeMap::new();
        let mut constants = Vec::new();
        for name in map.order() {
            let entry = map.get(name).ok_or("entry disappeared")?;
            let value = match (raw.present.get(name), raw.absent.get(name)) {
                (Some(value), None) => match &entry.kind {
                    EntryKind::Field { offset, .. } => {
                        if let Some(offset) = offset {
                            if value != offset {
                                return Err(format!("build record changed constant field {name}"));
                            }
                            constants.push(name.clone());
                        }
                        Value::Offset(*value)
                    }
                    EntryKind::Function { .. }
                    | EntryKind::Location { .. }
                    | EntryKind::Patch { .. }
                    | EntryKind::Vtable { .. }
                    | EntryKind::Global { .. } => {
                        if let Some(image) = image {
                            in_image(image, name, &entry.kind, *value)?;
                        }
                        Value::Address(*value)
                    }
                },
                (None, Some(reason)) if !reason.is_empty() => {
                    results.insert(name.clone(), Err(Absence::Recorded(reason.clone())));
                    continue;
                }
                _ => return Err(format!("build record must classify {name} exactly once")),
            };
            if entry
                .references()
                .iter()
                .any(|reference| !matches!(results.get(*reference), Some(Ok(_))))
            {
                return Err(format!(
                    "recorded present entry {name} depends on an absent entry"
                ));
            }
            results.insert(name.clone(), Ok(value));
        }
        constants.sort();
        if raw
            .constant_offsets
            .names
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != raw.constant_offsets.names.len()
            || raw.constant_offsets.names.iter().collect::<BTreeSet<_>>()
                != constants.iter().collect::<BTreeSet<_>>()
        {
            return Err("build record constant_offsets does not match the map".into());
        }
        Ok(Self {
            game_version: raw.game_version,
            exe_sha256: raw.exe_sha256,
            entries_sha256: raw.entries_sha256,
            resolution: Resolution {
                results,
                constant_offsets: constants,
            },
        })
    }
    /// The record as TOML. The text depends only on the record: tables and
    /// keys are in a fixed order and values in a fixed notation.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut text = String::new();
        text.push_str("# Generated by `angelica map verify`. Do not edit.\n");
        let _ = writeln!(text, "game_version = {}", quoted(&self.game_version));
        let _ = writeln!(text, "exe_sha256 = {}", quoted(&self.exe_sha256));
        let _ = writeln!(text, "entries_sha256 = {}", quoted(&self.entries_sha256));

        text.push_str("\n[present]\n");
        for (name, value) in self.resolution.present() {
            let _ = writeln!(text, "{} = {:#X}", quoted(name), value.get());
        }

        text.push_str("\n[absent]\n");
        for (name, absence) in self.resolution.absent() {
            let _ = writeln!(text, "{} = {}", quoted(name), quoted(&absence.to_string()));
        }

        text.push_str("\n[constant_offsets]\nnames = [");
        let names = self
            .resolution
            .constant_offsets()
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>();
        text.push_str(&names.join(", "));
        text.push_str("]\n");
        text
    }
}

/// Whether a recorded address lies where an entry of its kind must: code
/// for functions, locations, and patches, the image for vtables and
/// globals.
fn in_image(
    image: &angelica_pe::Image<'_>,
    name: &str,
    kind: &EntryKind,
    value: u32,
) -> Result<(), String> {
    match kind {
        EntryKind::Vtable { slots } => {
            let size = usize::try_from(*slots)
                .map_err(|e| e.to_string())?
                .checked_mul(8)
                .ok_or("vtable size overflow")?;
            if image.bytes_at(value, size).is_none() {
                return Err(format!("recorded vtable {name} is outside image"));
            }
        }
        EntryKind::Global { .. } => {
            if image.bytes_at(value, 1).is_none() {
                return Err(format!("recorded global {name} is outside image"));
            }
        }
        _ => {
            if !image.is_code(value) {
                return Err(format!("recorded entry {name} is outside code"));
            }
        }
    }
    Ok(())
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A TOML basic string.
fn quoted(value: &str) -> String {
    let mut text = String::with_capacity(value.len() + 2);
    text.push('"');
    for character in value.chars() {
        match character {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            character if character.is_control() => {
                let _ = write!(text, "\\u{:04X}", u32::from(character));
            }
            character => text.push(character),
        }
    }
    text.push('"');
    text
}
