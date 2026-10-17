//! Plugin manifests (`docs/formats/plugin-manifest-v1.md`) and the player's
//! grants (`docs/formats/host-installation-v1.md`).
//!
//! A plugin asks for two kinds of permission: the host's own, such as
//! `http.get`, and reading domains of the game map through Lens, whose
//! wording comes from the map. A domain the map does not describe, or of a
//! class this host does not present, is never granted.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The API versions this host runs.
pub const API_VERSIONS: &[&str] = &["1.0"];
/// The largest memory a plugin instance may request.
pub const MAX_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
/// Hosts a manifest lists for `http.get`.
pub const MAX_HOSTS: usize = 32;
/// Domains a manifest lists in `[lens] read`.
pub const MAX_DOMAINS: usize = 64;

/// What a plugin may do beyond its own windows, storage, and jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
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
    /// Send data to the hosts the manifest lists: POST, PUT, PATCH, DELETE.
    #[serde(rename = "http.send")]
    HttpSend,
    /// Draw marks, lines, and text over the game world.
    #[serde(rename = "draw.world")]
    DrawWorld,
    /// Send messages to the plugins the manifest lists.
    #[serde(rename = "messages.send")]
    MessagesSend,
    /// Put text on the player's clipboard for the player's action.
    #[serde(rename = "clipboard.write")]
    ClipboardWrite,
    /// Show notifications at the top right of the screen.
    #[serde(rename = "notifications")]
    Notifications,
    /// Play sounds from the plugin's storage.
    #[serde(rename = "sound.play")]
    SoundPlay,
    /// Keep a cache of files of the size `[storage] cache` names.
    #[serde(rename = "storage.cache")]
    StorageCache,
    /// Keep WebSocket connections to the hosts the manifest lists.
    #[serde(rename = "http.websocket")]
    HttpWebsocket,
    /// Open pages of the sites `[links] hosts` lists in the player's
    /// browser, for the player's action.
    #[serde(rename = "links.open")]
    LinksOpen,
}

impl Permission {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::FilesReplace => "files.replace",
            Self::ChatCommands => "chat.commands",
            Self::ChatPrint => "chat.print",
            Self::HttpGet => "http.get",
            Self::HttpSend => "http.send",
            Self::DrawWorld => "draw.world",
            Self::MessagesSend => "messages.send",
            Self::ClipboardWrite => "clipboard.write",
            Self::Notifications => "notifications",
            Self::SoundPlay => "sound.play",
            Self::StorageCache => "storage.cache",
            Self::HttpWebsocket => "http.websocket",
            Self::LinksOpen => "links.open",
        }
    }

    /// What the player is told when approving.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::FilesReplace => "Replace game files: fonts, textures, materials, and models",
            Self::ChatCommands => "Add its own chat commands",
            Self::ChatPrint => "Print its messages in your chat log",
            Self::HttpGet => "Download from these sites",
            Self::HttpSend => "Send data to these sites",
            Self::DrawWorld => "Draw marks, lines, and text over the game world",
            Self::MessagesSend => "Send messages to these plugins",
            Self::ClipboardWrite => "Copy text to your clipboard when you ask it to",
            Self::Notifications => "Show notifications and flash the game on the taskbar",
            Self::SoundPlay => "Play sounds",
            Self::StorageCache => "Keep a cache of files on your disk",
            Self::HttpWebsocket => "Keep live connections to these sites",
            Self::LinksOpen => "Open pages of these sites in your browser when you ask it to",
        }
    }

    /// The class the player sees the permission in.
    #[must_use]
    pub fn class(self) -> Class {
        match self {
            Self::HttpGet | Self::HttpSend | Self::HttpWebsocket | Self::LinksOpen => {
                Class::Network
            }
            Self::FilesReplace | Self::DrawWorld => Class::WhatYouSee,
            Self::ChatCommands
            | Self::ChatPrint
            | Self::MessagesSend
            | Self::ClipboardWrite
            | Self::Notifications
            | Self::SoundPlay
            | Self::StorageCache => Class::YoursOnly,
        }
    }
}

