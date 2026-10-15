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
    /// The `field` entries of text fields whose text is behind a pointer.
    pub text_pointers: std::collections::BTreeSet<String>,
    /// Where every action's function and globals are, or why the build
    /// lacks one.
    pub actions: BTreeMap<String, Result<super::act::ActionAt, String>>,
    /// The ids of every named element of each window the map has a schema
    /// of, by name, or why this build's layout of the window is unknown;
    /// filled by [`Schema::resolve_windows`].
    pub windows: BTreeMap<String, Result<BTreeMap<String, u32>, String>>,
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
                for at in std::iter::once(&field.at).chain(field.pointer.as_ref()) {
                    let offset = match entry(at) {
                        Ok(Value::Offset(offset)) => Ok(offset),
                        Ok(Value::Address(_)) => Err(format!("map entry {at} is not a field")),
                        Err(reason) => Err(format!("map entry {at} is absent: {reason}")),
                    };
                    offsets.insert(at.clone(), offset);
                }
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
        let actions = actions_at(map, base, &entry);
        let text_pointers = objects
            .types
            .values()
            .flat_map(|object| object.fields.values())
            .filter(|field| matches!(field.shape, angelica_map::Shape::Text(_)))
            .filter(|field| {
                map.get(&field.at).is_some_and(|found| {
                    matches!(
                        found.kind,
                        EntryKind::Field {
                            value: ValueKind::Ptr,
                            ..
                        }
                    )
                })
            })
            .map(|field| field.at.clone())
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
            text_pointers,
            actions,
            windows: BTreeMap::new(),
        }
    }

    /// Names the elements of every window the map has a schema of, by the
    /// fingerprint of the window's layout file in this build, which
    /// `layout` reads from the game's archives.
    pub fn resolve_windows(&mut self, map: &Map, layout: &dyn Fn(&str) -> Result<Vec<u8>, String>) {
        self.windows = map
            .windows()
            .map(|schema| {
                let named = layout(&schema.layout)
                    .map_err(|reason| format!("its layout file cannot be read: {reason}"))
                    .and_then(|bytes| {
                        schema.resolve(&angelica_map::window_layout_fingerprint(&bytes))
                    });
                (schema.window.clone(), named)
            })
            .collect();
    }

    /// The id of the element `element` of the window `window` in this build.
    ///
    /// # Errors
    /// `not-found` for a window or element the map does not name;
    /// `unavailable` when the build's layout of the window is unknown.
    pub fn element(&self, window: &str, element: &str) -> crate::error::Result<u32> {
        use crate::error::{Error, Kind};
        let named = self.windows.get(window).ok_or_else(|| {
            Error::new(
                Kind::NotFound,
                format!("the game map names no elements of window {window}"),
            )
        })?;
        let named = named.as_ref().map_err(|reason| {
            Error::new(
                Kind::Unavailable,
                format!(
                    "the elements of window {window} are not known in this game build: {reason}"
                ),
            )
        })?;
        named.get(element).copied().ok_or_else(|| {
            let known = named.keys().cloned().collect::<Vec<_>>().join(", ");
            Error::new(
                Kind::NotFound,
                format!("the game map names no element {element} of window {window}; its elements: {known}"),
            )
        })
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

/// Where each action's function and globals are in the build `entry`
/// resolves, or why the build lacks one.
fn actions_at(
    map: &Map,
    base: u64,
    entry: &dyn Fn(&str) -> Result<Value, String>,
) -> BTreeMap<String, Result<super::act::ActionAt, String>> {
    map.objects()
        .actions
        .values()
        .map(|action| (action.name.clone(), action_at(map, base, entry, action)))
        .collect()
}

fn action_at(
    map: &Map,
    base: u64,
    entry: &dyn Fn(&str) -> Result<Value, String>,
    action: &angelica_map::Action,
) -> Result<super::act::ActionAt, String> {
    let function = match entry(&action.call) {
        Ok(Value::Address(rva)) => base + u64::from(rva),
        Ok(Value::Offset(_)) => return Err(format!("map entry {} is not code", action.call)),
        Err(reason) => return Err(format!("map entry {} is absent: {reason}", action.call)),
    };
    let ret = match map.get(&action.call).map(|found| &found.kind) {
        Some(EntryKind::Function { abi, .. }) => abi.ret,
        _ => ValueKind::Void,
    };
    let mut globals = BTreeMap::new();
    for arg in &action.args {
        let angelica_map::ActSource::Global(global) = &arg.source else {
            continue;
        };
        let pointer = map.get(global).is_some_and(|found| {
            matches!(
                found.kind,
                EntryKind::Global {
                    value: ValueKind::Ptr
                }
            )
        });
        let at = match entry(global) {
            Ok(Value::Address(rva)) => RootAt {
                address: base + u64::from(rva),
                pointer,
            },
            Ok(Value::Offset(_)) => return Err(format!("map entry {global} is not a global")),
            Err(reason) => return Err(format!("map entry {global} is absent: {reason}")),
        };
        globals.insert(global.clone(), at);
    }
    Ok(super::act::ActionAt {
        function,
        ret,
        globals,
    })
}
