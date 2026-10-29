//! Converts a game map of format v2 into Anatomia 1 and the Lens catalog.
//!
//! The map is first read and validated as v2; the conversion then works on
//! its files as written, so nothing the v2 reader drops is dropped here
//! either. Every v2 construct has a place in Anatomia or the catalog; one
//! that has none (a `patch`) stops the conversion instead of being left
//! out. TOML comments are not data and are not carried over.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use toml::{Table, Value};

use crate::record::quoted;
use crate::{Anatomia, FORMAT_MAJOR, FORMAT_MINOR};

/// The extension that keeps where an entry came from, as the license of
/// its source requires.
pub const ORIGIN: &str = "angelica.origin";
/// The extension that keeps a field's v2 entry description where the
/// object model's description became the field's own.
pub const NOTES: &str = "angelica.notes";

/// The files of a converted description, by path relative to its
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub files: BTreeMap<String, String>,
    /// Every v2 entry name whose Anatomia name differs, and that name.
    pub renamed: BTreeMap<String, String>,
}

/// A v2 map's files, as written.
struct Source {
    entries: Vec<(String, Table)>,
    types: Vec<(String, Table)>,
    enums: Vec<(String, Table)>,
    root_files: BTreeMap<String, Table>,
    copied: Vec<(String, String)>,
    builds: Vec<(String, String)>,
}

/// Converts the v2 map in `directory`.
///
/// # Errors
///
/// A map that is not valid v2, and a construct Anatomia has no place for.
pub fn convert_map(directory: &Path) -> Result<Converted, String> {
    angelica_map::Map::load(directory)
        .map_err(|error| format!("the map is not valid v2: {error}"))?;
    let source = read_source(directory)?;
    convert(&source)
}

/// Converts a v2 map given as the text of its `map.toml` and of its other
/// files, each by its path relative to the map directory, as
/// `angelica_map::Map::from_sources` takes them; build records
/// (`builds/<version>.toml`) and `THIRD_PARTY_NOTICES.md` may be among
/// them. A file in no place of the map is not read.
///
/// # Errors
///
/// As [`convert_map`].
pub fn convert_sources(map_toml: &str, files: &[(String, String)]) -> Result<Converted, String> {
    let map: Vec<(String, String)> = files
        .iter()
        .filter(|(path, _)| {
            ["entries/", "sheets/", "types/", "enums/", "windows/"]
                .iter()
                .any(|folder| path.starts_with(folder))
                || angelica_map::ROOT_FILES.contains(&path.as_str())
        })
        .cloned()
        .collect();
    angelica_map::Map::from_sources(map_toml, &map)
        .map_err(|error| format!("the map is not valid v2: {error}"))?;
    convert(&source_of(files)?)
}

/// The files of a map given by path, sorted into the places
/// [`read_source`] reads from a directory.
fn source_of(files: &[(String, String)]) -> Result<Source, String> {
    let mut files: Vec<&(String, String)> = files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let parse = |path: &str, text: &str| -> Result<Table, String> {
        text.parse::<Table>()
            .map_err(|error| format!("{path}: {}", error.message()))
    };
    // The name of a file directly in `folder`, without `.toml`.
    let stem = |path: &str, folder: &str| -> Option<String> {
        path.strip_prefix(folder)?
            .strip_prefix('/')
            .filter(|name| !name.contains('/'))?
            .strip_suffix(".toml")
            .map(str::to_owned)
    };
    let mut source = Source {
        entries: Vec::new(),
        types: Vec::new(),
        enums: Vec::new(),
        root_files: BTreeMap::new(),
        copied: Vec::new(),
        builds: Vec::new(),
    };
    for (path, text) in files {
        if let Some(name) = stem(path, "entries") {
            source.entries.push((name, parse(path, text)?));
        } else if let Some(name) = stem(path, "types") {
            source.types.push((name, parse(path, text)?));
        } else if let Some(name) = stem(path, "enums") {
            source.enums.push((name, parse(path, text)?));
        } else if angelica_map::ROOT_FILES.contains(&path.as_str()) {
            source.root_files.insert(
                path.trim_end_matches(".toml").to_owned(),
                parse(path, text)?,
            );
        } else if stem(path, "windows").is_some()
            || path.starts_with("sheets/")
            || path == "THIRD_PARTY_NOTICES.md"
        {
            source.copied.push((path.clone(), text.clone()));
        } else if let Some(version) = stem(path, "builds") {
            source.builds.push((version, text.clone()));
        }
    }
    Ok(source)
}