/// How far a permission reaches, as the approval shows it; the most serious
/// class first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Acts in the game for the player.
    Acts,
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
            Self::Network => "Use the internet",
            Self::Acts => "Act in the game for you",
            Self::OtherPlayers => "See other players",
            Self::WhatYouSee => "Change what you see",
            Self::YoursOnly => "Other",
        }
    }
}

/// The class the player sees reading a domain in; `None` for a class this
/// host does not present, whose domains it never grants.
#[must_use]
pub fn domain_class(class: angelica_map::DomainClass) -> Option<Class> {
    use angelica_map::DomainClass;
    match class {
        DomainClass::You => Some(Class::YoursOnly),
        DomainClass::Display => Some(Class::WhatYouSee),
        DomainClass::Others => Some(Class::OtherPlayers),
        DomainClass::Acts | DomainClass::ActsOnOthers => None,
    }
}

/// The domains of the game map, by name, as the host knows them now.
pub type Domains<'a> = &'a dyn Fn(&str) -> Option<angelica_map::Domain>;

/// Something a plugin holds or asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// A permission of the host.
    Permission(Permission),
    /// Reading a domain of the game map through Lens.
    Read(angelica_map::Domain),
    /// Acting in a domain of the game map through Lens.
    Act(angelica_map::Domain),
    /// Changing values of a domain of the game map through Lens.
    Change(angelica_map::Domain),
}

impl Asked {
    #[must_use]
    pub fn class(&self) -> Class {
        match self {
            Self::Permission(permission) => permission.class(),
            Self::Read(domain) => domain_class(domain.class).unwrap_or(Class::YoursOnly),
            Self::Act(domain) if domain.act_local => Class::WhatYouSee,
            Self::Act(_) => Class::Acts,
            Self::Change(_) => Class::WhatYouSee,
        }
    }

    /// How the menu's details name it: `http.get`, `inventory.read`.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Permission(permission) => permission.name().to_owned(),
            Self::Read(domain) => format!("{}.read", domain.name),
            Self::Act(domain) => format!("{}.act", domain.name),
            Self::Change(domain) => format!("{}.change", domain.name),
        }
    }
}

/// Something the player is asked to approve, with the hosts of `http.get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub asked: Asked,
    pub hosts: Vec<String>,
}

/// The domains of `[lens] read` that can be granted: those the map
/// describes, in a class this host presents.
fn grantable(manifest: &Manifest, domains: Domains<'_>) -> Vec<angelica_map::Domain> {
    manifest
        .lens_read()
        .iter()
        .filter_map(|name| domains(name))
        .filter(|domain| domain_class(domain.class).is_some())
        .collect()
}

/// The domains of `[lens] act` that can be granted: those the map
/// describes with words for acting in them.
fn grantable_act(manifest: &Manifest, domains: Domains<'_>) -> Vec<angelica_map::Domain> {
    manifest
        .lens_act()
        .iter()
        .filter_map(|name| domains(name))
        .filter(|domain| domain.act.is_some())
        .collect()
}

/// The domains of `[lens] change` that can be granted: those the map
/// describes with words for changing them.
fn grantable_change(manifest: &Manifest, domains: Domains<'_>) -> Vec<angelica_map::Domain> {
    manifest
        .lens_change()
        .iter()
        .filter_map(|name| domains(name))
        .filter(|domain| domain.change.is_some())
        .collect()
}

/// The domains of `[lens] read`, `act`, and `change` that are not granted now:
/// the map does not describe them, this host does not present their class,
/// or the map has no words for acting in them or changing them. Acting is
/// named `<domain>.act`, changing `<domain>.change`.
#[must_use]
pub fn ungranted(manifest: &Manifest, domains: Domains<'_>) -> Vec<String> {
    let grantable: BTreeSet<String> = grantable(manifest, domains)
        .into_iter()
        .map(|domain| domain.name)
        .collect();
    let acting: BTreeSet<String> = grantable_act(manifest, domains)
        .into_iter()
        .map(|domain| domain.name)
        .collect();
    let changing: BTreeSet<String> = grantable_change(manifest, domains)
        .into_iter()
        .map(|domain| domain.name)
        .collect();
    manifest
        .lens_read()
        .into_iter()
        .filter(|name| !grantable.contains(name))
        .chain(
            manifest
                .lens_act()
                .into_iter()
                .filter(|name| !acting.contains(name))
                .map(|name| format!("{name}.act")),
        )
        .chain(
            manifest
                .lens_change()
                .into_iter()
                .filter(|name| !changing.contains(name))
                .map(|name| format!("{name}.change")),
        )
        .collect()
}

