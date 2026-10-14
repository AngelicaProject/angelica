use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::MapError;
use crate::model::{Entry, EntryKind, Locator, RawFile};

/// The map format version this reader supports.
pub const FORMAT: i64 = 2;

/// A map whose entries are valid and reference each other without cycles.
#[derive(Debug, Clone)]
pub struct Map {
    entries: BTreeMap<String, Entry>,
    sheets: BTreeMap<String, crate::SheetSchema>,
    objects: crate::objects::Objects,
    windows: BTreeMap<String, crate::WindowSchema>,
    /// Entry names, each after every entry it references.
    order: Vec<String>,
    entries_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMapFile {
    format: Option<i64>,
}

impl Map {
    /// Reads a map directory: `map.toml`, `domains.toml`, `roots.toml`, every
    /// file in `entries/`, `types/`, `enums/`,
    /// and `windows/`, and every file under `sheets/`.
    ///
    /// # Errors
    ///
    /// Returns a [`MapError`] when a file cannot be read or the map is not
    /// valid.
    pub fn load(directory: &Path) -> Result<Self, MapError> {
        let map_toml = read_text(&directory.join("map.toml"))?;
        let entries_directory = directory.join("entries");
        let mut files = Vec::new();
        if entries_directory.exists() {
            let listing = std::fs::read_dir(&entries_directory)
                .map_err(|error| io_error(&entries_directory, &error))?;
            for item in listing {
                let path = item
                    .map_err(|error| io_error(&entries_directory, &error))?
                    .path();
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                if !path.is_file() || path.extension().is_none_or(|extension| extension != "toml") {
                    return Err(MapError::UnexpectedFile { path });
                }
                files.push((format!("entries/{file_name}"), read_text(&path)?));
            }
        }
        for folder in ["types", "enums"] {
            let folder_path = directory.join(folder);
            if folder_path.exists() {
                let listing = std::fs::read_dir(&folder_path)
                    .map_err(|error| io_error(&folder_path, &error))?;
                for item in listing {
                    let path = item.map_err(|error| io_error(&folder_path, &error))?.path();
                    let file_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_owned();
                    if !path.is_file()
                        || path.extension().is_none_or(|extension| extension != "toml")
                    {
                        return Err(MapError::UnexpectedFile { path });
                    }
                    files.push((format!("{folder}/{file_name}"), read_text(&path)?));
                }
            }
        }
        for file in ["domains.toml", "roots.toml"] {
            let path = directory.join(file);
            if path.exists() {
                files.push((file.to_owned(), read_text(&path)?));
            }
        }
        let windows_directory = directory.join("windows");
        if windows_directory.exists() {
            let listing = std::fs::read_dir(&windows_directory)
                .map_err(|error| io_error(&windows_directory, &error))?;
            for item in listing {
                let path = item
                    .map_err(|error| io_error(&windows_directory, &error))?
                    .path();
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if !path.is_file() || path.extension().is_none_or(|extension| extension != "toml") {
                    return Err(MapError::UnexpectedFile { path });
                }
                files.push((format!("windows/{file_name}"), read_text(&path)?));
            }
        }
        let sheets_directory = directory.join("sheets");
        if sheets_directory.exists() {
            collect_sheets(&sheets_directory, "sheets", &mut files)?;
        }
        Self::from_sources(&map_toml, &files)
    }

