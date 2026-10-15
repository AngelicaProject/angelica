//! Verified entries exposed to the host by name, never supplied by a plugin.
#![forbid(unsafe_code)]
use angelica_map::{Entry, EntryKind, FunctionCode, Map, Value};
use angelica_pe::{Image, Layout};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    pub entry: Entry,
    pub value: Value,
    /// For a function, its code; for a location or a patch, the code of the
    /// function in the exception directory that holds it. `None` when the
    /// code is not known exactly.
    pub code: Option<FunctionCode>,
}

impl ResolvedEntry {
    /// The entry's code in the game loaded at `base`, as a hook checks it.
    ///
    /// # Errors
    ///
    /// Why the code is not known, so nothing is hooked there.
    pub fn hook_code(&self, base: usize) -> Result<angelica_hook::FunctionCode, String> {
        let code = self.code.as_ref().ok_or_else(|| {
            format!(
                "the code of map entry {} is not known exactly, so it is not hooked",
                self.entry.name
            )
        })?;
        let at = |rva: u32| base + rva as usize;
        Ok(angelica_hook::FunctionCode {
            start: at(code.start),
            ranges: code
                .ranges
                .iter()
                .map(|range| at(range.start)..at(range.end))
                .collect(),
            prologue: code.prologue.unwrap_or(0) as usize,
        })
    }
}

