use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use toml::{Table, Value};

use crate::meaning::Meaning;
use crate::model::{
    Common, Entry, Enum, Kind, Place, Slot, Step, Struct, StructField, Text, TypeDef, common,
    extension_attributes, parse_steps, parse_thread,
};
use crate::table::Keys;
use crate::ty::{Primitive, Ty};
use crate::{AnatomiaError, FORMAT_MAJOR, FORMAT_MINOR};

/// The folders of a description, and the kinds each file in them holds.
const FOLDERS: [(&str, &[&str]); 3] = [
    ("types", &["struct", "union", "enum", "alias", "template"]),
    ("functions", &["function", "location"]),
    ("globals", &["global", "vtable"]),
];

/// A description of the game: the central Anatomia, or a plugin's own
/// entries. An entry that cannot be read or does not fit the rest is
/// absent with its reason; the others are unaffected.
#[derive(Debug, Clone)]
pub struct Anatomia {
    pub minor: u32,
    pub namespace: String,
    pub extensions: Vec<String>,
    pub(crate) entries: BTreeMap<String, Entry>,
    pub(crate) types: BTreeMap<String, TypeDef>,
    pub(crate) aliases: BTreeMap<String, String>,
    pub(crate) broken: BTreeMap<String, String>,
    /// Entries that could not be read: absent in every build.
    pub(crate) unread: BTreeSet<String>,
    /// Entry names, each after every entry it refers to.
    pub(crate) order: Vec<String>,
    pub meaning: Meaning,
    /// The sheet and window schemas, as their files' paths and text.
    pub sheets: Vec<(String, String)>,
    pub windows: Vec<(String, String)>,
    pub(crate) sha256: String,
}

impl Anatomia {
    /// Reads a description directory: `anatomia.toml`, and every file of
    /// `types/`, `functions/`, `globals/`, `meaning/`, `windows/`, and,
    /// with folders, `sheets/`. `lens/` and `builds/` are not part of it.
    ///
    /// # Errors
    ///
    /// A file that cannot be read, is not TOML, or lies where no file
    /// belongs, and an `anatomia.toml` of another major version.
    pub fn load(directory: &Path) -> Result<Self, AnatomiaError> {
        let mut files = Vec::new();
        for folder in ["types", "functions", "globals", "meaning", "windows"] {
            collect(directory, folder, false, &mut files)?;
        }
        collect(directory, "sheets", true, &mut files)?;
        let manifest = read_text(&directory.join("anatomia.toml"))?;
        Self::from_sources(&manifest, &files)
    }

    /// Builds a description from the text of `anatomia.toml` and of every
    /// other file, given with its path relative to the directory.
    ///
    /// # Errors
    ///
    /// As [`Anatomia::load`].
    pub fn from_sources(manifest: &str, files: &[(String, String)]) -> Result<Self, AnatomiaError> {
        let manifest_table = parse_toml("anatomia.toml", manifest)?;
        let (minor, namespace, extensions) = read_manifest(manifest_table)?;
        let mut loader = Loader {
            extensions: extensions.clone(),
            entries: BTreeMap::new(),
            types: BTreeMap::new(),
            aliases: BTreeMap::new(),
            broken: BTreeMap::new(),
            unread: BTreeSet::new(),
            defined: BTreeMap::new(),
        };
        let mut meaning_files = Vec::new();
        let mut sheets = Vec::new();
        let mut windows = Vec::new();
        for (path, text) in files {
            let folder = path.split('/').next().unwrap_or_default();
            if folder == "sheets" {
                sheets.push((path.clone(), text.clone()));
                continue;
            }
            if folder == "windows" {
                windows.push((path.clone(), text.clone()));
                continue;
            }
            if folder != "meaning" && !FOLDERS.iter().any(|(name, _)| *name == folder) {
                return Err(AnatomiaError::UnexpectedFile {
                    path: PathBuf::from(path),
                });
            }
            let table = parse_toml(path, text)?;
            let area = path
                .strip_prefix(&format!("{folder}/"))
                .and_then(|name| name.strip_suffix(".toml"))
                .unwrap_or_default()
                .to_owned();
            if folder == "meaning" {
                meaning_files.push((area, table));
                continue;
            }
            let kinds = FOLDERS
                .iter()
                .find(|(name, _)| *name == folder)
                .map(|(_, kinds)| *kinds)
                .ok_or_else(|| AnatomiaError::UnexpectedFile {
                    path: PathBuf::from(path),
                })?;
            loader.read_file(path, &area, kinds, table);
        }
        loader.check_names();
        loader.check_references();
        let meaning = Meaning::read(meaning_files, &mut loader);
        let order = loader.order();
        let sha256 = description_sha256(manifest, files);
        Ok(Self {
            minor,
            namespace,
            extensions,
            entries: loader.entries,
            types: loader.types,
            aliases: loader.aliases,
            broken: loader.broken,
            unread: loader.unread,
            order,
            meaning,
            sheets,
            windows,
            sha256,
        })
    }

