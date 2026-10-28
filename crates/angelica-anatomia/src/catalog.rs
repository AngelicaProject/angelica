//! The Lens catalog: what Lens offers over Anatomia. The structures and
//! fields a Lens path goes through, roots where paths start, events Lens
//! watches and decides at, actions it takes for the player, and fields
//! plugins may change through it. The Lens library
//! owns what each means; here the catalog is read and its items are
//! checked against the description they name.

use std::collections::BTreeMap;
use std::path::Path;

use toml::{Table, Value};

use crate::resolve::Resolution;
use crate::{Anatomia, AnatomiaError, Kind, TypeDef};

/// The files of the catalog, under `lens/` of the description.
pub const FILES: [&str; 5] = ["types", "roots", "events", "actions", "changes"];

/// One item of the catalog and what of the description it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogItem {
    /// `type`, `root`, `event`, `action`, or `change`.
    pub kind: &'static str,
    pub name: String,
    /// The item as written.
    pub table: Table,
}

/// The Lens catalog.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    pub items: Vec<CatalogItem>,
}

impl Catalog {
    /// Reads `lens/types.toml`, `roots.toml`, `events.toml`,
    /// `actions.toml`, and `changes.toml` of a description directory; a
    /// missing file is empty.
    ///
    /// # Errors
    ///
    /// A file that cannot be read or is not TOML.
    pub fn load(directory: &Path) -> Result<Self, AnatomiaError> {
        let mut files = Vec::new();
        for file in FILES {
            let path = directory.join("lens").join(format!("{file}.toml"));
            match std::fs::read_to_string(&path) {
                Ok(text) => files.push((file, text)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(AnatomiaError::Io {
                        path,
                        message: error.to_string(),
                    });
                }
            }
        }
        Self::from_sources(&files)
    }

    /// Builds a catalog from the text of its files, by name without
    /// `.toml`.
    ///
    /// # Errors
    ///
    /// A file that is not TOML or not a table of items.
    pub fn from_sources(files: &[(&str, String)]) -> Result<Self, AnatomiaError> {
        let mut items = Vec::new();
        for (file, text) in files {
            let path = format!("lens/{file}.toml");
            let invalid = |message: String| AnatomiaError::Toml {
                path: path.clone().into(),
                message,
            };
            let table: Table = text
                .parse()
                .map_err(|error: toml::de::Error| invalid(error.message().to_owned()))?;
            let kind = match *file {
                "types" => "type",
                "roots" => "root",
                "events" => "event",
                "actions" => "action",
                "changes" => "change",
                _ => return Err(invalid("not a file of the catalog".to_owned())),
            };
            for (key, value) in table {
                if key != *file {
                    return Err(invalid(format!(
                        "holds only `[{file}.<name>]`, not `{key}`"
                    )));
                }
                let Value::Table(entries) = value else {
                    return Err(invalid(format!("`{file}` is a table")));
                };
                for (name, item) in entries {
                    let Value::Table(item) = item else {
                        return Err(invalid(format!("`{file}.{name}` is a table")));
                    };
                    items.push(CatalogItem {
                        kind,
                        name,
                        table: item,
                    });
                }
            }
        }
        Ok(Self { items })
    }

    /// Whether each item is usable in a build: everything of the
    /// description it names exists, is of the right kind, and is present.
    #[must_use]
    pub fn check(
        &self,
        anatomia: &Anatomia,
        resolution: &Resolution,
    ) -> BTreeMap<(String, String), Result<(), String>> {
        self.items
            .iter()
            .map(|item| {
                (
                    (item.kind.to_owned(), item.name.clone()),
                    item.usable(anatomia, resolution),
                )
            })
            .collect()
    }
}

impl CatalogItem {
    fn string(&self, key: &str) -> Option<&str> {
        self.table.get(key).and_then(Value::as_str)
    }

    #[allow(clippy::too_many_lines)]
    fn usable(&self, anatomia: &Anatomia, resolution: &Resolution) -> Result<(), String> {
        let entry = |name: &str, kinds: &[&str]| -> Result<(), String> {
            let name = anatomia.canonical(name);
            let found = anatomia
                .get(name)
                .ok_or_else(|| format!("`{name}` is not an entry of the description"))?;
            if !kinds.contains(&found.kind.table()) {
                return Err(format!(
                    "`{name}` is a {}, not a {}",
                    found.kind.table(),
                    kinds.join(" or ")
                ));
            }
            match resolution.get(name) {
                Some(Ok(_)) => Ok(()),
                Some(Err(absence)) => Err(format!("`{name}` is absent: {absence}")),
                None => Err(format!("`{name}` was not resolved")),
            }
        };
        let structure = |name: &str| -> Result<(), String> {
            match anatomia.types().get(name) {
                Some(TypeDef::Struct(_)) if !anatomia.broken().contains_key(name) => Ok(()),
                Some(TypeDef::Struct(_)) => Err(format!("structure `{name}` is absent")),
                _ => Err(format!("`{name}` is not a structure of the description")),
            }
        };
        match self.kind {
            "type" => {
                structure(&self.name)?;
                let Some(TypeDef::Struct(definition)) = anatomia.types().get(&self.name) else {
                    unreachable!("checked above");
                };
                let fields = self
                    .table
                    .get("fields")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten();
                for field in fields {
                    let field = field.as_str().unwrap_or_default();
                    if !definition.fields.contains_key(field) {
                        return Err(format!("`{field}` is not a field of `{}`", self.name));
                    }
                    if anatomia
                        .broken()
                        .contains_key(&format!("{}.{field}", self.name))
                    {
                        return Err(format!("field `{field}` is absent"));
                    }
                }
                Ok(())
            }
            "root" => {
                entry(self.string("global").unwrap_or_default(), &["global"])?;
                structure(self.string("type").unwrap_or_default())
            }
            "event" => {
                entry(
                    self.string("at").unwrap_or_default(),
                    &["function", "location"],
                )?;
                for arg in self.args() {
                    if let Some(ty) = arg.get("ref").and_then(Value::as_str) {
                        structure(ty)?;
                    }
                }
                Ok(())
            }
            "action" => {
                entry(self.string("call").unwrap_or_default(), &["function"])?;
                for arg in self.args() {
                    if let Some(global) = arg.get("global").and_then(Value::as_str) {
                        entry(global, &["global"])?;
                    }
                }
                Ok(())
            }
            "change" => {
                let (type_name, field) = self
                    .name
                    .rsplit_once('.')
                    .ok_or_else(|| format!("`{}` is not a field's name", self.name))?;
                structure(type_name)?;
                let Some(TypeDef::Struct(definition)) = anatomia.types().get(type_name) else {
                    unreachable!("checked above");
                };
                let Some(field) = definition.fields.get(field) else {
                    return Err(format!("`{}` is not a field of the description", self.name));
                };
                let located = match &field.place {
                    crate::Place::Located => self.name.clone(),
                    crate::Place::Bits { field, .. } => format!("{type_name}.{field}"),
                };
                if anatomia
                    .get(&located)
                    .is_some_and(|entry| matches!(entry.kind, Kind::Field { .. }))
                {
                    entry(&located, &["field"])?;
                }
                if let Some(call) = self.string("call") {
                    entry(call, &["function"])?;
                }
                Ok(())
            }
            _ => unreachable!("kinds come from FILES"),
        }
    }

    fn args(&self) -> impl Iterator<Item = &Table> {
        self.table
            .get("args")
            .and_then(Value::as_table)
            .into_iter()
            .flat_map(|args| args.values().filter_map(Value::as_table))
    }
}