fn read_source(directory: &Path) -> Result<Source, String> {
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let parse = |path: &Path| -> Result<Table, String> {
        read(path)?
            .parse::<Table>()
            .map_err(|error| format!("{}: {}", path.display(), error.message()))
    };
    let listing = |folder: &str| -> Result<Vec<std::path::PathBuf>, String> {
        let path = directory.join(folder);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut items = std::fs::read_dir(&path)
            .and_then(|items| {
                items
                    .map(|item| item.map(|item| item.path()))
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error| format!("{}: {error}", path.display()))?;
        items.sort();
        Ok(items)
    };
    let stem = |path: &Path| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    };
    let mut source = Source {
        entries: Vec::new(),
        types: Vec::new(),
        enums: Vec::new(),
        root_files: BTreeMap::new(),
        copied: Vec::new(),
        builds: Vec::new(),
    };
    for path in listing("entries")? {
        source.entries.push((stem(&path), parse(&path)?));
    }
    for path in listing("types")? {
        source.types.push((stem(&path), parse(&path)?));
    }
    for path in listing("enums")? {
        source.enums.push((stem(&path), parse(&path)?));
    }
    for file in angelica_map::ROOT_FILES {
        let path = directory.join(file);
        if path.exists() {
            source
                .root_files
                .insert(file.trim_end_matches(".toml").to_owned(), parse(&path)?);
        }
    }
    for path in listing("windows")? {
        source
            .copied
            .push((format!("windows/{}.toml", stem(&path)), read(&path)?));
    }
    let mut folders = vec!["sheets".to_owned()];
    while let Some(folder) = folders.pop() {
        for path in listing(&folder)? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                folders.push(format!("{folder}/{name}"));
            } else {
                source
                    .copied
                    .push((format!("{folder}/{name}"), read(&path)?));
            }
        }
    }
    // The attribution `origin` entries rely on goes with the description.
    let notices = directory.join("THIRD_PARTY_NOTICES.md");
    if notices.exists() {
        source
            .copied
            .push(("THIRD_PARTY_NOTICES.md".to_owned(), read(&notices)?));
    }
    for path in listing("builds")? {
        source.builds.push((stem(&path), read(&path)?));
    }
    Ok(source)
}

/// An object model field as written, with the type that declares it.
struct ObjectField<'a> {
    type_name: &'a str,
    name: &'a str,
    raw: &'a Table,
}

/// A file's sections: each a header and its keys in order.
type Sections = Vec<(String, Vec<(String, Value)>)>;

#[derive(Default)]
struct Output {
    /// Sections by file, each a header and its keys in order.
    files: BTreeMap<String, Sections>,
}

impl Output {
    fn section(&mut self, file: &str, header: String, keys: Vec<(String, Value)>) {
        self.files
            .entry(file.to_owned())
            .or_default()
            .push((header, keys));
    }
}

fn area(name: &str) -> &str {
    name.split('.').next().unwrap_or_default()
}