    /// Every entry that was read, by name.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// An entry by its name or an alias of it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(self.canonical(name))
    }

    /// The name an alias stands for, or the name itself.
    #[must_use]
    pub fn canonical<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map_or(name, String::as_str)
    }

    /// Every type, by name.
    #[must_use]
    pub fn types(&self) -> &BTreeMap<String, TypeDef> {
        &self.types
    }

    /// The names the description defines but cannot use, and why: an entry,
    /// a type, or a field of a structure that is absent in every build.
    #[must_use]
    pub fn broken(&self) -> &BTreeMap<String, String> {
        &self.broken
    }

    /// The SHA-256 over what decides how entries resolve: `anatomia.toml`
    /// and every file of `types/`, `functions/`, and `globals/`.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

fn read_manifest(table: Table) -> Result<(u32, String, Vec<String>), AnatomiaError> {
    let invalid = |message: String| AnatomiaError::Manifest { message };
    let mut keys = Keys::new(Value::Table(table)).map_err(invalid)?;
    let format = keys.required_string("format").map_err(invalid)?;
    let (major, minor) = format
        .split_once('.')
        .and_then(|(major, minor)| Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?)))
        .ok_or_else(|| invalid(format!("`format` is `major.minor`, not `{format}`")))?;
    if major != FORMAT_MAJOR {
        return Err(AnatomiaError::Format {
            found: format,
            supported: format!("{FORMAT_MAJOR}.{FORMAT_MINOR}"),
        });
    }
    let namespace = keys.required_string("namespace").map_err(invalid)?;
    if !valid_namespace(&namespace) {
        return Err(invalid(format!("`{namespace}` is not a namespace")));
    }
    let extensions = keys.strings("extensions").map_err(invalid)?;
    for extension in &extensions {
        if !valid_name(extension) {
            return Err(invalid(format!(
                "extension `{extension}` is not named `<owner>.<name>`"
            )));
        }
    }
    // A key of a newer minor version is ignored here: the manifest holds
    // nothing an entry's meaning rests on.
    if minor.cmp(&FORMAT_MINOR).is_le() {
        keys.finish().map_err(invalid)?;
    }
    Ok((minor, namespace, extensions))
}

pub(crate) struct Loader {
    extensions: Vec<String>,
    pub(crate) entries: BTreeMap<String, Entry>,
    pub(crate) types: BTreeMap<String, TypeDef>,
    aliases: BTreeMap<String, String>,
    pub(crate) broken: BTreeMap<String, String>,
    unread: BTreeSet<String>,
    /// Every name defined, with how many times, for duplicates. Types are
    /// a namespace of their own, as `type <name>`.
    defined: BTreeMap<String, usize>,
}

impl Loader {
    pub(crate) fn has_field(&self, name: &str) -> bool {
        let name = self.aliases.get(name).map_or(name, String::as_str);
        let Some((type_name, field)) = name.rsplit_once('.') else {
            return false;
        };
        matches!(self.types.get(type_name), Some(TypeDef::Struct(structure)) if structure.fields.contains_key(field))
    }

    pub(crate) fn canonical(&self, name: &str) -> String {
        self.aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }

    fn define(&mut self, name: &str) {
        *self.defined.entry(name.to_owned()).or_default() += 1;
    }

    pub(crate) fn mark(&mut self, name: &str, reason: String) {
        self.broken.entry(name.to_owned()).or_insert(reason);
    }

