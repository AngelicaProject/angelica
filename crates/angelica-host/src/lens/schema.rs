//! The game map's object model resolved for the running build: where each
//! root is and the offset of each field, or why the build lacks it.
#![forbid(unsafe_code)]
use angelica_map::{EntryKind, Map, Objects, Value, ValueKind};
use std::collections::BTreeMap;

/// Where a root's object is in the running game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootAt {
    /// The address of the `global` entry.
    pub address: u64,
    /// Whether the global holds a pointer to the object rather than the
    /// object itself.
    pub pointer: bool,
}

/// The object model and its places in one game build.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub objects: Objects,
    /// The offset of every `field` entry the model uses, or why it is
    /// absent in this build.
    pub offsets: BTreeMap<String, Result<u32, String>>,
    /// Every root's place, or why it is absent in this build.
    pub roots: BTreeMap<String, Result<RootAt, String>>,
    /// The address of every event's function or location, or why it is
    /// absent in this build.
    pub events: BTreeMap<String, Result<u64, String>>,
    /// The address of every `function` and `location` entry, or why it is
    /// absent: the points `angelica look` can count the calls of.
    pub code: BTreeMap<String, Result<u64, String>>,
}

impl Schema {
    /// The model of `map` for a build where `entry` resolves an entry to
    /// its value, or says why it is absent; the game is loaded at `base`.
    #[must_use]
    pub fn new(map: &Map, base: u64, entry: impl Fn(&str) -> Result<Value, String>) -> Self {
        let objects = map.objects().clone();
        let mut offsets = BTreeMap::new();
        for object in objects.types.values() {
            for field in object.fields.values() {
                let offset = match entry(&field.at) {
                    Ok(Value::Offset(offset)) => Ok(offset),
                    Ok(Value::Address(_)) => Err(format!("map entry {} is not a field", field.at)),
                    Err(reason) => Err(format!("map entry {} is absent: {reason}", field.at)),
                };
                offsets.insert(field.at.clone(), offset);
            }
        }
        let roots = objects
            .roots
            .values()
            .map(|root| {
                let pointer = map.get(&root.global).is_some_and(|global| {
                    matches!(
                        global.kind,
                        EntryKind::Global {
                            value: ValueKind::Ptr
                        }
                    )
                });
                let at = match entry(&root.global) {
                    Ok(Value::Address(rva)) => Ok(RootAt {
                        address: base + u64::from(rva),
                        pointer,
                    }),
                    Ok(Value::Offset(_)) => {
                        Err(format!("map entry {} is not a global", root.global))
                    }
                    Err(reason) => Err(format!("map entry {} is absent: {reason}", root.global)),
                };
                (root.name.clone(), at)
            })
            .collect();
        let events = objects
            .events
            .values()
            .map(|event| {
                let at = match entry(&event.at) {
                    Ok(Value::Address(rva)) => Ok(base + u64::from(rva)),
                    Ok(Value::Offset(_)) => Err(format!("map entry {} is not code", event.at)),
                    Err(reason) => Err(format!("map entry {} is absent: {reason}", event.at)),
                };
                (event.name.clone(), at)
            })
            .collect();
        let code = map
            .entries()
            .filter(|found| {
                matches!(
                    found.kind,
                    EntryKind::Function { .. } | EntryKind::Location { .. }
                )
            })
            .map(|found| {
                let at = match entry(&found.name) {
                    Ok(Value::Address(rva)) => Ok(base + u64::from(rva)),
                    Ok(Value::Offset(_)) => Err("not code".to_owned()),
                    Err(reason) => Err(reason),
                };
                (found.name.clone(), at)
            })
            .collect();
        Self {
            objects,
            offsets,
            roots,
            events,
            code,
        }
    }

    /// The model of `map` resolved through a catalog of the running build.
    #[must_use]
    pub fn from_catalog(map: &Map, catalog: &crate::catalog::Catalog, base: u64) -> Self {
        Self::new(map, base, |name| match catalog.present.get(name) {
            Some(resolved) => Ok(resolved.value),
            None => Err(catalog
                .absent
                .get(name)
                .cloned()
                .unwrap_or_else(|| "not resolved".into())),
        })
    }

    /// The offset of a field entry in this build.
    ///
    /// # Errors
    /// Why the build lacks it.
    pub fn offset(&self, entry: &str) -> Result<u32, String> {
        self.offsets
            .get(entry)
            .cloned()
            .unwrap_or_else(|| Err(format!("map entry {entry} is not in the object model")))
    }
}
