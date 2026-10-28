use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use toml::{Table, Value as Toml};

use crate::resolve::{Absence, Resolution, Value};
use crate::{Anatomia, Kind};

/// The result of resolving a description against one game executable, as
/// committed to `builds/<game version>.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRecord {
    pub game_version: String,
    pub exe_sha256: String,
    pub anatomia_sha256: String,
    pub resolution: Resolution,
}

struct Raw {
    game_version: String,
    exe_sha256: String,
    anatomia_sha256: String,
    present: BTreeMap<String, u32>,
    absent: BTreeMap<String, String>,
    confirmed: Vec<String>,
}

fn parse(text: &str) -> Result<Raw, String> {
    let mut table: Table = text
        .parse()
        .map_err(|error: toml::de::Error| error.message().to_owned())?;
    let mut string = |key: &str| match table.remove(key) {
        Some(Toml::String(value)) => Ok(value),
        _ => Err(format!("build record needs `{key}`")),
    };
    let game_version = string("game_version")?;
    let exe_sha256 = string("exe_sha256")?;
    let anatomia_sha256 = string("anatomia_sha256")?;
    let present = match table.remove("present") {
        Some(Toml::Table(present)) => present
            .into_iter()
            .map(|(name, value)| match value {
                Toml::Integer(value) => u32::try_from(value)
                    .map(|value| (name.clone(), value))
                    .map_err(|_| format!("present `{name}` is not 0 to {}", u32::MAX)),
                _ => Err(format!("present `{name}` is not a number")),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("build record needs `[present]`".to_owned()),
    };
    let absent = match table.remove("absent") {
        Some(Toml::Table(absent)) => absent
            .into_iter()
            .map(|(name, value)| match value {
                Toml::String(reason) if !reason.is_empty() => Ok((name, reason)),
                _ => Err(format!("absent `{name}` needs a reason")),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("build record needs `[absent]`".to_owned()),
    };
    let confirmed = match table.remove("confirmed") {
        Some(Toml::Table(mut confirmed)) => {
            let offsets = match confirmed.remove("offsets") {
                Some(Toml::Array(offsets)) => offsets
                    .into_iter()
                    .map(|name| match name {
                        Toml::String(name) => Ok(name),
                        _ => Err("`confirmed.offsets` holds names".to_owned()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err("build record needs `confirmed.offsets`".to_owned()),
            };
            if let Some(key) = confirmed.keys().next() {
                return Err(format!("unknown key `confirmed.{key}`"));
            }
            offsets
        }
        _ => return Err("build record needs `[confirmed]`".to_owned()),
    };
    if let Some(key) = table.keys().next() {
        return Err(format!("unknown key `{key}`"));
    }
    if !valid_hash(&exe_sha256) || !valid_hash(&anatomia_sha256) {
        return Err("build record hashes are SHA-256 in lowercase hexadecimal".to_owned());
    }
    Ok(Raw {
        game_version,
        exe_sha256,
        anatomia_sha256,
        present,
        absent,
        confirmed,
    })
}

impl BuildRecord {
    /// The constant offsets a record confirms for `executable`, with the
    /// offsets they were confirmed with; none for another executable.
    ///
    /// # Errors
    ///
    /// A record that is not one.
    pub fn confirmed_offsets(
        text: &str,
        executable: &str,
    ) -> Result<BTreeMap<String, u32>, String> {
        let raw = parse(text)?;
        if raw.exe_sha256 != executable {
            return Ok(BTreeMap::new());
        }
        raw.confirmed
            .into_iter()
            .map(|name| match raw.present.get(&name) {
                Some(offset) => Ok((name, *offset)),
                None => Err(format!("build record confirms absent entry {name}")),
            })
            .collect()
    }

    /// Reads a record of `anatomia`: every entry classified once, the
    /// description's hash, and constant offsets as the description gives
    /// them.
    ///
    /// # Errors
    ///
    /// A record of another description, or one that does not fit it.
    pub fn read(text: &str, anatomia: &Anatomia) -> Result<Self, String> {
        let raw = parse(text)?;
        if raw.anatomia_sha256 != anatomia.sha256() {
            return Err(format!(
                "build record is of another description: its anatomia_sha256 is {}, the description's is {}",
                raw.anatomia_sha256,
                anatomia.sha256()
            ));
        }
        let names: BTreeSet<&String> = anatomia.entries.keys().chain(&anatomia.unread).collect();
        let recorded: BTreeSet<&String> = raw.present.keys().chain(raw.absent.keys()).collect();
        if names != recorded || raw.present.len() + raw.absent.len() != names.len() {
            return Err("build record must classify every entry exactly once".to_owned());
        }
        let mut results = BTreeMap::new();
        for (name, value) in &raw.present {
            let entry = anatomia
                .entries
                .get(name)
                .ok_or_else(|| format!("build record finds {name}, which cannot be read"))?;
            let value = match &entry.kind {
                Kind::Field { at, .. } => {
                    if at.is_some_and(|at| at != *value) {
                        return Err(format!("build record changed constant field {name}"));
                    }
                    Value::Offset(*value)
                }
                _ => Value::Address(*value),
            };
            results.insert(name.clone(), Ok(value));
        }
        for (name, reason) in raw.absent {
            results.insert(name, Err(Absence(reason)));
        }
        let mut confirmed = raw.confirmed;
        confirmed.sort();
        let constants: Vec<String> = raw
            .present
            .keys()
            .filter(|name| {
                matches!(
                    anatomia.entries[*name].kind,
                    Kind::Field { at: Some(_), .. }
                )
            })
            .cloned()
            .collect();
        if confirmed != constants {
            return Err(
                "build record `confirmed.offsets` does not match its constant fields".to_owned(),
            );
        }
        Ok(Self {
            game_version: raw.game_version,
            exe_sha256: raw.exe_sha256,
            anatomia_sha256: raw.anatomia_sha256,
            resolution: Resolution { results, confirmed },
        })
    }

    /// The record as TOML, the same text for the same record.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut text = String::from("# Generated by `angelica anatomia verify`. Do not edit.\n");
        let _ = writeln!(text, "game_version = {}", quoted(&self.game_version));
        let _ = writeln!(text, "exe_sha256 = {}", quoted(&self.exe_sha256));
        let _ = writeln!(text, "anatomia_sha256 = {}", quoted(&self.anatomia_sha256));
        text.push_str("\n[present]\n");
        for (name, value) in self.resolution.present() {
            let _ = writeln!(text, "{} = {:#X}", quoted(name), value.get());
        }
        text.push_str("\n[absent]\n");
        for (name, absence) in self.resolution.absent() {
            let _ = writeln!(text, "{} = {}", quoted(name), quoted(&absence.0));
        }
        text.push_str("\n[confirmed]\noffsets = [");
        let names: Vec<String> = self
            .resolution
            .confirmed()
            .iter()
            .map(|name| quoted(name))
            .collect();
        text.push_str(&names.join(", "));
        text.push_str("]\n");
        text
    }
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A TOML basic string.
pub(crate) fn quoted(value: &str) -> String {
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