/// Everything the manifest asks for that can be granted.
#[must_use]
pub fn asked(manifest: &Manifest, domains: Domains<'_>) -> Vec<Asked> {
    manifest
        .permissions()
        .iter()
        .map(|permission| Asked::Permission(*permission))
        .chain(grantable(manifest, domains).into_iter().map(Asked::Read))
        .chain(grantable_act(manifest, domains).into_iter().map(Asked::Act))
        .chain(
            grantable_change(manifest, domains)
                .into_iter()
                .map(Asked::Change),
        )
        .collect()
}

/// What the player is asked to approve, by class, the most serious first:
/// everything the manifest declares that can be granted, or with an earlier
/// approval of the plugin only what it does not cover.
#[must_use]
pub fn requests(
    manifest: &Manifest,
    granted: Option<&Grants>,
    domains: Domains<'_>,
) -> Vec<(Class, Vec<Request>)> {
    let granted = granted.filter(|grants| grants.format == 1 && grants.plugin_id == manifest.id());
    let mut classes: BTreeMap<Class, Vec<Request>> = BTreeMap::new();
    for asked in asked(manifest, domains) {
        let (covered, hosts) = match &asked {
            Asked::Permission(permission) => {
                let hosts: Vec<String> = if *permission == Permission::HttpGet {
                    // The hosts are listed once, with http.get.
                    manifest
                        .http_hosts()
                        .into_iter()
                        .filter(|host| {
                            granted.is_none_or(|grants| !grants.http_hosts.contains(host))
                        })
                        .collect()
                } else if *permission == Permission::LinksOpen {
                    manifest
                        .link_hosts()
                        .into_iter()
                        .filter(|host| {
                            granted.is_none_or(|grants| !grants.link_hosts.contains(host))
                        })
                        .collect()
                } else if *permission == Permission::MessagesSend {
                    manifest
                        .message_to()
                        .into_iter()
                        .filter(|to| granted.is_none_or(|grants| !grants.message_to.contains(to)))
                        .collect()
                } else {
                    Vec::new()
                };
                let size_covered = *permission != Permission::StorageCache
                    || manifest.cache_gib().is_none_or(|gib| {
                        granted.is_some_and(|grants| {
                            grants.cache_gib.is_some_and(|granted| gib <= granted)
                        })
                    });
                (
                    size_covered
                        && granted.is_some_and(|grants| grants.permissions.contains(permission)),
                    hosts,
                )
            }
            Asked::Read(domain) => (
                granted.is_some_and(|grants| grants.lens_read.contains(&domain.name)),
                Vec::new(),
            ),
            Asked::Act(domain) => (
                granted.is_some_and(|grants| grants.lens_act.contains(&domain.name)),
                Vec::new(),
            ),
            Asked::Change(domain) => (
                granted.is_some_and(|grants| grants.lens_change.contains(&domain.name)),
                Vec::new(),
            ),
        };
        if !covered || !hosts.is_empty() {
            classes
                .entry(asked.class())
                .or_default()
                .push(Request { asked, hosts });
        }
    }
    classes.into_iter().collect()
}

/// The domains a plugin reads and acts in through Lens.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lens {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub act: Vec<String>,
    #[serde(default)]
    pub change: Vec<String>,
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

/// The hosts `http.get` reaches, or whose pages `links.open` opens.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    pub hosts: Vec<String>,
}

/// The most gibibytes a plugin's cache holds.
pub const MOST_CACHE_GIB: u32 = 1024;

/// The size of the plugin's cache.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSection {
    /// In gibibytes.
    pub cache: u32,
}

