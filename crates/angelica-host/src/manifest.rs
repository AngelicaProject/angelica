//! Plugin manifests (`docs/formats/plugin-manifest-v1.md`) and the player's
//! grants (`docs/formats/host-installation-v1.md`).
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The API versions this host runs.
pub const API_VERSIONS: &[&str] = &["1.0"];
/// The largest memory a plugin instance may request.
pub const MAX_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
/// Hosts a manifest lists for `http.get`.
pub const MAX_HOSTS: usize = 32;

/// What a plugin may do beyond its own windows, storage, and jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Replace text of the game's Excel sheets with text tables.
    #[serde(rename = "excel.text")]
    ExcelText,
    /// Add letters to the game's letter case conversion.
    #[serde(rename = "text.case")]
    TextCase,
    /// Serve replacement game files.
    #[serde(rename = "files.replace")]
    FilesReplace,
    /// Add chat commands.
    #[serde(rename = "chat.commands")]
    ChatCommands,
    /// Print messages in the chat log.
    #[serde(rename = "chat.print")]
    ChatPrint,
    /// Send HTTPS GET requests to the hosts the manifest lists.
    #[serde(rename = "http.get")]
    HttpGet,
    /// Read the state of the player's character.
    #[serde(rename = "state.self")]
    StateSelf,
    /// Receive the messages added to the chat log.
    #[serde(rename = "chat.read")]
    ChatRead,
    /// Keep elements of the game's own windows hidden.
    #[serde(rename = "gameui.hide")]
    GameUiHide,
    /// Put text in text elements of the game's own windows.
    #[serde(rename = "gameui.text")]
    GameUiText,
}

impl Permission {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ExcelText => "excel.text",
            Self::TextCase => "text.case",
            Self::FilesReplace => "files.replace",
            Self::ChatCommands => "chat.commands",
            Self::ChatPrint => "chat.print",
            Self::HttpGet => "http.get",
            Self::StateSelf => "state.self",
            Self::ChatRead => "chat.read",
            Self::GameUiHide => "gameui.hide",
            Self::GameUiText => "gameui.text",
        }
    }

    /// What the player is told when approving.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::ExcelText => "Replaces text the game shows from its data sheets",
            Self::TextCase => "Changes how the game capitalizes letters",
            Self::FilesReplace => "Replaces game files: fonts and textures",
            Self::ChatCommands => "Adds its own chat commands",
            Self::ChatPrint => "Prints its messages in your chat log",
            Self::HttpGet => "Downloads from these internet hosts",
            Self::StateSelf => {
                "Reads your character: name, class, level, HP and MP, position, and zone"
            }
            Self::ChatRead => "Reads the messages in your chat log, including other players'",
            Self::GameUiHide => "Hides parts of the game's interface",
            Self::GameUiText => "Puts its own text in the game's windows",
        }
    }

    /// The class the player sees the permission in.
    #[must_use]
    pub fn class(self) -> Class {
        match self {
            Self::HttpGet => Class::Network,
            Self::ChatRead => Class::OtherPlayers,
            Self::ExcelText
            | Self::TextCase
            | Self::FilesReplace
            | Self::GameUiHide
            | Self::GameUiText => Class::WhatYouSee,
            Self::ChatCommands | Self::ChatPrint | Self::StateSelf => Class::YoursOnly,
        }
    }
}

/// How far a permission reaches, as the approval shows it; the most serious
/// class first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Reaches the internet.
    Network,
    /// Sees what other players say or do.
    OtherPlayers,
    /// Changes what the player sees of the game.
    WhatYouSee,
    /// Stays with the player and only shows things.
    YoursOnly,
}

impl Class {
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Network => "Uses the internet",
            Self::OtherPlayers => "Sees other players",
            Self::WhatYouSee => "Changes what you see",
            Self::YoursOnly => "Only for you",
        }
    }
}

/// A permission the player is asked to approve, with the hosts of
/// `http.get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub permission: Permission,
    pub hosts: Vec<String>,
}

/// What the player is asked to approve, by class, the most serious first:
/// everything the manifest declares, or with an earlier approval of the
/// plugin only what it does not cover.
#[must_use]
pub fn requests(manifest: &Manifest, granted: Option<&Grants>) -> Vec<(Class, Vec<Request>)> {
    let granted = granted.filter(|grants| grants.format == 1 && grants.plugin_id == manifest.id());
    let mut classes: BTreeMap<Class, Vec<Request>> = BTreeMap::new();
    for &permission in manifest.permissions() {
        let hosts: Vec<String> = if permission == Permission::HttpGet {
            manifest
                .http_hosts()
                .into_iter()
                .filter(|host| granted.is_none_or(|grants| !grants.http_hosts.contains(host)))
                .collect()
        } else {
            Vec::new()
        };
        let covered = granted.is_some_and(|grants| grants.permissions.contains(&permission));
        if !covered || !hosts.is_empty() {
            classes
                .entry(permission.class())
                .or_default()
                .push(Request { permission, hosts });
        }
    }
    classes.into_iter().collect()
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

/// The hosts `http.get` reaches.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest: u32,
    pub plugin: Plugin,
    #[serde(default)]
    permissions: toml::Table,
    #[serde(default)]
    http: Option<Http>,
    #[serde(skip)]
    pub declared: BTreeSet<Permission>,
    /// Where the package came from; the loader sets it, the file has none.
    #[serde(skip)]
    pub origin: Origin,
}

/// Where a plugin's package came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Origin {
    /// The official catalog: a package signed with the release key, or a
    /// plugin file the catalog built and signed with it.
    #[default]
    Catalog,
    /// Its author, outside the catalog: a plugin file the player installed,
    /// compiled and signed on the player's computer.
    Author,
    /// In development: signed with the release key like the catalog, by
    /// `angelica dev install`, which adds a `development` file to the
    /// package. The menu marks it so a developer tells it from the rest.
    Development,
}