fn string(table: &Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[allow(clippy::too_many_lines)]
fn convert(source: &Source) -> Result<Converted, String> {
    let mut out = Output::default();

    // The object model's types and fields, by the entry each field reads.
    let mut types: BTreeMap<&str, &Table> = BTreeMap::new();
    for (_, file) in &source.types {
        for (name, value) in file
            .get("types")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            types.insert(name, value.as_table().ok_or("a type is a table")?);
        }
    }
    let mut by_entry: BTreeMap<String, Vec<ObjectField<'_>>> = BTreeMap::new();
    for (type_name, definition) in &types {
        for (name, value) in definition
            .get("fields")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            let raw = value.as_table().ok_or("a field is a table")?;
            let at = string(raw, "at").unwrap_or_else(|| format!("{type_name}.{name}"));
            by_entry.entry(at).or_default().push(ObjectField {
                type_name,
                name,
                raw,
            });
        }
    }

    // Every entry by name, with its kind.
    let mut entries: BTreeMap<&str, (&str, &Table)> = BTreeMap::new();
    for (_, file) in &source.entries {
        for (kind, items) in file {
            if kind == "patch" {
                return Err(
                    "a `patch` entry has no place in Anatomia: a plugin never changes code"
                        .to_owned(),
                );
            }
            for (name, value) in items.as_table().into_iter().flatten() {
                entries.insert(name, (kind, value.as_table().ok_or("an entry is a table")?));
            }
        }
    }

    // Each field entry's name in Anatomia: the object model's field that
    // reads it, or the entry's own member name.
    let mut renamed = BTreeMap::new();
    for (name, (kind, raw)) in &entries {
        if *kind != "field" {
            continue;
        }
        let of = string(raw, "of").unwrap_or_default();
        let plain: Vec<&ObjectField<'_>> = by_entry
            .get(*name)
            .into_iter()
            .flatten()
            .filter(|field| !field.raw.contains_key("bit"))
            .collect();
        if plain.len() > 1 {
            return Err(format!(
                "`{name}` is read by more than one field of the object model"
            ));
        }
        let member = name
            .rsplit_once('.')
            .map(|(_, member)| member)
            .unwrap_or_default();
        let field = plain.first().map_or(member, |field| field.name);
        let full = format!("{of}.{field}");
        if full != *name {
            renamed.insert((*name).to_owned(), full);
        }
    }
    let rename = |name: &str| {
        renamed
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    };

    // Roots give their globals a type.
    let mut root_types: BTreeMap<String, String> = BTreeMap::new();
    if let Some(roots) = source
        .root_files
        .get("roots")
        .and_then(|roots| roots.get("roots"))
        .and_then(Value::as_table)
    {
        for root in roots.values().filter_map(Value::as_table) {
            if let (Some(global), Some(ty)) = (string(root, "global"), string(root, "type")) {
                root_types.entry(global).or_insert(ty);
            }
        }
    }

    // Structures: their v2 type, element sizes from strides, text fields.
    let mut structures: BTreeSet<String> = types.keys().map(|name| (*name).to_owned()).collect();
    let mut sizes: BTreeMap<String, u32> = BTreeMap::new();
    let mut text_fields: BTreeMap<String, String> = BTreeMap::new();
    let mut fields: BTreeMap<String, Sections> = BTreeMap::new();
    let mut meaning: BTreeMap<String, Sections> = BTreeMap::new();
    let mut changes: Sections = Vec::new();

    for (name, (kind, raw)) in &entries {
        let mut keys: Table = (*raw).clone();
        let doc = keys.remove("doc");
        let checks = keys.remove("checks");
        let depends = keys.remove("depends");
        let origin = keys.remove("origin");
        let locator = keys.remove("locator");
        let mut section: Vec<(String, Value)> = Vec::new();
        let ext = |section: &mut Vec<(String, Value)>, notes: Option<Value>| {
            let mut ext = Table::new();
            if let Some(origin) = &origin {
                ext.insert(ORIGIN.to_owned(), origin.clone());
            }
            if let Some(notes) = notes {
                let mut table = Table::new();
                table.insert("entry_doc".to_owned(), notes);
                ext.insert(NOTES.to_owned(), Value::Table(table));
            }
            if !ext.is_empty() {
                section.push(("ext".to_owned(), Value::Table(ext)));
            }
        };
        let location = |section: &mut Vec<(String, Value)>| -> Result<(), String> {
            if let Some(locator) = &locator {
                section.push((
                    "find".to_owned(),
                    steps(locator.as_table().ok_or("a locator is a table")?, &rename)?,
                ));
            }
            if let Some(checks) = &checks {
                let converted = checks
                    .as_array()
                    .ok_or("`checks` is an array")?
                    .iter()
                    .map(|check| {
                        convert_check(check.as_table().ok_or("a check is a table")?, &rename)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                section.push(("checks".to_owned(), Value::Array(converted)));
            }
            if let Some(depends) = &depends {
                let names = depends
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|name| Value::String(rename(name)))
                    .collect();
                section.push(("depends".to_owned(), Value::Array(names)));
            }
            Ok(())
        };
        match *kind {
            "function" => {
                let abi = keys
                    .remove("abi")
                    .and_then(|abi| abi.as_table().cloned())
                    .ok_or("a function has `abi`")?;
                let args: Vec<String> = abi
                    .get("args")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                let ret = string(&abi, "ret").unwrap_or_else(|| "void".to_owned());
                let mut ty = format!("fn({})", args.join(", "));
                if ret != "void" {
                    let _ = write!(ty, " -> {ret}");
                }
                section.extend(doc.map(|doc| ("doc".to_owned(), doc)));
                section.push(("type".to_owned(), Value::String(ty)));
                section.push((
                    "thread".to_owned(),
                    keys.remove("thread").ok_or("a function has `thread`")?,
                ));
                location(&mut section)?;
                ext(&mut section, None);
                out.section(
                    &format!("functions/{}.toml", area(name)),
                    format!("function.{}", quoted(name)),
                    section,
                );
            }
            "location" => {
                section.extend(doc.map(|doc| ("doc".to_owned(), doc)));
                section.push((
                    "thread".to_owned(),
                    keys.remove("thread").ok_or("a location has `thread`")?,
                ));
                location(&mut section)?;
                ext(&mut section, None);
                out.section(
                    &format!("functions/{}.toml", area(name)),
                    format!("location.{}", quoted(name)),
                    section,
                );
            }
            "global" => {
                let value = string(raw, "type").unwrap_or_default();
                keys.remove("type");
                let ty = match (value.as_str(), root_types.get(*name)) {
                    ("ptr", Some(root)) => format!("ptr<{root}>"),
                    ("struct", Some(root)) => root.clone(),
                    ("struct", None) => "opaque".to_owned(),
                    (other, _) => other.to_owned(),
                };
                section.extend(doc.map(|doc| ("doc".to_owned(), doc)));
                section.push(("type".to_owned(), Value::String(ty)));
                location(&mut section)?;
                ext(&mut section, None);
                out.section(
                    &format!("globals/{}.toml", area(name)),
                    format!("global.{}", quoted(name)),
                    section,
                );
            }
            "vtable" => {
                section.extend(doc.map(|doc| ("doc".to_owned(), doc)));
                section.push((
                    "code_slots".to_owned(),
                    keys.remove("slots").ok_or("a vtable has `slots`")?,
                ));
                location(&mut section)?;
                ext(&mut section, None);
                out.section(
                    &format!("globals/{}.toml", area(name)),
                    format!("vtable.{}", quoted(name)),
                    section,
                );
            }
            "field" => {
                let of = string(raw, "of").ok_or("a field has `of`")?;
                let value = string(raw, "type").ok_or("a field has `type`")?;
                keys.remove("of");
                keys.remove("type");
                structures.insert(of.clone());
                let readers = by_entry.get(*name).map(Vec::as_slice).unwrap_or_default();
                let plain = readers.iter().find(|field| !field.raw.contains_key("bit"));
                let full = rename(name);
                let field_name = full
                    .rsplit_once('.')
                    .map(|(_, field)| field.to_owned())
                    .unwrap_or_default();
                let mut object_keys = Vec::new();
                let ty = match plain {
                    None => match value.as_str() {
                        "struct" => "opaque".to_owned(),
                        other => other.to_owned(),
                    },
                    Some(field) => {
                        let (ty, keys) = object_shape(
                            field,
                            &value,
                            &entries,
                            &renamed,
                            &mut sizes,
                            &mut text_fields,
                        )?;
                        object_keys = keys;
                        ty
                    }
                };
                let object_doc = plain.and_then(|field| field.raw.get("doc").cloned());
                let (field_doc, notes) = match (object_doc, doc) {
                    (Some(object), entry) => (Some(object), entry),
                    (None, entry) => (entry, None),
                };
                section.extend(field_doc.map(|doc| ("doc".to_owned(), doc)));
                section.push(("type".to_owned(), Value::String(ty)));
                if let Some(offset) = keys.remove("offset") {
                    section.push(("at".to_owned(), offset));
                }
                location(&mut section)?;
                section.extend(object_keys);
                // The v2 name stays usable: as a field name of the same
                // structure, or whole when the entry was named apart from it.
                if full != *name {
                    let alias = match name.rsplit_once('.') {
                        Some((holder, member)) if holder == of => member.to_owned(),
                        _ => (*name).to_owned(),
                    };
                    section.push((
                        "aliases".to_owned(),
                        Value::Array(vec![Value::String(alias)]),
                    ));
                }
                ext(&mut section, notes);
                fields
                    .entry(of.clone())
                    .or_default()
                    .push((field_name.clone(), section));
                if let Some(field) = plain {
                    object_meaning(field, &full, &mut meaning, &mut changes);
                }
                for bit in readers.iter().filter(|field| field.raw.contains_key("bit")) {
                    let mut bit_section = Vec::new();
                    bit_section.extend(
                        bit.raw
                            .get("doc")
                            .cloned()
                            .map(|doc| ("doc".to_owned(), doc)),
                    );
                    bit_section.push(("type".to_owned(), Value::String("bool".to_owned())));
                    let mut bits = Table::new();
                    bits.insert("in".to_owned(), Value::String(field_name.clone()));
                    bits.insert(
                        "at".to_owned(),
                        bit.raw.get("bit").cloned().unwrap_or(Value::Integer(0)),
                    );
                    bit_section.push(("bits".to_owned(), Value::Table(bits)));
                    for (key, value) in bit.raw {
                        if !matches!(
                            key.as_str(),
                            "doc" | "at" | "bit" | "domain" | "forbidden" | "set" | "whose" | "as"
                        ) {
                            return Err(format!(
                                "`{}.{}`: bit field key `{key}` has no place",
                                bit.type_name, bit.name
                            ));
                        }
                        let _ = value;
                    }
                    fields
                        .entry(of.clone())
                        .or_default()
                        .push((bit.name.to_owned(), bit_section));
                    object_meaning(
                        bit,
                        &format!("{of}.{}", bit.name),
                        &mut meaning,
                        &mut changes,
                    );
                }
            }
            other => return Err(format!("`{name}`: unknown kind `{other}`")),
        }
        if let Some(key) = keys.keys().next() {
            return Err(format!(
                "`{name}`: key `{key}` has no place in the conversion"
            ));
        }
    }

    // A field of the object model that reads no entry cannot exist in a
    // valid map; make sure none was skipped.
    for (entry, readers) in &by_entry {
        if !entries.contains_key(entry.as_str()) {
            let field = &readers[0];
            return Err(format!(
                "`{}.{}` reads `{entry}`, which is not an entry",
                field.type_name, field.name
            ));
        }
    }

    for structure in &structures {
        let definition = types.get(structure.as_str());
        let mut section = Vec::new();
        if let Some(definition) = definition {
            for key in definition.keys() {
                if !matches!(
                    key.as_str(),
                    "doc" | "domain" | "extends" | "variant" | "fields"
                ) {
                    return Err(format!("type `{structure}`: key `{key}` has no place"));
                }
            }
            section.extend(
                definition
                    .get("doc")
                    .cloned()
                    .map(|doc| ("doc".to_owned(), doc)),
            );
        }
        if let Some(size) = sizes.get(structure) {
            section.push(("size".to_owned(), Value::Integer(i64::from(*size))));
        }
        if let Some(base) = definition.and_then(|definition| string(definition, "extends")) {
            let mut extends = Table::new();
            extends.insert("type".to_owned(), Value::String(base));
            section.push((
                "extends".to_owned(),
                Value::Array(vec![Value::Table(extends)]),
            ));
        }
        if let Some(variant) = definition.and_then(|definition| definition.get("variant")) {
            section.push(("variant".to_owned(), variant.clone()));
        }
        if let Some(text) = text_fields.get(structure) {
            section.push(("text".to_owned(), Value::String(text.clone())));
        }
        let file = format!("types/{}.toml", area(structure));
        out.section(&file, format!("struct.{}", quoted(structure)), section);
        for (field, keys) in fields.remove(structure).unwrap_or_default() {
            out.section(
                &file,
                format!("struct.{}.fields.{field}", quoted(structure)),
                keys,
            );
        }
        if let Some(domain) = definition.and_then(|definition| definition.get("domain")) {
            meaning
                .entry(area(structure).to_owned())
                .or_default()
                .push((
                    format!("types.{}", quoted(structure)),
                    vec![("domain".to_owned(), domain.clone())],
                ));
        }
    }

    for (name, file) in &source.enums {
        let mut section = Vec::new();
        for (key, value) in file {
            if !matches!(key.as_str(), "doc" | "flags" | "values") {
                return Err(format!("enum `{name}`: key `{key}` has no place"));
            }
            section.push((key.clone(), value.clone()));
        }
        out.section(
            &format!("types/{}.toml", area(name)),
            format!("enum.{}", quoted(name)),
            section,
        );
    }

    if let Some(domains) = source.root_files.get("domains") {
        for (name, domain) in domains
            .get("domains")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            let domain = domain.as_table().ok_or("a domain is a table")?;
            out.section(
                "meaning/domains.toml",
                format!("domains.{name}"),
                domain
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            );
        }
    }
    for (area, sections) in meaning {
        for (header, keys) in sections {
            out.section(&format!("meaning/{area}.toml"), header, keys);
        }
    }

    let mut files = BTreeMap::new();
    files.insert(
        "anatomia.toml".to_owned(),
        format!(
            "format = \"{FORMAT_MAJOR}.{FORMAT_MINOR}\"\nnamespace = \"ffxiv\"\nextensions = [{}, {}]\n",
            quoted(ORIGIN),
            quoted(NOTES)
        ),
    );
    for (path, sections) in out.files {
        let mut text = String::new();
        for (header, keys) in sections {
            if !text.is_empty() {
                text.push('\n');
            }
            let _ = writeln!(text, "[{header}]");
            for (key, value) in keys {
                let _ = writeln!(text, "{} = {}", bare(&key), inline(&key, &value));
            }
        }
        files.insert(path, text);
    }

    for file in ["roots", "events", "actions"] {
        if let Some(table) = source.root_files.get(file) {
            files.insert(format!("lens/{file}.toml"), emit_table(table));
        }
    }
    // The structures and fields Lens offers: those of the object model.
    let mut offered = String::new();
    for (type_name, definition) in &types {
        let names: Vec<String> = definition
            .get("fields")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
            .map(|(name, _)| quoted(name))
            .collect();
        if !offered.is_empty() {
            offered.push('\n');
        }
        let _ = writeln!(
            offered,
            "[types.{}]\nfields = [{}]",
            quoted(type_name),
            names.join(", ")
        );
    }
    if !offered.is_empty() {
        files.insert("lens/types.toml".to_owned(), offered);
    }
    if !changes.is_empty() {
        let mut text = String::new();
        for (name, keys) in changes {
            if !text.is_empty() {
                text.push('\n');
            }
            let _ = writeln!(text, "[changes.{}]", quoted(&name));
            for (key, value) in keys {
                let _ = writeln!(text, "{} = {}", bare(&key), inline(&key, &value));
            }
        }
        files.insert("lens/changes.toml".to_owned(), text);
    }
    for (path, text) in &source.copied {
        files.insert(path.clone(), text.clone());
    }

    // Build records keep their results under the new names.
    let anatomia = described(&files)?;
    if let Some((name, reason)) = anatomia.broken().iter().next() {
        return Err(format!(
            "the converted description does not use `{name}`: {reason}"
        ));
    }
    for (version, text) in &source.builds {
        files.insert(
            format!("builds/{version}.toml"),
            convert_record(text, &rename, anatomia.sha256())?,
        );
    }
    Ok(Converted { files, renamed })
}