/// The plugins `messages.send` reaches.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagesTo {
    pub to: Vec<String>,
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
    #[serde(default)]
    links: Option<Http>,
    #[serde(default)]
    messages: Option<MessagesTo>,
    #[serde(default)]
    storage: Option<StorageSection>,
    #[serde(default)]
    lens: Option<Lens>,
    #[serde(skip)]
    pub declared: BTreeSet<Permission>,
    /// Where the package came from; the loader sets it, the file has none.
    #[serde(skip)]
    pub origin: Origin,
    /// The plugin's icon, a square PNG, from its package; the loader sets
    /// it.
    #[serde(skip)]
    pub icon: Option<std::sync::Arc<[u8]>>,
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

/// The id no plugin has: Angelica's own name in its layout and its chat
/// command `/angelica`.
pub const RESERVED_ID: &str = "angelica";

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
        Self::parse_as(text, false)
    }

    /// Parses the manifest of a version published or installed before:
    /// a permission this Angelica no longer has, such as one an update
    /// replaced with a domain of the game map, is left out instead of
    /// refusing the manifest, so its version and what it still declares
    /// can be compared with a newer one.
    ///
    /// # Errors
    /// As for [`Manifest::parse`], except for removed permissions.
    pub fn parse_earlier(text: &str) -> Result<Self, ManifestError> {
        Self::parse_as(text, true)
    }

    fn parse_as(text: &str, earlier: bool) -> Result<Self, ManifestError> {
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
        if plugin.id == RESERVED_ID {
            return fail(format!("plugin.id {RESERVED_ID} is Angelica's own"));
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
            let Ok(permission) = toml::Value::String(name.clone()).try_into::<Permission>() else {
                if earlier {
                    continue;
                }
                return fail(format!("unknown permission {name}"));
            };
            match value {
                toml::Value::Boolean(true) => {
                    declared.insert(permission);
                }
                toml::Value::Boolean(false) => {}
                _ => return fail(format!("permission {name} is true or false")),
            }
        }
        manifest.check_sections(&declared)?;
        if let Some(lens) = &manifest.lens {
            check_domains("lens.read", &lens.read)?;
            check_domains("lens.act", &lens.act)?;
            check_domains("lens.change", &lens.change)?;
        }
        manifest.declared = declared;
        Ok(manifest)
    }

    /// Checks the tables that come with permissions: `[http]`, `[links]`,
    /// `[messages]`, and `[storage]`.
    fn check_sections(&self, declared: &BTreeSet<Permission>) -> Result<(), ManifestError> {
        let fail = |message: String| Err(ManifestError(message));
        let sites = self
            .links
            .as_ref()
            .map_or(&[][..], |links| &links.hosts[..]);
        if declared.contains(&Permission::LinksOpen) == sites.is_empty() {
            return fail("links.open and a non-empty links.hosts come together".into());
        }
        check_hosts("links.hosts", sites)?;
        let hosts = self.http.as_ref().map_or(&[][..], |http| &http.hosts[..]);
        if declared.contains(&Permission::HttpGet) == hosts.is_empty() {
            return fail("http.get and a non-empty http.hosts come together".into());
        }
        if declared.contains(&Permission::HttpSend) && !declared.contains(&Permission::HttpGet) {
            return fail("http.send comes with http.get and its http.hosts".into());
        }
        if declared.contains(&Permission::HttpWebsocket) && !declared.contains(&Permission::HttpGet)
        {
            return fail("http.websocket comes with http.get and its http.hosts".into());
        }
        let receivers = self
            .messages
            .as_ref()
            .map_or(&[][..], |messages| &messages.to[..]);
        if declared.contains(&Permission::MessagesSend) == receivers.is_empty() {
            return fail("messages.send and a non-empty messages.to come together".into());
        }
        if receivers.len() > MAX_HOSTS {
            return fail(format!("messages.to lists at most {MAX_HOSTS} plugins"));
        }
        let mut listed = BTreeSet::new();
        for receiver in receivers {
            if !valid_id(receiver) {
                return fail(format!("messages.to: {receiver:?} is not a plugin id"));
            }
            if !listed.insert(receiver) {
                return fail(format!("messages.to lists {receiver} twice"));
            }
        }
        let cache = self.storage.as_ref().map(|storage| storage.cache);
        if declared.contains(&Permission::StorageCache) != cache.is_some() {
            return fail("storage.cache and storage.cache's size come together".into());
        }
        if cache.is_some_and(|gib| !(1..=MOST_CACHE_GIB).contains(&gib)) {
            return fail(format!("storage.cache is 1 to {MOST_CACHE_GIB} GiB"));
        }
        check_hosts("http.hosts", hosts)
    }

    /// The hosts `http.get` reaches, as the manifest lists them.
    #[must_use]
    pub fn http_hosts(&self) -> BTreeSet<String> {
        self.http
            .iter()
            .flat_map(|http| http.hosts.iter().cloned())
            .collect()
    }

    /// The sites whose pages `links.open` opens, as the manifest lists them.
    #[must_use]
    pub fn link_hosts(&self) -> BTreeSet<String> {
        self.links
            .iter()
            .flat_map(|links| links.hosts.iter().cloned())
            .collect()
    }

    /// The size of the cache `storage.cache` keeps, in gibibytes.
    #[must_use]
    pub fn cache_gib(&self) -> Option<u32> {
        self.storage.as_ref().map(|storage| storage.cache)
    }

    /// The plugins `messages.send` reaches, as the manifest lists them.
    #[must_use]
    pub fn message_to(&self) -> BTreeSet<String> {
        self.messages
            .iter()
            .flat_map(|messages| messages.to.iter().cloned())
            .collect()
    }

    /// The permissions the manifest declares.
    #[must_use]
    pub fn permissions(&self) -> &BTreeSet<Permission> {
        &self.declared
    }

    /// The domains the manifest's `[lens] read` lists.
    #[must_use]
    pub fn lens_read(&self) -> BTreeSet<String> {
        self.lens
            .iter()
            .flat_map(|lens| lens.read.iter().cloned())
            .collect()
    }

    /// The domains the manifest's `[lens] act` lists.
    #[must_use]
    pub fn lens_act(&self) -> BTreeSet<String> {
        self.lens
            .iter()
            .flat_map(|lens| lens.act.iter().cloned())
            .collect()
    }

    /// The domains the manifest's `[lens] change` lists.
    #[must_use]
    pub fn lens_change(&self) -> BTreeSet<String> {
        self.lens
            .iter()
            .flat_map(|lens| lens.change.iter().cloned())
            .collect()
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.plugin.id
    }
}