    fn read_file(&mut self, path: &str, area: &str, kinds: &[&str], table: Table) {
        for (kind, items) in table {
            let Value::Table(items) = items else {
                self.mark(
                    &format!("{path}:{kind}"),
                    format!("`{kind}` is not a table"),
                );
                continue;
            };
            for (name, value) in items {
                let is_type = matches!(
                    kind.as_str(),
                    "struct" | "union" | "enum" | "alias" | "template"
                );
                if !is_type && valid_name(&name) {
                    self.unread.insert(name.clone());
                }
                self.define(&if is_type {
                    format!("type {name}")
                } else {
                    name.clone()
                });
                if !valid_name(&name) {
                    self.mark(&name, "is not a valid name".to_owned());
                    continue;
                }
                if name.split('.').next() != Some(area) {
                    self.mark(&name, format!("does not belong to the area of {path}"));
                    continue;
                }
                if !kinds.contains(&kind.as_str()) {
                    self.mark(
                        &name,
                        format!(
                            "`{kind}` is not a kind {path} holds, or one this reader of Anatomia {FORMAT_MAJOR}.{FORMAT_MINOR} knows"
                        ),
                    );
                    continue;
                }
                let result = match kind.as_str() {
                    "struct" => self.read_struct(&name, value),
                    "union" | "enum" | "alias" | "template" => {
                        read_type(&kind, &name, value).map(|definition| {
                            self.types.insert(name.clone(), definition);
                        })
                    }
                    _ => read_entry(&kind, &name, value, &self.extensions).map(|entry| {
                        self.unread.remove(&name);
                        self.entries.insert(name.clone(), entry);
                    }),
                };
                if let Err(reason) = result {
                    self.mark(&name, reason);
                }
            }
        }
    }

    fn read_struct(&mut self, name: &str, value: Value) -> Result<(), String> {
        let mut keys = Keys::new(value)?;
        let doc = keys.string("doc")?;
        let size = keys.unsigned("size")?;
        let align = keys.unsigned("align")?;
        let extends = keys
            .array("extends")?
            .into_iter()
            .map(|base| {
                let mut base = Keys::new(base)?;
                let ty = base.required_string("type")?;
                let at = base.unsigned("at")?.unwrap_or(0);
                base.finish()?;
                Ok((ty, at))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let vtable = keys.string("vtable")?;
        let variant = keys
            .table("variant")?
            .map(|variant| {
                let mut variant = Keys::new(Value::Table(variant))?;
                let field = variant.required_string("field")?;
                let values = variant.array("values")?;
                variant.finish()?;
                if values.is_empty() {
                    return Err("`variant` names at least one value".to_owned());
                }
                Ok((field, values))
            })
            .transpose()?;
        let text = keys.string("text")?;
        let ext = extension_attributes(&mut keys, &self.extensions)?;
        let raw_fields = keys.table("fields")?.unwrap_or_default();
        keys.finish()?;

        let mut fields = BTreeMap::new();
        for (field, value) in raw_fields {
            let full = format!("{name}.{field}");
            self.define(&full);
            if !valid_word(&field) {
                self.mark(&full, "is not a valid field name".to_owned());
                continue;
            }
            match read_field(name, &field, value, &self.extensions) {
                Ok((definition, entry)) => {
                    for alias in &definition.aliases {
                        let alias_name = if alias.contains('.') {
                            alias.clone()
                        } else {
                            format!("{name}.{alias}")
                        };
                        self.define(&alias_name);
                        self.aliases.insert(alias_name, full.clone());
                    }
                    if let Some(entry) = entry {
                        self.entries.insert(full.clone(), entry);
                    }
                    fields.insert(field, definition);
                }
                Err(reason) => {
                    self.unread.insert(full.clone());
                    self.mark(&full, reason);
                }
            }
        }
        self.types.insert(
            name.to_owned(),
            TypeDef::Struct(Struct {
                name: name.to_owned(),
                doc,
                size,
                align,
                extends,
                vtable,
                variant,
                text,
                fields,
                ext,
            }),
        );
        Ok(())
    }

    /// Marks every name defined twice: neither definition is used.
    fn check_names(&mut self) {
        let duplicates: Vec<String> = self
            .defined
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(name, _)| name.clone())
            .collect();
        for name in duplicates {
            let name = name
                .strip_prefix("type ")
                .map_or(name.clone(), str::to_owned);
            let canonical = self.canonical(&name);
            self.mark(&canonical, format!("`{name}` is defined more than once"));
        }
    }

    /// Marks what refers to a name that does not exist or is of the wrong
    /// kind. Entries that refer to broken ones are absent when resolved.
    fn check_references(&mut self) {
        // Steps and checks may name an entry by an alias.
        let aliases = self.aliases.clone();
        for entry in self.entries.values_mut() {
            let canonical = |name: &mut String| {
                if let Some(target) = aliases.get(name.as_str()) {
                    name.clone_from(target);
                }
            };
            for step in &mut entry.find {
                match step {
                    Step::In(name) | Step::Entry(name) | Step::Vtable { vtable: name, .. } => {
                        canonical(name);
                    }
                    _ => {}
                }
            }
            for check in &mut entry.checks {
                match check {
                    crate::model::Check::Find(steps) => {
                        for step in steps {
                            if let Step::In(name)
                            | Step::Entry(name)
                            | Step::Vtable { vtable: name, .. } = step
                            {
                                canonical(name);
                            }
                        }
                    }
                    crate::model::Check::Equals(name)
                    | crate::model::Check::Contains {
                        function: Some(name),
                        ..
                    } => canonical(name),
                    _ => {}
                }
            }
            for name in &mut entry.depends {
                canonical(name);
            }
        }

        let mut problems = Vec::new();
        for entry in self.entries.values() {
            if let Some(problem) = self.entry_problem(entry) {
                problems.push((entry.name.clone(), problem));
            }
        }
        for definition in self.types.values() {
            for (name, problem) in self.type_problems(definition) {
                problems.push((name, problem));
            }
        }
        for (name, problem) in problems {
            self.mark(&name, problem);
        }
    }

    fn kind_of(&self, name: &str) -> Option<&'static str> {
        self.entries.get(name).map(|entry| entry.kind.table())
    }

