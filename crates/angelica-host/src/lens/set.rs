//! Changes: plugins set fields of the game's objects that the game map marks
//! as changeable, values that exist on the player's computer alone, such as
//! the camera's zoom limits or what the game's windows show. The host
//! changes them on the game's main thread, by writing memory or through the
//! game's own function the map names, keeps each changed value's original,
//! and puts it back when the plugin restores it or stops. A value is put
//! back only where the plugin's path still reaches the same place and the
//! value is still the one written: the game may have moved its object or
//! set the value itself meanwhile. A value the map marks held is one the
//! game sets again itself, such as a window's text: every frame the host
//! applies the plugin's value again wherever the path reaches, taking what
//! the game wrote as the new original. Platform independent: the Windows
//! adapter only writes and calls.
#![forbid(unsafe_code)]
use super::act::encode;
use super::plan::{Prepared, Target, prepare_set};
use super::read::{Memory, Value, current, objects};
use super::schema::Schema;
use crate::error::{Error, Kind, Result};
use angelica_map::{Number, Param, Shape, ValueKind};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

/// The most objects one change reaches.
pub const MOST_CHANGED: usize = 256;
/// The most paths one plugin keeps changed.
pub const MOST_PATHS: usize = 64;

/// A value handed to the game's function that sets a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    /// A number or a `bool`, as the 64 bits of its argument.
    Number(u64),
    /// Text without its terminator; the function gets it NUL-terminated.
    Text(Vec<u8>),
}

/// The game, as a change needs it: its memory, read and written, and the
/// functions that set values.
pub trait Writer {
    fn memory(&self) -> &dyn Memory;

    /// Writes `bytes` at `address`; false when the range is not writable
    /// data of the game.
    fn write(&self, address: u64, bytes: &[u8]) -> bool;

    /// Calls the game's function at `function` with the object at `object`
    /// and `value`, on the main thread; false when it cannot be called.
    fn call_setter(&self, function: u64, object: u64, value: &Setting) -> bool;
}

/// How a ready change reaches the game.
#[derive(Debug, Clone, PartialEq)]
enum How {
    /// These bytes written at the field's offset.
    Memory(Vec<u8>),
    /// The value handed to the function at `function`; `written` is the
    /// value as it reads back.
    Call {
        function: u64,
        setting: Setting,
        written: Vec<u8>,
    },
}

/// A change checked against the map, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    pub text: String,
    plan: Prepared,
    target: Target,
    how: How,
    hold: bool,
}

/// Checks a plugin's change of the value at path `text` to `value`.
/// `may_read` and `may_change` say whether the plugin may read and change a
/// domain.
///
/// # Errors
/// The planner's errors for the path; `permission-denied` for a field the
/// map does not let plugins change or a domain the plugin may not change;
/// `invalid-argument` for a value that does not fit; `unavailable` for a
/// setting function the build lacks.
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
    if !may_change(&target.domain) {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "{name} is in domain {}, which the plugin may not change: list it in the manifest's [lens] change",
                target.domain
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
    let function = settable
        .call
        .as_ref()
        .map(|call| {
            schema
                .code
                .get(call)
                .cloned()
                .unwrap_or_else(|| Err("not in the schema".into()))
                .map_err(|reason| {
                    Error::new(
                        Kind::Unavailable,
                        format!("{name} cannot be changed in this game build: {call}: {reason}"),
                    )
                })
        })
        .transpose()?;
    let how = match (&field.shape, function) {
        (Shape::Text(length), Some(function)) => {
            let text = checked_text(&name, *length, value)?;
            How::Call {
                function,
                setting: Setting::Text(text.clone()),
                written: text,
            }
        }
        (
            Shape::Value {
                kind,
                meaning,
                unit,
            },
            function,
        ) => {
            let param = Param {
                doc: field.doc.clone(),
                kind: *kind,
                meaning: meaning.clone(),
                unit: unit.clone(),
                default: None,
                range: settable.range,
            };
            let number = encode(schema, &name, &param, value)?;
            match function {
                None => How::Memory(bytes(*kind, number)),
                Some(function) => How::Call {
                    function,
                    setting: Setting::Number(bits(*kind, number)),
                    written: read_back(*kind, number),
                },
            }
        }
        _ => return Err(Error::invalid(format!("{name} is not a value"))),
    };
    let hold = settable.hold;
    Ok(Ready {
        text: text.to_owned(),
        plan,
        target,
        how,
        hold,
    })
}