/// Checks a list of host names, called `what`: at most [`MAX_HOSTS`]
/// distinct names a manifest may list.
fn check_hosts(what: &str, hosts: &[String]) -> Result<(), ManifestError> {
    if hosts.len() > MAX_HOSTS {
        return Err(ManifestError(format!(
            "{what} lists at most {MAX_HOSTS} hosts"
        )));
    }
    let mut unique = BTreeSet::new();
    for host in hosts {
        crate::http::check_host_pattern(host)
            .map_err(|problem| ManifestError(format!("{what}: {host:?} {problem}")))?;
        if !unique.insert(host) {
            return Err(ManifestError(format!("{what} lists {host} twice")));
        }
    }
    Ok(())
}

/// Checks a list of domain names of `[lens]`, called `what`.
fn check_domains(what: &str, listed: &[String]) -> Result<(), ManifestError> {
    if listed.len() > MAX_DOMAINS {
        return Err(ManifestError(format!(
            "{what} lists at most {MAX_DOMAINS} domains"
        )));
    }
    let mut domains = BTreeSet::new();
    for domain in listed {
        let word = domain.len() <= 64
            && domain.starts_with(|c: char| c.is_ascii_lowercase())
            && domain
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !word {
            return Err(ManifestError(format!(
                "{what}: {domain:?} is not a domain name: lowercase letters, digits, and underscores"
            )));
        }
        if !domains.insert(domain) {
            return Err(ManifestError(format!("{what} lists {domain} twice")));
        }
    }
    Ok(())
}