/// A plugin id: 1 to 64 lowercase letters, digits, and inner hyphens.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
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
        if !valid_id(&plugin.id) {
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
        let hosts = manifest
            .http
            .as_ref()
            .map_or(&[][..], |http| &http.hosts[..]);
        if declared.contains(&Permission::HttpGet) == hosts.is_empty() {
            return fail("http.get and a non-empty http.hosts come together".into());
        }
        if hosts.len() > MAX_HOSTS {
            return fail(format!("http.hosts lists at most {MAX_HOSTS} hosts"));
        }
        let mut unique = BTreeSet::new();
        for host in hosts {
            crate::http::check_host_pattern(host)
                .map_err(|problem| ManifestError(format!("http.hosts: {host:?} {problem}")))?;
            if !unique.insert(host) {
                return fail(format!("http.hosts lists {host} twice"));
            }
        }
        manifest.declared = declared;
        Ok(manifest)
    }

    /// The hosts `http.get` reaches, as the manifest lists them.
    #[must_use]
    pub fn http_hosts(&self) -> BTreeSet<String> {
        self.http
            .iter()
            .flat_map(|http| http.hosts.iter().cloned())
            .collect()
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
    /// The hosts approved for `http.get`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub http_hosts: BTreeSet<String>,
}

impl Grants {
    #[must_use]
    pub fn for_manifest(manifest: &Manifest) -> Self {
        Self {
            format: 1,
            plugin_id: manifest.id().into(),
            permissions: manifest.permissions().clone(),
            http_hosts: manifest.http_hosts(),
        }
    }

    /// Whether these grants cover everything the manifest declares.
    #[must_use]
    pub fn cover(&self, manifest: &Manifest) -> bool {
        self.format == 1
            && self.plugin_id == manifest.id()
            && manifest.permissions().is_subset(&self.permissions)
            && manifest.http_hosts().is_subset(&self.http_hosts)
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
    fn http_hosts_come_with_their_permission_and_need_approval() {
        let with = |hosts: &str| {
            format!(
                "{VALID}\"http.get\" = true
[http]
hosts = [{hosts}]
"
            )
        };
        let manifest = Manifest::parse(&with("\"example.com\", \"*.cdn.example.org\"")).unwrap();
        assert!(manifest.permissions().contains(&Permission::HttpGet));
        assert_eq!(
            manifest.http_hosts().into_iter().collect::<Vec<_>>(),
            ["*.cdn.example.org", "example.com"]
        );
        let grants = Grants::for_manifest(&manifest);
        assert!(grants.cover(&manifest));
        let json = serde_json::to_string(&grants).unwrap();
        assert!(
            json.contains("\"http_hosts\":[\"*.cdn.example.org\",\"example.com\"]"),
            "{json}"
        );

        let more = Manifest::parse(&with("\"example.com\", \"example.net\"")).unwrap();
        assert!(!grants.cover(&more), "a new host needs approval again");
        let without = Grants::for_manifest(&Manifest::parse(VALID).unwrap());
        assert!(
            !serde_json::to_string(&without)
                .unwrap()
                .contains("http_hosts"),
            "grants without hosts keep their format"
        );

        for hosts in [
            "",
            "\"localhost\"",
            "\"127.0.0.1\"",
            "\"Example.com\"",
            "\"example.com\", \"example.com\"",
            "\"*.com\"",
            "\"https://example.com\"",
        ] {
            assert!(Manifest::parse(&with(hosts)).is_err(), "{hosts}");
        }
        assert!(
            Manifest::parse(&format!(
                "{VALID}[http]
hosts = [\"example.com\"]
"
            ))
            .is_err(),
            "hosts without the permission"
        );
    }

    #[test]
    fn approval_asks_by_class_and_only_for_what_is_new() {
        let manifest = Manifest::parse(&format!(
            "{VALID}\"http.get\" = true\n[http]\nhosts = [\"a.example.org\", \"b.example.org\"]\n"
        ))
        .unwrap();
        let all = requests(&manifest, None);
        let classes: Vec<Class> = all.iter().map(|(class, _)| *class).collect();
        assert_eq!(classes, [Class::Network, Class::WhatYouSee]);
        assert_eq!(all[0].1[0].hosts.len(), 2);

        // An earlier approval without one host and without letter case.
        let mut granted = Grants::for_manifest(&manifest);
        granted.permissions.remove(&Permission::TextCase);
        let first = granted.http_hosts.iter().next().unwrap().clone();
        granted.http_hosts.remove(&first);
        let new = requests(&manifest, Some(&granted));
        assert_eq!(
            new,
            [
                (
                    Class::Network,
                    vec![Request {
                        permission: Permission::HttpGet,
                        hosts: vec![first],
                    }]
                ),
                (
                    Class::WhatYouSee,
                    vec![Request {
                        permission: Permission::TextCase,
                        hosts: Vec::new(),
                    }]
                ),
            ]
        );
        assert!(requests(&manifest, Some(&Grants::for_manifest(&manifest))).is_empty());
        // Another plugin's approval counts for nothing.
        granted.plugin_id = "other".into();
        assert_eq!(requests(&manifest, Some(&granted)), all);
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

    #[test]
    fn damaged_manifests_are_refused_without_panicking() {
        let mut mutator = crate::mutations::Mutator::new(13);
        for _ in 0..20_000 {
            if let Ok(manifest) = Manifest::parse(&mutator.mutate_text(VALID)) {
                let _ = Grants::for_manifest(&manifest).cover(&manifest);
                let _ = manifest.http_hosts();
            }
        }
    }
}