impl Converted {
    /// The description the converted files hold.
    ///
    /// # Errors
    ///
    /// None for a conversion [`convert_map`] or [`convert_sources`] made.
    pub fn anatomia(&self) -> Result<Anatomia, String> {
        described(&self.files)
    }
}

/// The description of converted `files`, which the conversion checked.
fn described(files: &BTreeMap<String, String>) -> Result<Anatomia, String> {
    let sources: Vec<(String, String)> = files
        .iter()
        .filter(|(path, _)| {
            [
                "types/",
                "functions/",
                "globals/",
                "meaning/",
                "sheets/",
                "windows/",
            ]
            .iter()
            .any(|folder| path.starts_with(folder))
        })
        .map(|(path, text)| (path.clone(), text.clone()))
        .collect();
    let manifest = files
        .get("anatomia.toml")
        .ok_or("the converted description has no anatomia.toml")?;
    Anatomia::from_sources(manifest, &sources)
        .map_err(|error| format!("the converted description cannot be read: {error}"))
}

/// The Anatomia type and the keys of a field the object model reads.
#[allow(clippy::too_many_lines)]
fn object_shape(
    field: &ObjectField<'_>,
    value: &str,
    entries: &BTreeMap<&str, (&str, &Table)>,
    renamed: &BTreeMap<String, String>,
    sizes: &mut BTreeMap<String, u32>,
    text_fields: &mut BTreeMap<String, String>,
) -> Result<(String, Vec<(String, Value)>), String> {
    let raw = field.raw;
    let full = format!("{}.{}", field.type_name, field.name);
    let mut keys = Vec::new();
    for key in ["sheet", "enum", "flags", "element", "unit"] {
        if let Some(value) = raw.get(key) {
            keys.push((key.to_owned(), value.clone()));
        }
    }
    for key in raw.keys() {
        if !matches!(
            key.as_str(),
            "doc"
                | "at"
                | "text"
                | "ref"
                | "embed"
                | "array"
                | "sheet"
                | "enum"
                | "flags"
                | "element"
                | "unit"
                | "pointer"
                | "domain"
                | "forbidden"
                | "set"
                | "whose"
                | "as"
        ) {
            return Err(format!("`{full}`: key `{key}` has no place"));
        }
    }
    let ty = if let Some(most) = raw.get("text") {
        match (value, raw.get("pointer").and_then(Value::as_str)) {
            ("struct", Some(pointer)) => {
                let (_, pointer_entry) = entries
                    .get(pointer)
                    .ok_or_else(|| format!("`{full}`: no entry `{pointer}`"))?;
                let holder = string(pointer_entry, "of").ok_or("a field has `of`")?;
                // The holder's field that holds the pointer, by its name in
                // Anatomia.
                let text_field = renamed
                    .get(pointer)
                    .map_or(pointer, String::as_str)
                    .rsplit_once('.')
                    .map(|(_, field)| field.to_owned())
                    .unwrap_or_default();
                if let Some(existing) = text_fields.insert(holder.clone(), text_field.clone())
                    && existing != text_field
                {
                    return Err(format!("`{holder}` reads as text through two fields"));
                }
                keys.push(("text".to_owned(), most.clone()));
                holder
            }
            ("struct", None) => {
                keys.push(("text".to_owned(), Value::Boolean(true)));
                format!("char8[{}]", most.as_integer().unwrap_or(0))
            }
            ("ptr", None) => {
                keys.push(("text".to_owned(), most.clone()));
                "ptr<char8>".to_owned()
            }
            _ => return Err(format!("`{full}`: text of a `{value}` entry has no place")),
        }
    } else if let Some(target) = raw.get("ref").and_then(Value::as_str) {
        format!("ptr<{target}>")
    } else if let Some(target) = raw.get("embed").and_then(Value::as_str) {
        target.to_owned()
    } else if let Some(array) = raw.get("array").and_then(Value::as_table) {
        let of = string(array, "of").ok_or("an array has `of`")?;
        let pointers = array
            .get("pointers")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let stride = array
            .get("stride")
            .and_then(Value::as_integer)
            .and_then(|stride| u32::try_from(stride).ok())
            .ok_or("an array has `stride`")?;
        let element = if pointers {
            if stride != 8 {
                return Err(format!("`{full}`: an array of pointers has stride 8"));
            }
            format!("ptr<{of}>")
        } else {
            if let Some(existing) = sizes.insert(of.clone(), stride)
                && existing != stride
            {
                return Err(format!(
                    "`{of}` is an element of arrays with strides {existing} and {stride}"
                ));
            }
            of
        };
        let length = match (array.get("length"), array.get("count"), array.get("most")) {
            (Some(length), None, None) => length.as_integer().unwrap_or(0),
            (None, Some(count), Some(most)) => {
                keys.push(("count".to_owned(), count.clone()));
                most.as_integer().unwrap_or(0)
            }
            _ => {
                return Err(format!(
                    "`{full}`: an array has `length` or `count` with `most`"
                ));
            }
        };
        if let Some(present) = array.get("present") {
            keys.push(("present".to_owned(), present.clone()));
        }
        for key in array.keys() {
            if !matches!(
                key.as_str(),
                "of" | "length"
                    | "count"
                    | "most"
                    | "stride"
                    | "pointers"
                    | "present"
                    | "partition"
            ) {
                return Err(format!("`{full}`: array key `{key}` has no place"));
            }
        }
        let in_place = format!("{element}[{length}]");
        match value {
            "struct" => in_place,
            "ptr" => format!("ptr<{in_place}>"),
            _ => {
                return Err(format!(
                    "`{full}`: an array of a `{value}` entry has no place"
                ));
            }
        }
    } else {
        value.to_owned()
    };
    Ok((ty, keys))
}