/// The player's approval of a plugin: `grants/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    pub format: u32,
    pub plugin_id: String,
    /// A permission this Angelica no longer has, such as one an update
    /// replaced with a domain of the game map, is left out: the plugin asks
    /// for what its manifest needs now.
    #[serde(deserialize_with = "known_permissions")]
    pub permissions: BTreeSet<Permission>,
    /// The hosts approved for `http.get`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub http_hosts: BTreeSet<String>,
    /// The sites approved for `links.open`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub link_hosts: BTreeSet<String>,
    /// The plugins approved for `messages.send`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub message_to: BTreeSet<String>,
    /// The size approved for `storage.cache`, in gibibytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_gib: Option<u32>,
    /// The domains of the game map approved for reading through Lens.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub lens_read: BTreeSet<String>,
    /// The domains of the game map approved for acting in through Lens.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub lens_act: BTreeSet<String>,
    /// The domains of the game map approved for changing through Lens.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub lens_change: BTreeSet<String>,
}

/// The permissions of a grant this Angelica knows, by name.
fn known_permissions<'de, D>(deserializer: D) -> Result<BTreeSet<Permission>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let names = Vec::<String>::deserialize(deserializer)?;
    Ok(names
        .into_iter()
        .filter_map(|name| serde_json::from_value(serde_json::Value::String(name)).ok())
        .collect())
}

impl Grants {
    /// Everything the manifest asks for that can be granted.
    #[must_use]
    pub fn for_manifest(manifest: &Manifest, domains: Domains<'_>) -> Self {
        Self {
            format: 1,
            plugin_id: manifest.id().into(),
            permissions: manifest.permissions().clone(),
            http_hosts: manifest.http_hosts(),
            link_hosts: manifest.link_hosts(),
            message_to: manifest.message_to(),
            cache_gib: manifest.cache_gib(),
            lens_read: grantable(manifest, domains)
                .into_iter()
                .map(|domain| domain.name)
                .collect(),
            lens_act: grantable_act(manifest, domains)
                .into_iter()
                .map(|domain| domain.name)
                .collect(),
            lens_change: grantable_change(manifest, domains)
                .into_iter()
                .map(|domain| domain.name)
                .collect(),
        }
    }