    /// Builds a map from the text of `map.toml` and of each entries and
    /// schema file, given with its path relative to the map directory
    /// (`entries/<area>.toml`, `sheets/<sheet>.toml`, `types/<area>.toml`,
    /// `enums/<name>.toml`, `domains.toml`, `roots.toml`,
    /// `windows/<window>.toml`).
    ///
    /// # Errors
    ///
    /// Returns a [`MapError`] when the map is not valid.
    pub fn from_sources(map_toml: &str, files: &[(String, String)]) -> Result<Self, MapError> {
        let raw = toml::from_str::<RawMapFile>(map_toml).map_err(|error| MapError::Toml {
            path: PathBuf::from("map.toml"),
            message: error.message().to_owned(),
        })?;
        if raw.format != Some(FORMAT) {
            return Err(MapError::Format {
                found: raw.format,
                supported: FORMAT,
            });
        }

        let mut sheets = BTreeMap::new();
        for (relative, text) in files.iter().filter(|(path, _)| path.starts_with("sheets/")) {
            let schema =
                crate::sheets::parse(relative, text).map_err(|message| MapError::InvalidSheet {
                    path: PathBuf::from(relative),
                    message,
                })?;
            sheets.insert(schema.sheet.clone(), schema);
        }
        let windows = window_schemas(files)?;
        let mut objects = object_model(files)?;
        let mut sorted = files
            .iter()
            .filter(|(path, _)| {
                !path.starts_with("sheets/")
                    && !path.starts_with("windows/")
                    && !is_object_file(path)
            })
            .collect::<Vec<_>>();
        sorted.sort_by(|left, right| left.0.cmp(&right.0));

        let mut entries = BTreeMap::new();
        for (relative, text) in &sorted {
            let path = PathBuf::from(relative);
            let area = relative
                .strip_prefix("entries/")
                .and_then(|name| name.strip_suffix(".toml"))
                .filter(|area| !area.contains('/'))
                .ok_or_else(|| MapError::UnexpectedFile { path: path.clone() })?;
            let file = toml::from_str::<RawFile>(text).map_err(|error| MapError::Toml {
                path: path.clone(),
                message: error.message().to_owned(),
            })?;
            for (table, name, raw_entry) in file.into_entries() {
                if !is_valid_name(&name) {
                    return Err(MapError::InvalidName { path, name });
                }
                if name.split('.').next() != Some(area) {
                    return Err(MapError::WrongArea { path, name });
                }
                if entries.contains_key(&name) {
                    return Err(MapError::Duplicate { name });
                }
                let entry = raw_entry
                    .into_entry(table, name.clone())
                    .map_err(|message| MapError::InvalidEntry {
                        name: name.clone(),
                        message,
                    })?;
                entries.insert(name, entry);
            }
        }

        check_references(&entries)?;
        let order = dependency_order(&entries)?;
        crate::objects::check(&mut objects, &entries).map_err(|message| {
            MapError::InvalidObjects {
                path: PathBuf::from("types"),
                message,
            }
        })?;
        Ok(Self {
            entries,
            sheets,
            objects,
            windows,
            order,
            entries_sha256: content_hash(map_toml, &sorted),
        })
    }

    /// Every entry, by name.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    /// The object model: domains, enums, types, and roots.
    #[must_use]
    pub fn objects(&self) -> &crate::objects::Objects {
        &self.objects
    }

    /// Every sheet schema, by sheet name.
    pub fn sheets(&self) -> impl Iterator<Item = &crate::SheetSchema> {
        self.sheets.values()
    }

    /// The schema of a sheet.
    #[must_use]
    pub fn sheet(&self, name: &str) -> Option<&crate::SheetSchema> {
        self.sheets.get(name)
    }

    /// Every window schema, by window name.
    pub fn windows(&self) -> impl Iterator<Item = &crate::WindowSchema> {
        self.windows.values()
    }

    /// The schema of a window.
    #[must_use]
    pub fn window(&self, name: &str) -> Option<&crate::WindowSchema> {
        self.windows.get(name)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The SHA-256 of `map.toml` and the entries files, in lowercase
    /// hexadecimal; see the format for exactly what is hashed.
    #[must_use]
    pub fn entries_sha256(&self) -> &str {
        &self.entries_sha256
    }

    /// Entry names, each after every entry it references.
    pub(crate) fn order(&self) -> &[String] {
        &self.order
    }
}

/// The object model among `files`, before it is checked against the
/// entries.
fn object_model(files: &[(String, String)]) -> Result<crate::objects::Objects, MapError> {
    let mut objects = crate::objects::Objects::default();
    for (relative, text) in files.iter().filter(|(path, _)| is_object_file(path)) {
        let invalid = |message: String| MapError::InvalidObjects {
            path: PathBuf::from(relative),
            message,
        };
        if relative == "domains.toml" {
            objects.domains = crate::objects::parse_domains(text).map_err(invalid)?;
        } else if relative == "roots.toml" {
            objects.roots = crate::objects::parse_roots(text).map_err(invalid)?;
        } else if let Some(name) = relative
            .strip_prefix("enums/")
            .and_then(|name| name.strip_suffix(".toml"))
        {
            let parsed = crate::objects::parse_enum(name, text).map_err(invalid)?;
            objects.enums.insert(parsed.name.clone(), parsed);
        } else if let Some(area) = relative
            .strip_prefix("types/")
            .and_then(|name| name.strip_suffix(".toml"))
        {
            for parsed in crate::objects::parse_types(area, text).map_err(invalid)? {
                if objects.types.contains_key(&parsed.name) {
                    return Err(invalid(format!("type `{}` is defined twice", parsed.name)));
                }
                objects.types.insert(parsed.name.clone(), parsed);
            }
        }
    }
    Ok(objects)
}

/// The window schemas among `files`, by window name.
fn window_schemas(
    files: &[(String, String)],
) -> Result<BTreeMap<String, crate::WindowSchema>, MapError> {
    let mut windows = BTreeMap::new();
    for (relative, text) in files
        .iter()
        .filter(|(path, _)| path.starts_with("windows/"))
    {
        let schema =
            crate::windows::parse(relative, text).map_err(|message| MapError::InvalidWindow {
                path: PathBuf::from(relative),
                message,
            })?;
        windows.insert(schema.window.clone(), schema);
    }
    Ok(windows)
}

/// Adds every `.toml` file under `directory`, by its path relative to the
/// map as `prefix/...`, to `files`.
fn collect_sheets(
    directory: &Path,
    prefix: &str,
    files: &mut Vec<(String, String)>,
) -> Result<(), MapError> {
    let listing = std::fs::read_dir(directory).map_err(|error| io_error(directory, &error))?;
    for item in listing {
        let path = item.map_err(|error| io_error(directory, &error))?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if path.is_dir() {
            collect_sheets(&path, &format!("{prefix}/{name}"), files)?;
        } else if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "toml")
        {
            files.push((format!("{prefix}/{name}"), read_text(&path)?));
        } else {
            return Err(MapError::UnexpectedFile { path });
        }
    }
    Ok(())
}

/// Lowercase dotted words of at least two parts, each starting with a letter
/// and containing letters, digits, and underscores.
fn is_valid_name(name: &str) -> bool {
    let parts = name.split('.').collect::<Vec<_>>();
    parts.len() >= 2
        && parts.iter().all(|part| {
            part.starts_with(|first: char| first.is_ascii_lowercase())
                && part.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
                })
        })
}

