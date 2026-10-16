//! What the registry's CI asks of the `angelica` tool
//! (`docs/architecture/catalog.md`, `docs/formats/catalog-v1.md`): a review
//! of an entry's pull request, which says whether it merges by itself, and
//! the signed index of the catalog.
use runtime::manifest::{Class, Manifest, domain_class};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The domains of the released game map, by name.
pub type Domains = BTreeMap<String, angelica_map::Domain>;

/// `plugins/<id>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub format: u32,
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub author_key: String,
}

impl Entry {
    /// Reads an entry and checks its form; `file_id` is the file's name
    /// without `.toml`.
    ///
    /// # Errors
    /// Malformed TOML, another format, or a field of the wrong form.
    pub fn parse(text: &str, file_id: &str) -> Result<Self, String> {
        let entry: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        if entry.format != 1 {
            return Err(format!("format {} is unsupported", entry.format));
        }
        if entry.id != file_id {
            return Err(format!(
                "the id {} is not the file's name {file_id}",
                entry.id
            ));
        }
        let mut parts = entry.repository.split('/');
        let plain = |part: Option<&str>| {
            part.is_some_and(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            })
        };
        if !(plain(parts.next()) && plain(parts.next()) && parts.next().is_none()) {
            return Err(format!("{} is not owner/name", entry.repository));
        }
        if !entry.tag.starts_with('v') || entry.tag[1..].parse::<semver::Version>().is_err() {
            return Err(format!("{} is not v<version>", entry.tag));
        }
        if entry.author_key.len() != 64
            || !entry
                .author_key
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err("author_key is not 64 lowercase hexadecimal digits".into());
        }
        Ok(entry)
    }

    /// The version the tag names.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.tag[1..]
    }
}

/// What a plugin declares that a player approves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub permissions: BTreeSet<String>,
    /// The game map domains it reads, acts in, and changes through Lens, as
    /// `<domain>.read`, `<domain>.act`, and `<domain>.change`.
    pub lens: BTreeSet<String>,
    pub hosts: BTreeSet<String>,
    pub memory: Option<u64>,
}

impl Declared {
    #[must_use]
    pub fn of(manifest: &Manifest) -> Self {
        Self {
            permissions: manifest
                .permissions()
                .iter()
                .map(|p| p.name().to_owned())
                .collect(),
            lens: [
                ("read", manifest.lens_read()),
                ("act", manifest.lens_act()),
                ("change", manifest.lens_change()),
            ]
            .into_iter()
            .flat_map(|(verb, domains)| {
                domains
                    .into_iter()
                    .map(move |domain| format!("{domain}.{verb}"))
            })
            .collect(),
            hosts: manifest.http_hosts(),
            memory: manifest.plugin.memory_bytes,
        }
    }
}

/// The review of an entry's pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    pub id: String,
    pub version: String,
    /// It merges by itself.
    pub automatic: bool,
    /// Why a maintainer reviews it; empty when automatic.
    pub reasons: Vec<String>,
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
    pub permissions_added: Vec<String>,
    /// Game map domains it reads, acts in, or changes that the catalog
    /// version does not.
    pub lens_added: Vec<String>,
    pub hosts_added: Vec<String>,
}

/// Reads a plugin file: its author key and manifest.
///
/// # Errors
/// A file whose signature or manifest does not verify.
pub fn open(file: &[u8]) -> Result<([u8; 32], Manifest), String> {
    let file = runtime::author::open(file)?;
    let manifest = std::str::from_utf8(&file.manifest).map_err(|e| e.to_string())?;
    Ok((
        file.author,
        Manifest::parse(manifest).map_err(|e| e.to_string())?,
    ))
}

/// Reads the plugin file of a version published before: its author key and
/// manifest, without the permissions this Angelica removed.
///
/// # Errors
/// A file whose signature or manifest does not verify.
pub fn open_earlier(file: &[u8]) -> Result<([u8; 32], Manifest), String> {
    let file = runtime::author::open(file)?;
    let manifest = std::str::from_utf8(&file.manifest).map_err(|e| e.to_string())?;
    Ok((
        file.author,
        Manifest::parse_earlier(manifest).map_err(|e| e.to_string())?,
    ))
}