    /// Whether these grants cover everything the manifest asks for that
    /// can be granted.
    #[must_use]
    pub fn cover(&self, manifest: &Manifest, domains: Domains<'_>) -> bool {
        self.format == 1
            && self.plugin_id == manifest.id()
            && manifest.permissions().is_subset(&self.permissions)
            && manifest.http_hosts().is_subset(&self.http_hosts)
            && manifest.link_hosts().is_subset(&self.link_hosts)
            && manifest.message_to().is_subset(&self.message_to)
            && manifest
                .cache_gib()
                .is_none_or(|gib| self.cache_gib.is_some_and(|granted| gib <= granted))
            && grantable(manifest, domains)
                .iter()
                .all(|domain| self.lens_read.contains(&domain.name))
            && grantable_act(manifest, domains)
                .iter()
                .all(|domain| self.lens_act.contains(&domain.name))
            && grantable_change(manifest, domains)
                .iter()
                .all(|domain| self.lens_change.contains(&domain.name))
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
"files.replace" = true
"draw.world" = true
"#;

    fn none(_: &str) -> Option<angelica_map::Domain> {
        None
    }

    /// The map describes `inventory`, for the player only, and `party`, of
    /// a class this host does not present.
    fn described(name: &str) -> Option<angelica_map::Domain> {
        let wording = |en: &str| angelica_map::Wording {
            en: en.into(),
            ru: en.into(),
        };
        let class = match name {
            "inventory" => angelica_map::DomainClass::You,
            "party" => angelica_map::DomainClass::ActsOnOthers,
            _ => return None,
        };
        Some(angelica_map::Domain {
            name: name.into(),
            doc: "A domain.".into(),
            class,
            read: wording("Sees the items you carry"),
            change: None,
            act: None,
            act_local: false,
        })
    }

    #[test]
    fn domains_the_map_describes_are_asked_for_and_granted() {
        let manifest = Manifest::parse(&format!(
            "{VALID}[lens]\nread = [\"inventory\", \"party\", \"retainers\"]\n"
        ))
        .unwrap();
        assert_eq!(
            manifest.lens_read().into_iter().collect::<Vec<_>>(),
            ["inventory", "party", "retainers"]
        );
        let all = requests(&manifest, None, &described);
        assert_eq!(all.last().unwrap().0, Class::YoursOnly);
        assert_eq!(
            all.last().unwrap().1,
            [Request {
                asked: Asked::Read(described("inventory").unwrap()),
                hosts: Vec::new(),
            }]
        );
        assert_eq!(ungranted(&manifest, &described), ["party", "retainers"]);
        let grants = Grants::for_manifest(&manifest, &described);
        assert_eq!(
            grants
                .lens_read
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["inventory"]
        );
        assert!(grants.cover(&manifest, &described));
        assert!(requests(&manifest, Some(&grants), &described).is_empty());
        let json = serde_json::to_string(&grants).unwrap();
        assert!(json.contains("\"lens_read\":[\"inventory\"]"), "{json}");
        // An approval from before the map described the domain does not
        // cover it.
        let earlier = Grants::for_manifest(&manifest, &none);
        assert!(!earlier.cover(&manifest, &described));
        assert_eq!(requests(&manifest, Some(&earlier), &described).len(), 1);

        for read in [
            "[\"Inventory\"]",
            "[\"inventory\", \"inventory\"]",
            "[\"inventory.read\"]",
            "[\"\"]",
        ] {
            assert!(
                Manifest::parse(&format!("{VALID}[lens]\nread = {read}\n")).is_err(),
                "{read}"
            );
        }
        assert!(Manifest::parse(&format!("{VALID}[lens]\nwrite = []\n")).is_err());
    }

    #[test]
    fn an_earlier_manifest_is_read_without_the_permissions_angelica_removed() {
        let earlier = VALID.replace(
            "\"draw.world\" = true",
            "\"draw.world\" = true
\"excel.text\" = true",
        );
        assert!(Manifest::parse(&earlier).is_err(), "a new version may not");
        let manifest = Manifest::parse_earlier(&earlier).unwrap();
        assert_eq!(manifest.plugin.version, "0.2.0");
        assert_eq!(
            manifest.permissions().iter().copied().collect::<Vec<_>>(),
            [Permission::FilesReplace, Permission::DrawWorld]
        );
        assert!(
            Manifest::parse_earlier(&VALID.replace("\"draw.world\" = true", "\"draw.world\" = 1"))
                .is_err(),
            "everything else is checked as before"
        );
    }

    #[test]
    fn a_grant_of_a_removed_permission_keeps_the_rest_and_asks_again() {
        let old = r#"{"format": 1, "plugin_id": "harmonia", "permissions": ["excel.text", "files.replace"]}"#;
        let grants: Grants = serde_json::from_str(old).unwrap();
        assert_eq!(
            grants.permissions.iter().copied().collect::<Vec<_>>(),
            [Permission::FilesReplace]
        );
        let manifest = Manifest::parse(VALID).unwrap();
        assert!(
            !grants.cover(&manifest, &none),
            "draw.world was never granted"
        );
    }

    #[test]
    fn parses_a_valid_manifest() {
        let manifest = Manifest::parse(VALID).unwrap();
        assert_eq!(manifest.id(), "harmonia");
        assert_eq!(
            manifest.permissions().iter().copied().collect::<Vec<_>>(),
            [Permission::FilesReplace, Permission::DrawWorld]
        );
        let grants = Grants::for_manifest(&manifest, &none);
        assert!(grants.cover(&manifest, &none));
        let mut fewer = grants.clone();
        fewer.permissions.remove(&Permission::DrawWorld);
        assert!(!fewer.cover(&manifest, &none));
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
        let grants = Grants::for_manifest(&manifest, &none);
        assert!(grants.cover(&manifest, &none));
        let json = serde_json::to_string(&grants).unwrap();
        assert!(
            json.contains("\"http_hosts\":[\"*.cdn.example.org\",\"example.com\"]"),
            "{json}"
        );

        let more = Manifest::parse(&with("\"example.com\", \"example.net\"")).unwrap();
        assert!(
            !grants.cover(&more, &none),
            "a new host needs approval again"
        );
        let without = Grants::for_manifest(&Manifest::parse(VALID).unwrap(), &none);
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
        let all = requests(&manifest, None, &none);
        let classes: Vec<Class> = all.iter().map(|(class, _)| *class).collect();
        assert_eq!(classes, [Class::Network, Class::WhatYouSee]);
        assert_eq!(all[0].1[0].hosts.len(), 2);

        // An earlier approval without one host and without drawing.
        let mut granted = Grants::for_manifest(&manifest, &none);
        granted.permissions.remove(&Permission::DrawWorld);
        let first = granted.http_hosts.iter().next().unwrap().clone();
        granted.http_hosts.remove(&first);
        let new = requests(&manifest, Some(&granted), &none);
        assert_eq!(
            new,
            [
                (
                    Class::Network,
                    vec![Request {
                        asked: Asked::Permission(Permission::HttpGet),
                        hosts: vec![first],
                    }]
                ),
                (
                    Class::WhatYouSee,
                    vec![Request {
                        asked: Asked::Permission(Permission::DrawWorld),
                        hosts: Vec::new(),
                    }]
                ),
            ]
        );
        assert!(
            requests(
                &manifest,
                Some(&Grants::for_manifest(&manifest, &none)),
                &none
            )
            .is_empty()
        );
        // Another plugin's approval counts for nothing.
        granted.plugin_id = "other".into();
        assert_eq!(requests(&manifest, Some(&granted), &none), all);
    }

    #[test]
    fn refuses_invalid_fields() {
        for (from, to) in [
            ("manifest = 1", "manifest = 2"),
            ("id = \"harmonia\"", "id = \"Harmonia\""),
            ("id = \"harmonia\"", "id = \"angelica\""),
            ("api = \"1.0\"", "api = \"1.5\""),
            ("version = \"0.2.0\"", "version = \"two\""),
            ("\"draw.world\" = true", "\"game.memory\" = true"),
            ("\"draw.world\" = true", "\"text.case\" = [\"x\"]"),
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
    fn a_cache_names_its_size_and_a_larger_one_asks_again() {
        let with = |gib: &str| {
            VALID.replace(
                "\"draw.world\" = true
",
                &format!(
                    "\"draw.world\" = true
\"storage.cache\" = true
[storage]
cache = {gib}
"
                ),
            )
        };
        let manifest = Manifest::parse(&with("64")).unwrap();
        assert_eq!(manifest.cache_gib(), Some(64));
        let grants = Grants::for_manifest(&manifest, &none);
        assert_eq!(grants.cache_gib, Some(64));
        assert!(grants.cover(&Manifest::parse(&with("32")).unwrap(), &none));
        let larger = Manifest::parse(&with("128")).unwrap();
        assert!(!grants.cover(&larger, &none));
        let asked = requests(&larger, Some(&grants), &none);
        assert_eq!(asked.len(), 1);
        assert_eq!(
            asked[0].1[0].asked,
            Asked::Permission(Permission::StorageCache)
        );
        for gib in ["0", "1025"] {
            assert!(Manifest::parse(&with(gib)).is_err(), "{gib}");
        }
        assert!(
            Manifest::parse(&VALID.replace(
                "[permissions]",
                "[storage]
cache = 1
[permissions]"
            ))
            .is_err()
        );
        assert!(
            Manifest::parse(&VALID.replace(
                "\"draw.world\" = true
",
                "\"draw.world\" = true
\"storage.cache\" = true
"
            ))
            .is_err()
        );
    }

    #[test]
    fn damaged_manifests_are_refused_without_panicking() {
        let mut mutator = crate::mutations::Mutator::new(13);
        for _ in 0..20_000 {
            if let Ok(manifest) = Manifest::parse(&mutator.mutate_text(VALID)) {
                let _ = Grants::for_manifest(&manifest, &none).cover(&manifest, &none);
                let _ = manifest.http_hosts();
            }
        }
    }
}
