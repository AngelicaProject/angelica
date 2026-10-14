//! Changes: plugins set fields of the game's objects that the game map marks
//! as changeable, values that exist on the player's computer alone, such as
//! the camera's zoom limits. The host writes on the game's main thread,
//! keeps each changed value's original, and puts it back when the plugin
//! restores it or stops. A value is put back only where the plugin's path
//! still reaches the same place and the value is still the one written: the
//! game may have moved its object or set the value itself meanwhile.
//! Platform independent: the Windows adapter only writes.
#![forbid(unsafe_code)]
use super::act::encode;
use super::plan::{Prepared, Target, prepare_set};
use super::read::{Memory, Value, objects};
use super::schema::Schema;
use crate::error::{Error, Kind, Result};
use angelica_map::{Number, Param, Shape, ValueKind};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// The most objects one change reaches.
pub const MOST_CHANGED: usize = 256;
/// The most paths one plugin keeps changed.
pub const MOST_PATHS: usize = 64;

/// The game, as a change needs it: its memory, read and written.
pub trait Writer {
    fn memory(&self) -> &dyn Memory;

    /// Writes `bytes` at `address`; false when the range is not writable
    /// data of the game.
    fn write(&self, address: u64, bytes: &[u8]) -> bool;
}

/// A change checked against the map, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    pub text: String,
    plan: Prepared,
    target: Target,
    bytes: Vec<u8>,
}

/// Checks a plugin's change of the value at path `text` to `value`.
/// `may_read` and `may_change` say whether the plugin may read and change a
/// domain.
///
/// # Errors
/// The planner's errors for the path; `permission-denied` for a field the
/// map does not let plugins change or a domain the plugin may not change;
/// `invalid-argument` for a value that does not fit.
pub fn prepare(
    schema: &Schema,
    text: &str,
    value: &Value,
    may_read: &dyn Fn(&str) -> bool,
    may_change: &dyn Fn(&str) -> bool,
) -> Result<Ready> {
    // Planned with every domain first, so a mistake names the change; the
    // domains are checked after.
    let (plan, target) = prepare_set(schema, text, &|_| true)?;
    let field = &target.field;
    let name = format!("{}.{}", target.owner, field.name);
    let Some(settable) = &field.set else {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!("{name} is not a value plugins change"),
        ));
    };
    if !may_change(&field.domain) {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "{name} is in domain {}, which the plugin may not change: list it in the manifest's [lens] change",
                field.domain
            ),
        ));
    }
    if let Some(domain) = plan
        .domains
        .iter()
        .find(|domain| !may_read(domain) && !may_change(domain))
    {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "the path to {name} goes through {domain}, which the plugin may neither read nor change: list it in the manifest's [lens] read"
            ),
        ));
    }
    let Shape::Value {
        kind,
        meaning,
        unit,
    } = &field.shape
    else {
        return Err(Error::invalid(format!("{name} is not a value")));
    };
    let param = Param {
        doc: field.doc.clone(),
        kind: *kind,
        meaning: meaning.clone(),
        unit: unit.clone(),
        default: None,
        range: settable.range,
    };
    let number = encode(schema, &name, &param, value)?;
    let bytes = bytes(*kind, number);
    Ok(Ready {
        text: text.to_owned(),
        plan,
        target,
        bytes,
    })
}

/// A number as the bytes of its kind, little-endian.
fn bytes(kind: ValueKind, number: Number) -> Vec<u8> {
    let integer = match number {
        Number::Integer(value) => value,
        #[allow(clippy::cast_possible_truncation)] // a float written to an integer field
        Number::Float(_) => number.float() as i64,
    };
    match kind {
        #[allow(clippy::cast_possible_truncation)] // an f32 field
        ValueKind::F32 => (number.float() as f32).to_le_bytes().to_vec(),
        ValueKind::F64 => number.float().to_le_bytes().to_vec(),
        ValueKind::I8 | ValueKind::U8 | ValueKind::Bool => integer.to_le_bytes()[..1].to_vec(),
        ValueKind::I16 | ValueKind::U16 => integer.to_le_bytes()[..2].to_vec(),
        ValueKind::I32 | ValueKind::U32 => integer.to_le_bytes()[..4].to_vec(),
        _ => integer.to_le_bytes().to_vec(),
    }
}