/// Puts a field's domain, consent, and changes where they belong.
fn object_meaning(
    field: &ObjectField<'_>,
    full: &str,
    meaning: &mut BTreeMap<String, Sections>,
    changes: &mut Sections,
) {
    let mut keys = Vec::new();
    for key in ["domain", "forbidden", "whose", "as"] {
        if let Some(value) = field.raw.get(key) {
            keys.push((key.to_owned(), value.clone()));
        }
    }
    if let Some(partition) = field
        .raw
        .get("array")
        .and_then(Value::as_table)
        .and_then(|array| array.get("partition"))
    {
        keys.push(("partition".to_owned(), partition.clone()));
    }
    if !keys.is_empty() {
        meaning
            .entry(area(full).to_owned())
            .or_default()
            .push((format!("fields.{}", quoted(full)), keys));
    }
    match field.raw.get("set") {
        None => {}
        Some(Value::Table(set)) => {
            changes.push((
                full.to_owned(),
                set.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ));
        }
        Some(_) => changes.push((full.to_owned(), Vec::new())),
    }
}

/// A v2 locator as a `find` pipeline.
fn steps(locator: &Table, rename: &dyn Fn(&str) -> String) -> Result<Value, String> {
    let step = |key: &str, value: Value| {
        let mut table = Table::new();
        table.insert(key.to_owned(), value);
        Value::Table(table)
    };
    let mut steps = Vec::new();
    for key in locator.keys() {
        if !matches!(
            key.as_str(),
            "signature" | "at" | "follow" | "vtable" | "slot" | "in" | "operand"
        ) {
            return Err(format!("locator key `{key}` has no place"));
        }
    }
    if let Some(function) = string(locator, "in") {
        steps.push(step("in", Value::String(rename(&function))));
        steps.push(step(
            "scan",
            locator
                .get("signature")
                .cloned()
                .ok_or("an `in` locator has `signature`")?,
        ));
        steps.push(step("operand", Value::String("disp".to_owned())));
    } else if let Some(vtable) = string(locator, "vtable") {
        let mut table = Table::new();
        table.insert("vtable".to_owned(), Value::String(rename(&vtable)));
        table.insert(
            "slot".to_owned(),
            locator
                .get("slot")
                .cloned()
                .ok_or("a vtable locator has `slot`")?,
        );
        steps.push(Value::Table(table));
    } else {
        steps.push(step(
            "scan",
            locator
                .get("signature")
                .cloned()
                .ok_or("a locator has `signature`")?,
        ));
        let at = locator.get("at").and_then(Value::as_integer).unwrap_or(0);
        match string(locator, "follow").as_deref() {
            Some("rel32") => steps.push(step("rel32", Value::Integer(at))),
            Some(other) => return Err(format!("`follow = \"{other}\"` has no place")),
            None if at != 0 => steps.push(step("offset", Value::Integer(at))),
            None => {}
        }
    }
    Ok(Value::Array(steps))
}