    fn entry_problem(&self, entry: &Entry) -> Option<String> {
        let wants = |name: &str, kinds: &[&str]| -> Option<String> {
            match self.kind_of(name) {
                // An entry that could not be read is absent, like any
                // other absent entry this one depends on.
                None if self.unread.contains(name) => None,
                None => Some(format!("refers to `{name}`, which is not an entry")),
                Some(kind) if !kinds.is_empty() && !kinds.contains(&kind) => Some(format!(
                    "refers to `{name}`, a {kind}, where it needs a {}",
                    kinds.join(" or ")
                )),
                Some(_) => None,
            }
        };
        let step_problem = |steps: &[Step]| -> Option<String> {
            steps.iter().find_map(|step| match step {
                Step::In(name) => wants(name, &["function"]),
                Step::Entry(name) => wants(name, &[]),
                Step::Vtable { vtable, slot } => wants(vtable, &["vtable"]).or_else(|| {
                    let Some(Entry {
                        kind: Kind::Vtable { slots, .. },
                        ..
                    }) = self.entries.get(vtable)
                    else {
                        return None;
                    };
                    match slot {
                        Slot::Name(slot) if !slots.contains_key(slot) => {
                            Some(format!("`{vtable}` has no slot `{slot}`"))
                        }
                        _ => None,
                    }
                }),
                _ => None,
            })
        };
        if let Some(problem) = step_problem(&entry.find) {
            return Some(problem);
        }
        for check in &entry.checks {
            let problem = match check {
                crate::model::Check::Find(steps) => step_problem(steps),
                crate::model::Check::Equals(name) => wants(name, &[]),
                crate::model::Check::Contains {
                    function: Some(name),
                    ..
                } => wants(name, &["function"]),
                crate::model::Check::Contains { function: None, .. }
                    if !matches!(entry.kind, Kind::Function { .. }) =>
                {
                    Some(
                        "a `contains` check of an entry that is not a function names `in`"
                            .to_owned(),
                    )
                }
                _ => None,
            };
            if problem.is_some() {
                return problem;
            }
        }
        if let Some(name) = entry
            .depends
            .iter()
            .find(|name| self.kind_of(name).is_none() && !self.unread.contains(*name))
        {
            return Some(format!("depends on `{name}`, which is not an entry"));
        }
        let mut names = Vec::new();
        match &entry.kind {
            Kind::Function { ty, member_of, .. } => {
                Ty::Fn(ty.clone()).names(&mut names);
                names.extend(member_of.iter().cloned());
            }
            Kind::Location { registers, .. } => {
                for ty in registers.values() {
                    ty.names(&mut names);
                }
            }
            Kind::Global { ty } => ty.names(&mut names),
            Kind::Vtable { slots, .. } => {
                for (_, ty) in slots.values() {
                    if let Some(ty) = ty {
                        ty.names(&mut names);
                    }
                }
            }
            Kind::Field { .. } => {}
        }
        names
            .iter()
            .find(|name| !self.types.contains_key(*name))
            .map(|name| format!("names type `{name}`, which is not defined"))
    }

