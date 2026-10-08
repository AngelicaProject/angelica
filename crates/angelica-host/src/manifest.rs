//! Plugin manifests (`docs/formats/plugin-manifest-v1.md`) and the player's
//! grants (`docs/formats/host-installation-v1.md`).
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The API versions this host runs.
pub const API_VERSIONS: &[&str] = &["1.0"];
/// The largest memory a plugin instance may request.
pub const MAX_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;

/// What a plugin may do beyond its own windows, storage, and jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Replace text of the game's Excel sheets with text tables.
    #[serde(rename = "excel.text")]
    ExcelText,
    /// Add letters to the game's letter case conversion.
    #[serde(rename = "text.case")]
    TextCase,
}

impl Permission {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ExcelText => "excel.text",
            Self::TextCase => "text.case",
        }
    }

    /// What the player is told when approving.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::ExcelText => "Replaces text the game shows from its data sheets",
            Self::TextCase => "Changes how the game capitalizes letters",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: String,
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// Memory of each instance; the host default when absent.
    #[serde(default)]
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest: u32,
    pub plugin: Plugin,
    #[serde(default)]
    permissions: toml::Table,
    #[serde(skip)]
    pub declared: BTreeSet<Permission>,
}

#[derive(Debug, thiserror::Error)]
#[error("angelica.toml: {0}")]
pub struct ManifestError(pub String);

impl Manifest {
    /// # Errors
    /// Names the first field that breaks the format.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let fail = |message: String| Err(ManifestError(message));
        let mut manifest: Self = toml::from_str(text).map_err(|e| ManifestError(e.to_string()))?;
        if manifest.manifest != 1 {
            return fail(format!(
                "manifest format {} is unsupported",
                manifest.manifest
            ));
        }
        let plugin = &manifest.plugin;
        if plugin.id.is_empty()
            || plugin.id.len() > 64
            || plugin.id.starts_with('-')
            || plugin.id.ends_with('-')
            || !plugin
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return fail(
                "plugin.id has 1 to 64 lowercase letters, digits, and inner hyphens".into(),
            );
        }
        for (field, value) in [("name", &plugin.name), ("description", &plugin.description)] {
            if value.trim().is_empty() || value.len() > 1024 {
                return fail(format!("plugin.{field} is empty or longer than 1024 bytes"));
            }
        }
        semver::Version::parse(&plugin.version)
            .map_err(|e| ManifestError(format!("plugin.version: {e}")))?;
        if !API_VERSIONS.contains(&plugin.api.as_str()) {
            return fail(format!(
                "plugin.api {} is not supported; this host runs {}",
                plugin.api,
                API_VERSIONS.join(", ")
            ));
        }
        if plugin
            .memory_bytes
            .is_some_and(|bytes| bytes == 0 || bytes > MAX_MEMORY_BYTES)
        {
            return fail(format!("plugin.memory_bytes is 1 to {MAX_MEMORY_BYTES}"));
        }
        let mut declared = BTreeSet::new();
        for (name, value) in &manifest.permissions {
            let permission: Permission = toml::Value::String(name.clone())
                .try_into()
                .map_err(|_| ManifestError(format!("unknown permission {name}")))?;
            match value {
                toml::Value::Boolean(true) => {
                    declared.insert(permission);
                }
                toml::Value::Boolean(false) => {}
                _ => return fail(format!("permission {name} is true or false")),
            }
        }
        manifest.declared = declared;
        Ok(manifest)
    }

    /// The permissions the manifest declares.
    #[must_use]
    pub fn permissions(&self) -> &BTreeSet<Permission> {
        &self.declared
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.plugin.id
    }
}

/// The player's approval of a plugin: `grants/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    pub format: u32,
    pub plugin_id: String,
    pub permissions: BTreeSet<Permission>,
}

impl Grants {
    #[must_use]
    pub fn for_manifest(manifest: &Manifest) -> Self {
        Self {
            format: 1,
            plugin_id: manifest.id().into(),
            permissions: manifest.permissions().clone(),
        }
    }

    /// Whether these grants cover everything the manifest declares.
    #[must_use]
    pub fn cover(&self, manifest: &Manifest) -> bool {
        self.format == 1
            && self.plugin_id == manifest.id()
            && manifest.permissions().is_subset(&self.permissions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
manifest = 1
[plugin]
id = "harmonia"
name = "Harmonia"
version = "0.2.0"
api = "1.0"
description = "Community translations."
[permissions]
"excel.text" = true
"text.case" = true
"#;

    #[test]
    fn parses_a_valid_manifest() {
        let manifest = Manifest::parse(VALID).unwrap();
        assert_eq!(manifest.id(), "harmonia");
        assert_eq!(
            manifest.permissions().iter().copied().collect::<Vec<_>>(),
            [Permission::ExcelText, Permission::TextCase]
        );
        let grants = Grants::for_manifest(&manifest);
        assert!(grants.cover(&manifest));
        let mut fewer = grants.clone();
        fewer.permissions.remove(&Permission::TextCase);
        assert!(!fewer.cover(&manifest));
    }

    #[test]
    fn refuses_invalid_fields() {
        for (from, to) in [
            ("manifest = 1", "manifest = 2"),
            ("id = \"harmonia\"", "id = \"Harmonia\""),
            ("api = \"1.0\"", "api = \"1.5\""),
            ("version = \"0.2.0\"", "version = \"two\""),
            ("\"text.case\" = true", "\"game.memory\" = true"),
            ("\"text.case\" = true", "\"text.case\" = [\"x\"]"),
            (
                "description = \"Community translations.\"",
                "description = \" \"",
            ),
            ("[permissions]", "unknown = 1\n[permissions]"),
        ] {
            assert!(Manifest::parse(&VALID.replace(from, to)).is_err(), "{to}");
        }
    }
}