fn check_references(entries: &BTreeMap<String, Entry>) -> Result<(), MapError> {
    let bad = |entry: &Entry, reference: &str, problem: &str| MapError::BadReference {
        name: entry.name.clone(),
        reference: reference.to_owned(),
        problem: problem.to_owned(),
    };
    for entry in entries.values() {
        for reference in entry.references() {
            if !entries.contains_key(reference) {
                return Err(bad(entry, reference, "not defined"));
            }
        }
        let locators =
            entry
                .locator
                .iter()
                .chain(entry.checks.iter().filter_map(|check| match check {
                    crate::Check::Locator(locator) => Some(locator),
                    _ => None,
                }));
        for locator in locators {
            match locator {
                Locator::VtableSlot { vtable, slot } => match &entries[vtable].kind {
                    EntryKind::Vtable { slots } if slot < slots => {}
                    EntryKind::Vtable { .. } => {
                        return Err(bad(entry, vtable, "a vtable without that slot"));
                    }
                    _ => return Err(bad(entry, vtable, "not a vtable")),
                },
                Locator::Operand { function, .. } => {
                    if !matches!(entries[function].kind, EntryKind::Function { .. }) {
                        return Err(bad(entry, function, "not a function"));
                    }
                }
                Locator::Signature { .. } => {}
            }
        }
    }
    Ok(())
}

fn dependency_order(entries: &BTreeMap<String, Entry>) -> Result<Vec<String>, MapError> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Visiting,
        Done,
    }

    fn visit<'a>(
        name: &'a str,
        entries: &'a BTreeMap<String, Entry>,
        states: &mut HashMap<&'a str, State>,
        path: &mut Vec<&'a str>,
        order: &mut Vec<String>,
    ) -> Result<(), MapError> {
        match states.get(name) {
            Some(State::Done) => return Ok(()),
            Some(State::Visiting) => {
                let start = path
                    .iter()
                    .position(|&visited| visited == name)
                    .unwrap_or(0);
                let mut names = path[start..]
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect::<Vec<_>>();
                names.push(name.to_owned());
                return Err(MapError::Cycle { names });
            }
            None => {}
        }
        states.insert(name, State::Visiting);
        path.push(name);
        for reference in entries[name].references() {
            visit(reference, entries, states, path, order)?;
        }
        path.pop();
        states.insert(name, State::Done);
        order.push(name.to_owned());
        Ok(())
    }

    let mut states = HashMap::new();
    let mut order = Vec::with_capacity(entries.len());
    for name in entries.keys() {
        visit(name, entries, &mut states, &mut Vec::new(), &mut order)?;
    }
    Ok(order)
}

/// SHA-256 over `map.toml` and each entries file in path order, each as its
/// relative path, a NUL byte, its length as a little-endian `u64`, and its
/// bytes.
fn content_hash(map_toml: &str, files: &[&(String, String)]) -> String {
    let mut hasher = Sha256::new();
    let mut add = |path: &str, text: &str| {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    };
    add("map.toml", map_toml);
    for (path, text) in files {
        add(path, text);
    }
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn read_text(path: &Path) -> Result<String, MapError> {
    let bytes = std::fs::read(path).map_err(|error| io_error(path, &error))?;
    String::from_utf8(bytes).map_err(|_| MapError::Io {
        path: path.to_owned(),
        message: "the file is not UTF-8".to_owned(),
    })
}

fn io_error(path: &Path, error: &std::io::Error) -> MapError {
    MapError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

/// Whether a file of the map belongs to the object model.
fn is_object_file(path: &str) -> bool {
    path == "domains.toml"
        || path == "roots.toml"
        || path.starts_with("types/")
        || path.starts_with("enums/")
}