    fn type_problems(&self, definition: &TypeDef) -> Vec<(String, String)> {
        let unknown = |ty: &Ty, params: &[String]| -> Option<String> {
            let mut names = Vec::new();
            ty.names(&mut names);
            names
                .into_iter()
                .find(|name| !self.types.contains_key(name) && !params.contains(name))
                .map(|name| format!("names type `{name}`, which is not defined"))
        };
        let mut problems = Vec::new();
        match definition {
            TypeDef::Struct(structure) => {
                if let Some((base, _)) = structure
                    .extends
                    .iter()
                    .find(|(base, _)| !matches!(self.types.get(base), Some(TypeDef::Struct(_))))
                {
                    problems.push((
                        structure.name.clone(),
                        format!("extends `{base}`, which is not a structure"),
                    ));
                }
                if structure.variant.is_some() && structure.extends.is_empty() {
                    problems.push((
                        structure.name.clone(),
                        "has a `variant` without a base".to_owned(),
                    ));
                }
                if let Some(text) = &structure.text
                    && !structure.fields.contains_key(text)
                {
                    problems.push((
                        structure.name.clone(),
                        format!("reads as text through `{text}`, which is not its field"),
                    ));
                }
                for field in structure.fields.values() {
                    let full = format!("{}.{}", structure.name, field.name);
                    let sibling = |name: &Option<String>| {
                        name.as_ref()
                            .filter(|name| !structure.fields.contains_key(*name))
                            .cloned()
                    };
                    let problem = unknown(&field.ty, &[])
                        .or_else(|| {
                            sibling(&field.count)
                                .map(|name| format!("counts by `{name}`, which is not a field of the structure"))
                        })
                        .or_else(|| match &field.place {
                            Place::Bits { field: base, .. } => match structure.fields.get(base) {
                                Some(StructField {
                                    ty: Ty::Primitive(primitive),
                                    place: Place::Located,
                                    ..
                                }) if primitive.is_integer() => None,
                                _ => Some(format!(
                                    "is bits of `{base}`, which is not a located integer field of the structure"
                                )),
                            },
                            Place::Located => None,
                        })
                        .or_else(|| {
                            [&field.r#enum, &field.flags]
                                .into_iter()
                                .flatten()
                                .find(|name| !matches!(self.types.get(*name), Some(TypeDef::Enum(_))))
                                .map(|name| format!("names enum `{name}`, which is not defined"))
                        });
                    if let Some(problem) = problem {
                        problems.push((full, problem));
                    }
                }
            }
            TypeDef::Union { name, fields, .. } => {
                if let Some(problem) = fields.values().find_map(|ty| unknown(ty, &[])) {
                    problems.push((name.clone(), problem));
                }
            }
            TypeDef::Alias { name, ty, .. } => {
                if let Some(problem) = unknown(ty, &[]) {
                    problems.push((name.clone(), problem));
                }
            }
            TypeDef::Template {
                name,
                params,
                fields,
                ..
            } => {
                if let Some(problem) = fields.values().find_map(|(ty, _)| unknown(ty, params)) {
                    problems.push((name.clone(), problem));
                }
            }
            TypeDef::Enum(_) => {}
        }
        problems
    }

    /// Entry names in an order where each comes after what it refers to.
    /// An entry in a cycle is marked.
    fn order(&mut self) -> Vec<String> {
        let mut order = Vec::new();
        let mut done = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        let names: Vec<String> = self.entries.keys().cloned().collect();
        let mut cycles = Vec::new();
        for name in &names {
            self.visit(name, &mut done, &mut visiting, &mut order, &mut cycles);
        }
        for name in cycles {
            self.mark(&name, "refers to itself through other entries".to_owned());
        }
        order
    }

    fn visit(
        &self,
        name: &str,
        done: &mut BTreeSet<String>,
        visiting: &mut BTreeSet<String>,
        order: &mut Vec<String>,
        cycles: &mut Vec<String>,
    ) {
        if done.contains(name) {
            return;
        }
        if !visiting.insert(name.to_owned()) {
            cycles.push(name.to_owned());
            return;
        }
        if let Some(entry) = self.entries.get(name) {
            for reference in entry.references() {
                self.visit(reference, done, visiting, order, cycles);
            }
            // A field is found after the structure's other fields it is
            // counted by or is bits of; neither is a reference of the entry.
        }
        visiting.remove(name);
        done.insert(name.to_owned());
        if self.entries.contains_key(name) {
            order.push(name.to_owned());
        }
    }
}

fn read_entry(
    kind: &str,
    name: &str,
    value: Value,
    extensions: &[String],
) -> Result<Entry, String> {
    let mut keys = Keys::new(value)?;
    let find = parse_steps(keys.array("find")?)?;
    let Common {
        doc,
        checks,
        depends,
        ext,
    } = common(&mut keys, extensions)?;
    let kind = match kind {
        "function" => {
            let ty = match Ty::parse(&keys.required_string("type")?)? {
                Ty::Fn(function) => function,
                other => return Err(format!("`type` of a function is `fn(…)`, not `{other}`")),
            };
            Kind::Function {
                ty,
                thread: parse_thread(&keys.required_string("thread")?)?,
                member_of: keys.string("member_of")?,
            }
        }
        "location" => {
            let registers = keys
                .table("registers")?
                .unwrap_or_default()
                .into_iter()
                .map(|(register, ty)| match ty {
                    Value::String(ty) => Ok((register, Ty::parse(&ty)?)),
                    _ => Err(format!("register `{register}` takes a type")),
                })
                .collect::<Result<_, String>>()?;
            Kind::Location {
                thread: parse_thread(&keys.required_string("thread")?)?,
                registers,
            }
        }
        "global" => Kind::Global {
            ty: Ty::parse(&keys.required_string("type")?)?,
        },
        "vtable" => {
            let mut slots = BTreeMap::new();
            for (slot, value) in keys.table("slots")?.unwrap_or_default() {
                let parsed = match value {
                    Value::Integer(index) => (index, None),
                    Value::Table(table) => {
                        let mut slot_keys = Keys::new(Value::Table(table))?;
                        let index = slot_keys
                            .integer("slot")?
                            .ok_or_else(|| format!("slot `{slot}` needs `slot`"))?;
                        let ty = slot_keys
                            .string("type")?
                            .map(|ty| Ty::parse(&ty))
                            .transpose()?;
                        slot_keys.finish()?;
                        (index, ty)
                    }
                    _ => return Err(format!("slot `{slot}` is a number or a table")),
                };
                let index = u32::try_from(parsed.0)
                    .map_err(|_| format!("slot `{slot}` is not negative"))?;
                slots.insert(slot, (index, parsed.1));
            }
            let code_slots = keys.unsigned("code_slots")?.unwrap_or(0);
            if code_slots == 0 && slots.is_empty() {
                return Err("a virtual table needs `code_slots` or `slots`".to_owned());
            }
            Kind::Vtable { code_slots, slots }
        }
        _ => unreachable!("kinds come from FOLDERS"),
    };
    keys.finish()?;
    Ok(Entry {
        name: name.to_owned(),
        kind,
        find,
        checks,
        depends,
        doc,
        ext,
    })
}

/// A field of a structure, and the entry that finds it when it is not bits
/// of another field.
fn read_field(
    structure: &str,
    name: &str,
    value: Value,
    extensions: &[String],
) -> Result<(StructField, Option<Entry>), String> {
    let mut keys = Keys::new(value)?;
    let ty = Ty::parse(&keys.required_string("type")?)?;
    let at = keys.unsigned("at")?;
    let find = keys.array("find")?;
    let bits = keys.table("bits")?;
    let Common {
        doc,
        checks,
        depends,
        ext,
    } = common(&mut keys, extensions)?;
    let text = match keys.value("text") {
        None => None,
        Some(Value::Boolean(true)) => Some(Text::Terminated),
        Some(Value::Integer(most)) if (1..=65535).contains(&most) => {
            Some(Text::Most(u32::try_from(most).unwrap_or(0)))
        }
        Some(_) => return Err("`text` is `true` or the most bytes, 1 to 65535".to_owned()),
    };
    let field = StructField {
        name: name.to_owned(),
        ty,
        place: Place::Located,
        doc,
        text,
        count: keys.string("count")?,
        present: keys.string("present")?,
        sheet: keys.string("sheet")?,
        r#enum: keys.string("enum")?,
        flags: keys.string("flags")?,
        element: keys.string("element")?,
        unit: keys.string("unit")?,
        aliases: keys.strings("aliases")?,
        ext,
    };
    keys.finish()?;
    if let Some(alias) = field
        .aliases
        .iter()
        .find(|alias| !valid_word(alias) && !valid_name(alias))
    {
        return Err(format!(
            "alias `{alias}` is neither a field name nor an entry name"
        ));
    }
    check_text(&field)?;

    if let Some(bits) = bits {
        if at.is_some() || !find.is_empty() || !checks.is_empty() || !depends.is_empty() {
            return Err(
                "bits of another field have no `at`, `find`, `checks`, or `depends`".to_owned(),
            );
        }
        let mut bits = Keys::new(Value::Table(bits))?;
        let base = bits.required_string("in")?;
        let offset = bits.unsigned("at")?.ok_or("`bits` needs `at`")?;
        let width = bits.unsigned("width")?.unwrap_or(1);
        bits.finish()?;
        if width == 0 || offset + width > 64 {
            return Err("`bits` lie within 64 bits".to_owned());
        }
        if !matches!(field.ty, Ty::Primitive(primitive) if primitive.is_integer() || primitive == Primitive::Bool)
        {
            return Err("bits read as an integer or a `bool`".to_owned());
        }
        let place = Place::Bits {
            field: base,
            at: u8::try_from(offset).unwrap_or(0),
            width: u8::try_from(width).unwrap_or(0),
        };
        return Ok((StructField { place, ..field }, None));
    }

    let find = match (at, find.is_empty()) {
        (Some(_), false) => return Err("a field has `at` or `find`, not both".to_owned()),
        (None, true) => return Err("a field needs `at`, `find`, or `bits`".to_owned()),
        (Some(_), true) => Vec::new(),
        (None, false) => parse_steps(find)?,
    };
    let entry = Entry {
        name: format!("{structure}.{name}"),
        kind: Kind::Field {
            of: structure.to_owned(),
            field: name.to_owned(),
            at,
        },
        find,
        checks,
        depends,
        doc: None,
        ext: BTreeMap::new(),
    };
    Ok((field, Some(entry)))
}

/// Whether a field's `text` fits its type.
fn check_text(field: &StructField) -> Result<(), String> {
    match (&field.text, &field.ty) {
        (None, _)
        | (Some(Text::Terminated), Ty::Array(_, _))
        | (Some(Text::Most(_)), Ty::Ptr(_) | Ty::Named(..)) => {
            if matches!(field.text, Some(Text::Terminated))
                && !matches!(&field.ty, Ty::Array(inner, _) if **inner == Ty::Char8)
            {
                return Err("`text = true` is for a `char8` array".to_owned());
            }
            Ok(())
        }
        _ => Err(
            "`text` is `true` on a `char8` array, or the most bytes on a pointer or a structure"
                .to_owned(),
        ),
    }
}

fn read_type(kind: &str, name: &str, value: Value) -> Result<TypeDef, String> {
    let mut keys = Keys::new(value)?;
    let doc = keys.string("doc")?;
    let definition = match kind {
        "enum" => {
            let ty = keys
                .string("type")?
                .map(|ty| match Ty::parse(&ty)? {
                    Ty::Primitive(primitive) if primitive.is_integer() => Ok(primitive),
                    _ => Err("an enum's `type` is an integer".to_owned()),
                })
                .transpose()?;
            let flags = keys.boolean("flags")?.unwrap_or(false);
            let mut values = BTreeMap::new();
            for (value_name, value) in keys.table("values")?.unwrap_or_default() {
                let Value::Integer(number) = value else {
                    return Err(format!("value `{value_name}` is an integer"));
                };
                let number = u64::try_from(number)
                    .map_err(|_| format!("value `{value_name}` is not negative"))?;
                if flags && !number.is_power_of_two() {
                    return Err(format!("value `{value_name}` of a flags enum is one bit"));
                }
                values.insert(value_name, number);
            }
            if values.is_empty() {
                return Err("an enum has at least one value".to_owned());
            }
            TypeDef::Enum(Enum {
                name: name.to_owned(),
                doc,
                ty,
                flags,
                values,
            })
        }
        "union" => {
            let fields = keys
                .table("fields")?
                .unwrap_or_default()
                .into_iter()
                .map(|(field, ty)| match ty {
                    Value::String(ty) => Ok((field, Ty::parse(&ty)?)),
                    _ => Err(format!("union field `{field}` takes a type")),
                })
                .collect::<Result<_, String>>()?;
            TypeDef::Union {
                name: name.to_owned(),
                doc,
                fields,
            }
        }
        "alias" => TypeDef::Alias {
            name: name.to_owned(),
            doc,
            ty: Ty::parse(&keys.required_string("type")?)?,
        },
        "template" => {
            let params = keys.strings("params")?;
            if params.is_empty() {
                return Err("a template has at least one parameter".to_owned());
            }
            let size = keys.unsigned("size")?;
            let mut fields = BTreeMap::new();
            for (field, value) in keys.table("fields")?.unwrap_or_default() {
                let mut field_keys = Keys::new(value)?;
                let ty = Ty::parse(&field_keys.required_string("type")?)?;
                let at = field_keys
                    .unsigned("at")?
                    .ok_or_else(|| format!("template field `{field}` needs `at`"))?;
                field_keys.finish()?;
                fields.insert(field, (ty, at));
            }
            let range = keys
                .table("range")?
                .map(|range| {
                    let mut range = Keys::new(Value::Table(range))?;
                    let bounds = (range.required_string("from")?, range.required_string("to")?);
                    range.finish()?;
                    Ok::<_, String>(bounds)
                })
                .transpose()?;
            TypeDef::Template {
                name: name.to_owned(),
                doc,
                params,
                size,
                fields,
                range,
            }
        }
        _ => unreachable!("type kinds come from read_file"),
    };
    keys.finish()?;
    Ok(definition)
}

/// A lowercase letter followed by lowercase letters, digits, and
/// underscores.
pub(crate) fn valid_word(word: &str) -> bool {
    word.len() <= 64
        && word.starts_with(|character: char| character.is_ascii_lowercase())
        && word
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// `ffxiv`, or a plugin id: 1 to 64 lowercase letters, digits, and inner
/// hyphens.
fn valid_namespace(namespace: &str) -> bool {
    (1..=64).contains(&namespace.len())
        && !namespace.starts_with('-')
        && !namespace.ends_with('-')
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// At least two words separated by dots.
pub(crate) fn valid_name(name: &str) -> bool {
    name.split('.').count() >= 2 && name.split('.').all(valid_word)
}

fn description_sha256(manifest: &str, files: &[(String, String)]) -> String {
    let mut parts: Vec<(&str, &str)> = files
        .iter()
        .filter(|(path, _)| {
            FOLDERS
                .iter()
                .any(|(folder, _)| path.starts_with(&format!("{folder}/")))
        })
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    parts.sort_unstable();
    let mut hasher = Sha256::new();
    for (path, text) in std::iter::once(("anatomia.toml", manifest)).chain(parts) {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn parse_toml(path: &str, text: &str) -> Result<Table, AnatomiaError> {
    text.parse::<Table>().map_err(|error| AnatomiaError::Toml {
        path: PathBuf::from(path),
        message: error.message().to_owned(),
    })
}

fn read_text(path: &Path) -> Result<String, AnatomiaError> {
    std::fs::read_to_string(path).map_err(|error| AnatomiaError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    })
}

/// Collects the `.toml` files of a folder, with their paths relative to the
/// description's directory.
fn collect(
    root: &Path,
    folder: &str,
    nested: bool,
    files: &mut Vec<(String, String)>,
) -> Result<(), AnatomiaError> {
    let directory = root.join(folder);
    if !directory.exists() {
        return Ok(());
    }
    let io = |error: std::io::Error| AnatomiaError::Io {
        path: directory.clone(),
        message: error.to_string(),
    };
    let mut listing = std::fs::read_dir(&directory)
        .map_err(io)?
        .map(|item| item.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(io)?;
    listing.sort();
    for path in listing {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let relative = format!("{folder}/{file_name}");
        if nested && path.is_dir() {
            collect(root, &relative, true, files)?;
        } else if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "toml")
        {
            files.push((relative, read_text(&path)?));
        } else {
            return Err(AnatomiaError::UnexpectedFile { path });
        }
    }
    Ok(())
}