/// A plugin's text for a text field of at most `length` bytes: UTF-8
/// shorter than that, without control characters, so it never carries the
/// game's macros.
fn checked_text(name: &str, length: u16, value: &Value) -> Result<Vec<u8>> {
    let Value::Text(bytes) = value else {
        return Err(Error::invalid(format!("{name} is text")));
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(Error::invalid(format!("the text for {name} is not UTF-8")));
    };
    if text.chars().any(char::is_control) {
        return Err(Error::invalid(format!(
            "the text for {name} has control characters"
        )));
    }
    if bytes.len() >= usize::from(length) {
        return Err(Error::new(
            Kind::LimitExceeded,
            format!(
                "the text for {name} is longer than {} bytes",
                length.saturating_sub(1)
            ),
        ));
    }
    Ok(bytes.clone())
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

/// A number as the 64 bits of a function's argument of its kind.
fn bits(kind: ValueKind, number: Number) -> u64 {
    let mut padded = [0u8; 8];
    let bytes = bytes(kind, number);
    padded[..bytes.len()].copy_from_slice(&bytes);
    u64::from_le_bytes(padded)
}

/// A number as [`current`] reads it back from a field of its kind.
fn read_back(kind: ValueKind, number: Number) -> Vec<u8> {
    match kind {
        ValueKind::Bool => vec![u8::from(number.float() != 0.0)],
        #[allow(clippy::cast_possible_truncation)] // an f32 field
        ValueKind::F32 => f64::from(number.float() as f32).to_le_bytes().to_vec(),
        ValueKind::F64 => number.float().to_le_bytes().to_vec(),
        _ => {
            let bytes = bytes(kind, number);
            let mut padded = [0u8; 8];
            padded[..bytes.len()].copy_from_slice(&bytes);
            let integer = match kind {
                ValueKind::I8 => i128::from(i8::from_le_bytes([padded[0]])),
                ValueKind::I16 => i128::from(i16::from_le_bytes([padded[0], padded[1]])),
                ValueKind::I32 => i128::from(i32::from_le_bytes([
                    padded[0], padded[1], padded[2], padded[3],
                ])),
                ValueKind::I64 => i128::from(i64::from_le_bytes(padded)),
                _ => i128::from(u64::from_le_bytes(padded)),
            };
            integer.to_le_bytes().to_vec()
        }
    }
}

/// What a value read back by [`current`] is handed to its setting function
/// as, to put it back.
fn setting_of(ready: &Ready, value: &[u8]) -> Setting {
    let kind = match &ready.target.field.shape {
        Shape::Value { kind, .. } => *kind,
        _ => return Setting::Text(value.to_vec()),
    };
    let number = match kind {
        ValueKind::Bool => Number::Integer(i64::from(value.first().copied().unwrap_or(0))),
        ValueKind::F32 | ValueKind::F64 => {
            Number::Float(f64::from_le_bytes(value.try_into().unwrap_or([0; 8])).to_bits())
        }
        _ => Number::Integer(
            i64::try_from(i128::from_le_bytes(value.try_into().unwrap_or([0; 16])))
                .unwrap_or_default(),
        ),
    };
    Setting::Number(bits(kind, number))
}

/// One changed value: whose it is, through which path, in which object,
/// what it was, and what was written.
struct Changed {
    plugin: String,
    path: String,
    object: u64,
    original: Vec<u8>,
    written: Vec<u8>,
}

/// Every value plugins changed, by the address of the field, and the paths
/// that reach them.
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

impl Ready {
    /// The value the field holds in the object at `object` now, as the
    /// change compares it; `None` when it cannot be read.
    fn now(&self, game: &dyn Writer, object: u64) -> Option<Vec<u8>> {
        match &self.how {
            How::Memory(bytes) => {
                let mut now = vec![0; bytes.len()];
                game.memory()
                    .read(object + u64::from(self.target.offset), &mut now)
                    .then_some(now)
            }
            How::Call { .. } => current(game.memory(), &self.target.read, object),
        }
    }

    /// The value as the change writes it, as [`Ready::now`] reads it.
    fn written(&self) -> &[u8] {
        match &self.how {
            How::Memory(bytes) => bytes,
            How::Call { written, .. } => written,
        }
    }

    /// Puts `value`, read by [`Ready::now`], in the object at `object`.
    fn put(&self, game: &dyn Writer, object: u64, value: &[u8]) -> bool {
        match &self.how {
            How::Memory(_) => game.write(object + u64::from(self.target.offset), value),
            How::Call { function, .. } => {
                game.call_setter(*function, object, &setting_of(self, value))
            }
        }
    }

    /// Writes the change in the object at `object`.
    fn apply(&self, game: &dyn Writer, object: u64) -> bool {
        match &self.how {
            How::Memory(bytes) => game.write(object + u64::from(self.target.offset), bytes),
            How::Call {
                function, setting, ..
            } => game.call_setter(*function, object, setting),
        }
    }

    fn key(&self, object: u64) -> u64 {
        object + u64::from(self.target.offset)
    }
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
        let objects = places(game.memory(), ready)?;
        for object in &objects {
            if let Some(changed) = held.values.get(&ready.key(*object))
                && changed.plugin != plugin
            {
                return Err(Error::new(
                    Kind::Unavailable,
                    format!("{} is changed by another plugin", ready.text),
                ));
            }
        }
        let paths = held.paths.entry(plugin.to_owned()).or_default();
        if let Some(known) = paths.iter_mut().find(|known| known.text == ready.text) {
            *known = ready.clone();
        } else {
            if paths.len() >= MOST_PATHS {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!("a plugin keeps at most {MOST_PATHS} paths changed; restore one first"),
                ));
            }
            paths.push(ready.clone());
        }
        for object in &objects {
            let key = ready.key(*object);
            let original = if let Some(changed) = held.values.get(&key) {
                changed.original.clone()
            } else {
                ready
                    .now(game, *object)
                    .ok_or_else(|| unwritable(&ready.text))?
            };
            if !ready.apply(game, *object) {
                return Err(unwritable(&ready.text));
            }
            held.values.insert(
                key,
                Changed {
                    plugin: plugin.to_owned(),
                    path: ready.text.clone(),
                    object: *object,
                    original,
                    written: ready.written().to_vec(),
                },
            );
        }
        Ok(objects.len())
    }

    /// Applies every held change again where its path reaches: in objects
    /// it reaches now for the first time, and where the game set the value
    /// itself again, whose value becomes the original. Values in objects
    /// the path no longer reaches are forgotten. On the main thread, once a
    /// frame.
    pub fn frame(&self, game: &dyn Writer) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let readies: Vec<(String, Ready)> = held
            .paths
            .iter()
            .flat_map(|(plugin, readies)| {
                readies
                    .iter()
                    .filter(|ready| ready.hold)
                    .map(|ready| (plugin.clone(), ready.clone()))
            })
            .collect();
        for (plugin, ready) in readies {
            let Ok(objects) = places(game.memory(), &ready) else {
                continue;
            };
            let mut reached = BTreeSet::new();
            for object in objects {
                let key = ready.key(object);
                reached.insert(key);
                let original = match held.values.get(&key) {
                    Some(changed) if changed.plugin != plugin => continue,
                    Some(changed) => match ready.now(game, object) {
                        Some(now) if now != changed.written => now,
                        _ => continue,
                    },
                    None => match ready.now(game, object) {
                        Some(now) => now,
                        None => continue,
                    },
                };
                if ready.apply(game, object) {
                    held.values.insert(
                        key,
                        Changed {
                            plugin: plugin.clone(),
                            path: ready.text.clone(),
                            object,
                            original,
                            written: ready.written().to_vec(),
                        },
                    );
                }
            }
            held.values.retain(|key, changed| {
                changed.plugin != plugin || changed.path != ready.text || reached.contains(key)
            });
        }
    }

    /// Whether any change waits for [`Changes::frame`].
    #[must_use]
    pub fn holding(&self) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .paths
            .values()
            .flatten()
            .any(|ready| ready.hold)
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