/// One changed value: whose it is, what it was, and what was written.
struct Changed {
    plugin: String,
    original: Vec<u8>,
    written: Vec<u8>,
}

/// Every value plugins changed, by address, and the paths that reach them.
#[derive(Default)]
pub struct Changes {
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    values: BTreeMap<u64, Changed>,
    /// Each plugin's changes, as planned, to find their places again.
    paths: BTreeMap<String, Vec<Ready>>,
}

impl Changes {
    /// Writes a ready change for `plugin`. Returns how many values it
    /// changed: none while its path reaches no object.
    ///
    /// # Errors
    /// `limit-exceeded` past [`MOST_CHANGED`] objects or [`MOST_PATHS`]
    /// paths; `unavailable` for a value another plugin changed or memory
    /// that cannot be written.
    pub fn set(&self, plugin: &str, ready: &Ready, game: &dyn Writer) -> Result<usize> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let addresses = places(game.memory(), ready)?;
        for address in &addresses {
            if let Some(changed) = held.values.get(address)
                && changed.plugin != plugin
            {
                return Err(Error::new(
                    Kind::Unavailable,
                    format!("{} is changed by another plugin", ready.text),
                ));
            }
        }
        let paths = held.paths.entry(plugin.to_owned()).or_default();
        if !paths.iter().any(|known| known.text == ready.text) {
            if paths.len() >= MOST_PATHS {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!("a plugin keeps at most {MOST_PATHS} paths changed; restore one first"),
                ));
            }
            paths.push(ready.clone());
        }
        for address in &addresses {
            let original = if let Some(changed) = held.values.get(address) {
                changed.original.clone()
            } else {
                let mut original = vec![0; ready.bytes.len()];
                if !game.memory().read(*address, &mut original) {
                    return Err(unwritable(&ready.text));
                }
                original
            };
            if !game.write(*address, &ready.bytes) {
                return Err(unwritable(&ready.text));
            }
            held.values.insert(
                *address,
                Changed {
                    plugin: plugin.to_owned(),
                    original,
                    written: ready.bytes.clone(),
                },
            );
        }
        Ok(addresses.len())
    }

    /// Puts back what `plugin` changed through the path `text`.
    pub fn restore(&self, plugin: &str, text: &str, game: &dyn Writer) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(paths) = held.paths.get_mut(plugin) else {
            return;
        };
        let restored: Vec<Ready> = paths
            .iter()
            .filter(|ready| ready.text == text)
            .cloned()
            .collect();
        paths.retain(|ready| ready.text != text);
        for ready in restored {
            put_back(&mut held, plugin, &ready, game);
        }
    }

    /// Puts back everything a stopped plugin changed.
    pub fn remove_plugin(&self, plugin: &str, game: &dyn Writer) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        for ready in held.paths.remove(plugin).unwrap_or_default() {
            put_back(&mut held, plugin, &ready, game);
        }
        held.values.retain(|_, changed| changed.plugin != plugin);
    }
}

/// The addresses of the value in every object the change's path reaches.
fn places(memory: &dyn Memory, ready: &Ready) -> Result<Vec<u64>> {
    let reached = objects(memory, &ready.plan, 0)?;
    if reached.len() > MOST_CHANGED {
        return Err(Error::new(
            Kind::LimitExceeded,
            format!("a change reaches at most {MOST_CHANGED} objects"),
        ));
    }
    Ok(reached
        .into_iter()
        .map(|(_, object, _)| object + u64::from(ready.target.offset))
        .collect())
}

/// Puts back the originals of `plugin`'s values that the path still
/// reaches and that still hold what was written.
fn put_back(held: &mut Held, plugin: &str, ready: &Ready, game: &dyn Writer) {
    let Ok(addresses) = places(game.memory(), ready) else {
        return;
    };
    for address in addresses {
        let Some(changed) = held.values.get(&address) else {
            continue;
        };
        if changed.plugin != plugin {
            continue;
        }
        let mut current = vec![0; changed.written.len()];
        if game.memory().read(address, &mut current) && current == changed.written {
            game.write(address, &changed.original);
        }
        held.values.remove(&address);
    }
}

fn unwritable(text: &str) -> Error {
    Error::new(
        Kind::Unavailable,
        format!("{text} cannot be written in the game's memory now"),
    )
}