/// The code of an entry at `value` in `image`.
fn code_of(entry: &Entry, value: Value, image: &Image<'_>) -> Option<FunctionCode> {
    let Value::Address(rva) = value else {
        return None;
    };
    match entry.kind {
        EntryKind::Function { .. } => angelica_map::function_code(image, rva),
        EntryKind::Location { .. } | EntryKind::Patch { .. } => {
            angelica_map::function_holding(image, rva)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub present: BTreeMap<String, ResolvedEntry>,
    pub absent: BTreeMap<String, String>,
    pub executable_sha256: String,
    pub entries_sha256: String,
    pub verified: bool,
}

impl Catalog {
    /// Selects a matching authenticated build record, otherwise resolves the signed map.
    /// # Errors
    /// Rejects malformed records, ambiguous matching records or invalid image data.
    pub fn from_signed_files(
        files: &BTreeMap<String, Vec<u8>>,
        mapped: &[u8],
        base: u64,
        hash: String,
    ) -> Result<(Map, Self), String> {
        let map = Self::map_from_files(files)?;
        let mut candidates = Vec::new();
        for (name, bytes) in files.iter().filter(|(name, _)| {
            name.starts_with("builds/")
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension == "toml")
        }) {
            let text = std::str::from_utf8(bytes).map_err(|e| format!("{name}: {e}"))?;
            if angelica_map::BuildRecord::matches(text, &hash, map.entries_sha256())? {
                candidates.push(text);
            }
        }
        if candidates.len() > 1 {
            return Err("multiple signed build records match the executable".into());
        }
        if let Some(text) = candidates.first() {
            let image = Image::parse(mapped, Layout::Mapped).map_err(|e| e.to_string())?;
            let record = angelica_map::BuildRecord::from_toml(text, &map, &image)?;
            let mut catalog = Self {
                executable_sha256: hash,
                entries_sha256: map.entries_sha256().into(),
                verified: true,
                ..Self::default()
            };
            for (name, value) in record.resolution.present() {
                let entry = map.get(name).ok_or("recorded entry is not in map")?;
                catalog.present.insert(
                    name.into(),
                    ResolvedEntry {
                        entry: entry.clone(),
                        value,
                        code: code_of(entry, value, &image),
                    },
                );
            }
            for (name, error) in record.resolution.absent() {
                catalog.absent.insert(name.into(), error.to_string());
            }
            return Ok((map, catalog));
        }
        let catalog = Self::resolve(&map, mapped, base, hash).map_err(|e| e.to_string())?;
        Ok((map, catalog))
    }
    /// Resolves a signed map against a snapshot copied from the Windows module.
    ///
    /// # Errors
    /// Returns malformed-image errors. Missing or ambiguous entries remain absent.
    pub fn resolve(
        map: &Map,
        mapped: &[u8],
        base: u64,
        executable_sha256: String,
    ) -> Result<Self, angelica_pe::ImageError> {
        let image = Image::parse(mapped, Layout::Mapped)?;
        let resolution = map.resolve(&image, base);
        let mut catalog = Self {
            executable_sha256,
            entries_sha256: map.entries_sha256().into(),
            ..Self::default()
        };
        for (name, result) in resolution.results() {
            match result {
                Ok(value) => {
                    if let Some(entry) = map.get(name) {
                        catalog.present.insert(
                            name.clone(),
                            ResolvedEntry {
                                entry: entry.clone(),
                                value: *value,
                                code: code_of(entry, *value, &image),
                            },
                        );
                    }
                }
                Err(error) => {
                    catalog.absent.insert(name.clone(), error.to_string());
                }
            }
        }
        Ok(catalog)
    }

    /// Reconstructs only the files authenticated by the release envelope.
    ///
    /// # Errors
    /// Rejects missing `map.toml`, non-UTF8 data and invalid map files.
    pub fn map_from_files(files: &BTreeMap<String, Vec<u8>>) -> Result<Map, String> {
        let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string());
        let root = text(files.get("map.toml").ok_or("signed map has no map.toml")?)?;
        let entries = files
            .iter()
            .filter(|(name, _)| {
                ["entries/", "sheets/", "types/", "enums/", "windows/"]
                    .iter()
                    .any(|folder| name.starts_with(folder))
                    || angelica_map::ROOT_FILES.contains(&name.as_str())
            })
            .map(|(name, bytes)| Ok((name.clone(), text(bytes)?)))
            .collect::<Result<Vec<_>, String>>()?;
        Map::from_sources(&root, &entries).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_map_keeps_every_file_of_its_object_model() {
        let files: BTreeMap<String, Vec<u8>> = [
            ("map.toml", "format = 2\n"),
            (
                "entries/act.toml",
                r#"
[function."act.go"]
locator = { signature = "C3" }
abi = { args = ["u32"], ret = "void" }
thread = "main"
"#,
            ),
            (
                "domains.toml",
                r#"
[domains.moves]
doc = "Moves."
class = "acts"
read = { en = "Sees", ru = "Видит" }
act = { en = "Moves you", ru = "Двигает вас" }
"#,
            ),
            (
                "actions.toml",
                r#"
[actions.go]
doc = "Goes."
call = "act.go"
domain = "moves"

[actions.go.args.where]
arg = 0
doc = "Where."
"#,
            ),
        ]
        .into_iter()
        .map(|(name, text)| (name.to_owned(), text.as_bytes().to_vec()))
        .collect();
        let map = Catalog::map_from_files(&files).unwrap();
        assert!(map.objects().actions.contains_key("go"));
        assert!(map.objects().domains.contains_key("moves"));
    }

    #[test]
    fn a_constant_offset_is_used_only_in_a_build_its_record_confirms() {
        use angelica_pe::testing::{CODE, PeBuilder};
        let mut files: BTreeMap<String, Vec<u8>> = [
            (
                "map.toml",
                "format = 2
",
            ),
            (
                "entries/game.toml",
                "[field.\"game.thing.name\"]
of = \"game.thing\"
type = \"ptr\"
offset = 0x8
",
            ),
        ]
        .into_iter()
        .map(|(name, text)| (name.to_owned(), text.as_bytes().to_vec()))
        .collect();
        let builder = PeBuilder::new().section(".text", CODE, &[0xC3]);
        let mapped = builder.build_mapped();
        let base = 0x1_4000_0000;
        let hash = "ab".repeat(32);

        let (map, catalog) =
            Catalog::from_signed_files(&files, &mapped, base, hash.clone()).unwrap();
        assert!(!catalog.verified);
        assert!(
            catalog.absent.contains_key("game.thing.name"),
            "a build without a record does not use a constant offset"
        );

        let image = Image::parse(&mapped, Layout::Mapped).unwrap();
        let record = angelica_map::BuildRecord {
            game_version: "2026.09.15.0000.0000".to_owned(),
            exe_sha256: hash.clone(),
            entries_sha256: map.entries_sha256().to_owned(),
            resolution: map.resolve_confirming(
                &image,
                base,
                &["game.thing.name".to_owned()].into(),
            ),
        };
        files.insert(
            "builds/2026.09.15.0000.0000.toml".to_owned(),
            record.to_toml().into_bytes(),
        );
        let (_, catalog) = Catalog::from_signed_files(&files, &mapped, base, hash).unwrap();
        assert!(catalog.verified);
        assert_eq!(catalog.present["game.thing.name"].value, Value::Offset(0x8));
    }
}
