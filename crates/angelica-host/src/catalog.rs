//! Verified entries exposed to the host by name, never supplied by a plugin.
#![forbid(unsafe_code)]
use angelica_map::{Entry, Map, Value};
use angelica_pe::{Image, Layout};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    pub entry: Entry,
    pub value: Value,
    pub function_range: Option<std::ops::Range<u32>>,
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
                let function_range = match value {
                    Value::Address(rva) => image.function_at(rva),
                    Value::Offset(_) => None,
                };
                catalog.present.insert(
                    name.into(),
                    ResolvedEntry {
                        entry: entry.clone(),
                        value,
                        function_range,
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
                        let function_range = match value {
                            Value::Address(rva) => image.function_at(*rva),
                            Value::Offset(_) => None,
                        };
                        catalog.present.insert(
                            name.clone(),
                            ResolvedEntry {
                                entry: entry.clone(),
                                value: *value,
                                function_range,
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
            .filter(|(name, _)| name.starts_with("entries/"))
            .map(|(name, bytes)| Ok((name.clone(), text(bytes)?)))
            .collect::<Result<Vec<_>, String>>()?;
        Map::from_sources(&root, &entries).map_err(|e| e.to_string())
    }
}