/// Reviews `entry` against the catalog's `previous` entry and the manifest
/// of its published version, with the author's plugin file from the release
/// of the entry's tag. A new plugin merges by itself only with the released
/// map's `domains` ([`new_plugin`]).
///
/// # Errors
/// An author file that is not the entry's: another key, id, or version.
pub fn review(
    entry: &Entry,
    author_file: &[u8],
    previous: Option<(&Entry, &Manifest)>,
    domains: Option<&Domains>,
) -> Result<Review, String> {
    let (author, manifest) = open(author_file)?;
    if runtime::package::hex(&author) != entry.author_key {
        return Err("the release's plugin file is not signed with the entry's author_key".into());
    }
    if manifest.id() != entry.id {
        return Err(format!(
            "the release holds {}, not {}",
            manifest.id(),
            entry.id
        ));
    }
    if manifest.plugin.version != entry.version() {
        return Err(format!(
            "the release holds version {}, not {}",
            manifest.plugin.version,
            entry.version()
        ));
    }
    let declared = Declared::of(&manifest);
    let mut reasons = Vec::new();
    let (mut permissions_added, mut lens_added, mut hosts_added) =
        (Vec::new(), Vec::new(), Vec::new());
    match previous {
        None => reasons.extend(new_plugin(&manifest, domains)),
        Some((before, published)) => {
            if before.repository != entry.repository {
                reasons.push(format!("the repository changes from {}", before.repository));
            }
            if before.author_key != entry.author_key {
                reasons.push("the author key changes".to_owned());
            }
            let newer = semver::Version::parse(entry.version()).ok()
                > semver::Version::parse(before.version()).ok();
            if !newer {
                reasons.push(format!(
                    "{} is not newer than {}",
                    entry.version(),
                    before.version()
                ));
            }
            let was = Declared::of(published);
            permissions_added = declared
                .permissions
                .difference(&was.permissions)
                .cloned()
                .collect();
            lens_added = declared.lens.difference(&was.lens).cloned().collect();
            hosts_added = declared.hosts.difference(&was.hosts).cloned().collect();
            if !permissions_added.is_empty() {
                reasons.push(format!("new permissions: {}", permissions_added.join(", ")));
            }
            if !lens_added.is_empty() {
                reasons.push(format!("new game map domains: {}", lens_added.join(", ")));
            }
            if !hosts_added.is_empty() {
                reasons.push(format!("new hosts: {}", hosts_added.join(", ")));
            }
            if declared.memory.unwrap_or(0) > was.memory.unwrap_or(0) {
                reasons.push("more memory".to_owned());
            }
        }
    }
    Ok(Review {
        id: entry.id.clone(),
        version: entry.version().to_owned(),
        automatic: reasons.is_empty(),
        reasons,
        permissions: declared.permissions.into_iter().collect(),
        hosts: declared.hosts.into_iter().collect(),
        permissions_added,
        lens_added,
        hosts_added,
    })
}

/// Why a new plugin waits for a maintainer: it asks for something in a class
/// above **Changes what you see** (acting for the player, the internet, or
/// other players), or names a domain the released map does not describe as
/// it asks for it. Without the map, every new plugin waits.
fn new_plugin(manifest: &Manifest, domains: Option<&Domains>) -> Vec<String> {
    let Some(domains) = domains else {
        return vec!["a new plugin".to_owned()];
    };
    let mut by_class: BTreeMap<Class, Vec<String>> = BTreeMap::new();
    let mut unknown = Vec::new();
    for permission in manifest.permissions() {
        by_class
            .entry(permission.class())
            .or_default()
            .push(permission.name().to_owned());
    }
    for name in manifest.lens_read() {
        match domains
            .get(&name)
            .and_then(|domain| domain_class(domain.class))
        {
            Some(class) => by_class
                .entry(class)
                .or_default()
                .push(format!("{name}.read")),
            None => unknown.push(format!("{name}.read")),
        }
    }
    for name in manifest.lens_act() {
        by_class
            .entry(Class::Acts)
            .or_default()
            .push(format!("{name}.act"));
    }
    for name in manifest.lens_change() {
        if domains
            .get(&name)
            .is_some_and(|domain| domain.change.is_some())
        {
            by_class
                .entry(Class::WhatYouSee)
                .or_default()
                .push(format!("{name}.change"));
        } else {
            unknown.push(format!("{name}.change"));
        }
    }
    let mut reasons: Vec<String> = by_class
        .into_iter()
        .filter(|(class, _)| *class < Class::WhatYouSee)
        .map(|(class, names)| {
            format!(
                "a new plugin in the class {}: {}",
                class.title(),
                names.join(", ")
            )
        })
        .collect();
    if !unknown.is_empty() {
        reasons.push(format!(
            "a new plugin with game map domains the released map does not describe: {}",
            unknown.join(", ")
        ));
    }
    reasons
}

/// `withdrawn.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withdrawn {
    pub format: u32,
    #[serde(default)]
    pub withdrawn: Vec<WithdrawnItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawnItem {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub reason: String,
}