fn convert_check(check: &Table, rename: &dyn Fn(&str) -> String) -> Result<Value, String> {
    let mut table = Table::new();
    if let Some(signature) = check.get("contains") {
        table.insert("contains".to_owned(), signature.clone());
        if let Some(function) = string(check, "in") {
            table.insert("in".to_owned(), Value::String(rename(&function)));
        }
    } else if let Some(name) = string(check, "equals") {
        table.insert("equals".to_owned(), Value::String(rename(&name)));
    } else if let Some(slot) = check.get("vtable_slot") {
        table.insert("vtable_slot".to_owned(), slot.clone());
        table.insert(
            "code_slots".to_owned(),
            check
                .get("code_slots")
                .cloned()
                .ok_or("`vtable_slot` has `code_slots`")?,
        );
    } else {
        table.insert("find".to_owned(), steps(check, rename)?);
    }
    Ok(Value::Table(table))
}

fn convert_record(
    text: &str,
    rename: &dyn Fn(&str) -> String,
    sha256: &str,
) -> Result<String, String> {
    let table: Table = text
        .parse()
        .map_err(|error: toml::de::Error| error.message().to_owned())?;
    let get = |key: &str| string(&table, key).ok_or_else(|| format!("a build record has `{key}`"));
    let mut out = String::from("# Generated by `angelica anatomia verify`. Do not edit.\n");
    let _ = writeln!(out, "game_version = {}", quoted(&get("game_version")?));
    let _ = writeln!(out, "exe_sha256 = {}", quoted(&get("exe_sha256")?));
    let _ = writeln!(out, "anatomia_sha256 = {}", quoted(sha256));
    let renamed = |key: &str| -> BTreeMap<String, Value> {
        table
            .get(key)
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
            .map(|(name, value)| (rename(name), value.clone()))
            .collect()
    };
    out.push_str("\n[present]\n");
    for (name, value) in renamed("present") {
        let _ = writeln!(
            out,
            "{} = {:#X}",
            quoted(&name),
            value.as_integer().unwrap_or(0)
        );
    }
    out.push_str("\n[absent]\n");
    for (name, value) in renamed("absent") {
        let _ = writeln!(
            out,
            "{} = {}",
            quoted(&name),
            quoted(value.as_str().unwrap_or_default())
        );
    }
    let mut confirmed: Vec<String> = table
        .get("constant_offsets")
        .and_then(|constants| constants.get("names"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(rename)
        .collect();
    confirmed.sort();
    out.push_str("\n[confirmed]\noffsets = [");
    out.push_str(
        &confirmed
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str("]\n");
    Ok(out)
}

/// A key as TOML writes it: bare when it can be.
fn bare(key: &str) -> String {
    if !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        key.to_owned()
    } else {
        quoted(key)
    }
}

/// A value on one line. Offsets and sizes are hexadecimal.
fn inline(key: &str, value: &Value) -> String {
    match value {
        Value::String(text) => quoted(text),
        Value::Integer(number) if matches!(key, "at" | "size") && *number >= 10 => {
            format!("{number:#X}")
        }
        Value::Integer(number) => number.to_string(),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(|item| inline(key, item)).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Table(table) => {
            if table.is_empty() {
                return "{}".to_owned();
            }
            let items: Vec<String> = table
                .iter()
                .map(|(key, value)| format!("{} = {}", bare(key), inline("", value)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
        other => other.to_string(),
    }
}

/// A catalog file: one section per item, with the item's tables of tables
/// (`args`, `outcomes`) as sections of their own.
fn emit_table(table: &Table) -> String {
    fn emit(text: &mut String, header: &str, table: &Table) {
        let (sections, keys): (Vec<_>, Vec<_>) = table.iter().partition(|(_, value)| {
            value
                .as_table()
                .is_some_and(|inner| !inner.is_empty() && inner.values().all(Value::is_table))
        });
        if !keys.is_empty() || sections.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            let _ = writeln!(text, "[{header}]");
            for (key, value) in keys {
                let _ = writeln!(text, "{} = {}", bare(key), inline(key, value));
            }
        }
        for (key, value) in sections {
            for (name, inner) in value.as_table().into_iter().flatten() {
                if let Some(inner) = inner.as_table() {
                    emit(
                        text,
                        &format!("{header}.{}.{}", bare(key), bare(name)),
                        inner,
                    );
                }
            }
        }
    }
    let mut text = String::new();
    for (kind, items) in table {
        for (name, item) in items.as_table().into_iter().flatten() {
            if let Some(item) = item.as_table() {
                emit(&mut text, &format!("{}.{}", bare(kind), bare(name)), item);
            }
        }
    }
    text
}