/// The objects the change's path reaches.
fn places(memory: &dyn Memory, ready: &Ready) -> Result<Vec<u64>> {
    let reached = objects(memory, &ready.plan, 0)?;
    if reached.len() > MOST_CHANGED {
        return Err(Error::new(
            Kind::LimitExceeded,
            format!("a change reaches at most {MOST_CHANGED} objects"),
        ));
    }
    Ok(reached.into_iter().map(|(_, object, _)| object).collect())
}

/// Puts back the originals of `plugin`'s values through `ready`'s path
/// that the path still reaches and that still hold what was written; the
/// others are forgotten.
fn put_back(held: &mut Held, plugin: &str, ready: &Ready, game: &dyn Writer) {
    let reached: BTreeSet<u64> = places(game.memory(), ready)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mine: Vec<u64> = held
        .values
        .iter()
        .filter(|(_, changed)| changed.plugin == plugin && changed.path == ready.text)
        .map(|(key, _)| *key)
        .collect();
    for key in mine {
        let Some(changed) = held.values.remove(&key) else {
            continue;
        };
        if reached.contains(&changed.object)
            && ready.now(game, changed.object).as_deref() == Some(changed.written.as_slice())
        {
            ready.put(game, changed.object, &changed.original);
        }
    }
}

fn unwritable(text: &str) -> Error {
    Error::new(
        Kind::Unavailable,
        format!("{text} cannot be written in the game's memory now"),
    )
}