/// A plugin in the index.
#[derive(Debug, Clone, Serialize)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub authors: Vec<String>,
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
    pub link: String,
    pub sha256: String,
    pub source: String,
}

/// The catalog link of a plugin.
#[must_use]
pub fn link(id: &str) -> String {
    format!(
        "https://github.com/AngelicaProject/angelica-registry/releases/download/{id}/{id}.angelica"
    )
}

/// The index entry for `entry`, with the plugin file the catalog built and
/// signed with `release`.
///
/// # Errors
/// A file not signed with the release key, or not the entry's version.
pub fn listed(entry: &Entry, file: &[u8], release: &[u8; 32]) -> Result<Listed, String> {
    use sha2::Digest as _;
    let (author, manifest) = open(file)?;
    if author != *release {
        return Err(format!("{} is not signed with the release key", entry.id));
    }
    if manifest.id() != entry.id || manifest.plugin.version != entry.version() {
        return Err(format!(
            "the built file of {} is not {}",
            entry.id, entry.tag
        ));
    }
    let declared = Declared::of(&manifest);
    Ok(Listed {
        id: entry.id.clone(),
        name: manifest.plugin.name.clone(),
        description: manifest.plugin.description.clone(),
        version: manifest.plugin.version.clone(),
        authors: manifest.plugin.authors.clone(),
        permissions: declared.permissions.into_iter().collect(),
        hosts: declared.hosts.into_iter().collect(),
        link: link(&entry.id),
        sha256: runtime::package::hex(&sha2::Sha256::digest(file)),
        source: format!("https://github.com/{}/tree/{}", entry.repository, entry.tag),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn manifest(version: &str, permissions: &str) -> String {
        format!(
            "manifest = 1\n[plugin]\nid = \"timers\"\nname = \"Timers\"\nversion = \"{version}\"\napi = \"1.0\"\ndescription = \"Resets.\"\n[permissions]\n{permissions}"
        )
    }

    fn entry(tag: &str, key: &SigningKey) -> Entry {
        Entry {
            format: 1,
            id: "timers".into(),
            repository: "author/timers".into(),
            tag: tag.into(),
            author_key: runtime::package::hex(key.verifying_key().as_bytes()),
        }
    }

    #[test]
    fn entries_have_one_form() {
        let good = "format = 1\nid = \"timers\"\nrepository = \"author/timers\"\ntag = \"v1.2.0\"\nauthor_key = \"6fb36909f2f2778f0b5fc8dc2d2eddb6fa096146699b4fd3efa360a7fbe16b62\"\n";
        assert_eq!(Entry::parse(good, "timers").unwrap().version(), "1.2.0");
        assert!(Entry::parse(good, "other").is_err());
        for (from, to) in [
            ("author/timers", "author/timers/x"),
            ("author/timers", "https://github.com/a"),
            ("v1.2.0", "1.2.0"),
            ("6fb3", "6FB3"),
            ("format = 1", "format = 2"),
        ] {
            assert!(
                Entry::parse(&good.replacen(from, to, 1), "timers").is_err(),
                "{to}"
            );
        }
        assert!(Entry::parse(&format!("{good}extra = 1\n"), "timers").is_err());
    }

    #[test]
    fn an_update_that_declares_no_more_merges_by_itself_and_others_wait() {
        let key = SigningKey::from_bytes(&[3; 32]);
        let before = entry("v1.0.0", &key);
        let published = Manifest::parse(&manifest("1.0.0", "\"chat.print\" = true\n")).unwrap();

        let file = runtime::author::sign(
            &key,
            manifest("1.1.0", "\"chat.print\" = true\n").as_bytes(),
            b"\0asm",
        )
        .unwrap();
        let update = review(
            &entry("v1.1.0", &key),
            &file,
            Some((&before, &published)),
            None,
        )
        .unwrap();
        assert!(update.automatic, "{:?}", update.reasons);

        let file = runtime::author::sign(
            &key,
            manifest("1.2.0", "\"chat.print\" = true\n\"sound.play\" = true\n").as_bytes(),
            b"\0asm",
        )
        .unwrap();
        let more = review(
            &entry("v1.2.0", &key),
            &file,
            Some((&before, &published)),
            None,
        )
        .unwrap();
        assert!(!more.automatic);
        assert_eq!(more.permissions_added, vec!["sound.play"]);

        // Changing a domain of the game map is as new as a permission.
        let lens_file = runtime::author::sign(
            &key,
            format!(
                "{}\n[lens]\nchange = [\"text\"]\n",
                manifest("1.3.0", "\"chat.print\" = true\n")
            )
            .as_bytes(),
            b"\0asm",
        )
        .unwrap();
        let lens = review(
            &entry("v1.3.0", &key),
            &lens_file,
            Some((&before, &published)),
            None,
        )
        .unwrap();
        assert!(!lens.automatic);
        assert_eq!(lens.lens_added, vec!["text.change"]);
        assert_eq!(lens.reasons, vec!["new game map domains: text.change"]);

        let new = review(&entry("v1.2.0", &key), &file, None, None).unwrap();
        assert_eq!(new.reasons, vec!["a new plugin"]);

        let stranger = SigningKey::from_bytes(&[4; 32]);
        let forged =
            runtime::author::sign(&stranger, manifest("1.1.0", "").as_bytes(), b"\0asm").unwrap();
        assert!(
            review(
                &entry("v1.1.0", &key),
                &forged,
                Some((&before, &published)),
                None
            )
            .is_err()
        );
        assert!(
            review(
                &entry("v1.3.0", &key),
                &file,
                Some((&before, &published)),
                None
            )
            .is_err(),
            "the release holds another version"
        );
    }

    fn domain(name: &str, class: angelica_map::DomainClass, change: bool) -> angelica_map::Domain {
        let words = angelica_map::Wording {
            en: String::new(),
            ru: String::new(),
        };
        angelica_map::Domain {
            name: name.into(),
            doc: String::new(),
            class,
            read: words.clone(),
            change: change.then(|| words.clone()),
            act: None,
        }
    }

    #[test]
    fn a_new_plugin_merges_by_itself_only_below_the_serious_classes() {
        use angelica_map::DomainClass;
        let key = SigningKey::from_bytes(&[3; 32]);
        let domains: Domains = [
            domain("inventory", DomainClass::You, false),
            domain("text", DomainClass::Display, true),
            domain("chat", DomainClass::Others, false),
        ]
        .into_iter()
        .map(|domain| (domain.name.clone(), domain))
        .collect();
        let new = |extra: &str| {
            let file = runtime::author::sign(
                &key,
                format!(
                    "{}{extra}",
                    manifest("1.0.0", "\"chat.print\" = true\n\"draw.world\" = true\n")
                )
                .as_bytes(),
                b"\0asm",
            )
            .unwrap();
            review(&entry("v1.0.0", &key), &file, None, Some(&domains)).unwrap()
        };

        let only_you = new("[lens]\nread = [\"inventory\"]\nchange = [\"text\"]\n");
        assert!(only_you.automatic, "{:?}", only_you.reasons);

        let others = new("[lens]\nread = [\"inventory\", \"chat\"]\n");
        assert_eq!(
            others.reasons,
            vec!["a new plugin in the class Sees other players: chat.read"]
        );
        let acts = new("[lens]\nact = [\"inventory\"]\n");
        assert_eq!(
            acts.reasons,
            vec!["a new plugin in the class Acts in the game for you: inventory.act"]
        );
        let unknown = new("[lens]\nread = [\"retainers\"]\nchange = [\"inventory\"]\n");
        assert_eq!(
            unknown.reasons,
            vec![
                "a new plugin with game map domains the released map does not describe: retainers.read, inventory.change"
            ]
        );

        let internet = runtime::author::sign(
            &key,
            format!(
                "{}[http]\nhosts = [\"example.com\"]\n",
                manifest("1.0.0", "\"http.get\" = true\n")
            )
            .as_bytes(),
            b"\0asm",
        )
        .unwrap();
        let internet = review(&entry("v1.0.0", &key), &internet, None, Some(&domains)).unwrap();
        assert_eq!(
            internet.reasons,
            vec!["a new plugin in the class Uses the internet: http.get"]
        );
    }

    #[test]
    fn the_index_lists_only_what_the_catalog_signed() {
        let release = SigningKey::from_bytes(&[1; 32]);
        let author = SigningKey::from_bytes(&[3; 32]);
        let built = runtime::author::sign(
            &release,
            manifest("1.1.0", "\"chat.print\" = true\n").as_bytes(),
            b"\0asm",
        )
        .unwrap();
        let listed = listed(
            &entry("v1.1.0", &author),
            &built,
            &release.verifying_key().to_bytes(),
        )
        .unwrap();
        assert_eq!(listed.permissions, vec!["chat.print"]);
        assert!(
            listed
                .link
                .ends_with("/releases/download/timers/timers.angelica")
        );
        let authors_own =
            runtime::author::sign(&author, manifest("1.1.0", "").as_bytes(), b"\0asm").unwrap();
        assert!(
            super::listed(
                &entry("v1.1.0", &author),
                &authors_own,
                &release.verifying_key().to_bytes()
            )
            .is_err()
        );
    }
}
