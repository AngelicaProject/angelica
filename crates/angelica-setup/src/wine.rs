//! What the installer changes for a game that runs in Wine, on Linux
//! (`docs/architecture/installer.md`): the installation folder as the game
//! sees it, and the DLL override that makes Wine load the proxy.
use std::path::{Path, PathBuf};

/// The registry key of the override: only for the game's program, so other
/// programs in the prefix keep Wine's own `dinput8`.
const OVERRIDES: &str = r"Software\\Wine\\AppDefaults\\ffxiv_dx11.exe\\DllOverrides";
/// The value name and data: the proxy in the game folder first, Wine's own
/// `dinput8` when there is none.
const NAME: &str = "\"dinput8\"";
const VALUE: &str = "\"native,builtin\"";

/// Where Angelica is installed for `game`: in the folder of the flatpak that
/// holds the game, which only it can see, or in the user's data folder.
#[must_use]
pub fn installation_dir(home: &Path, data: Option<&Path>, game: &Path) -> PathBuf {
    let game = std::fs::canonicalize(game).unwrap_or_else(|_| game.to_path_buf());
    let apps = home.join(".var/app");
    if let Ok(inside) = game.strip_prefix(&apps)
        && let Some(app) = inside.components().next()
    {
        return apps.join(app).join("data/angelica");
    }
    data.map_or_else(|| home.join(".local/share"), Path::to_path_buf)
        .join("angelica")
}

/// `path` as Wine shows it to the game: under drive `Z:`, the root of the
/// file system in every prefix.
#[must_use]
pub fn windows_path(path: &Path) -> String {
    format!("Z:{}", path.display().to_string().replace('/', "\\"))
}

/// Whether `user.reg` makes Wine load the proxy for the game.
#[cfg(test)]
fn has_override(registry: &str) -> bool {
    section(registry).is_some_and(|(start, end)| {
        registry[start..end]
            .lines()
            .any(|line| line.trim() == format!("{NAME}={VALUE}"))
    })
}

/// The byte range of the override key's values in `registry`, after its
/// header line, up to the next key.
fn section(registry: &str) -> Option<(usize, usize)> {
    let header = format!("[{OVERRIDES}]").to_ascii_lowercase();
    let mut offset = 0;
    let mut start = None;
    for line in registry.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(begin) = start {
            if trimmed.starts_with('[') {
                return Some((begin, offset));
            }
        } else if trimmed.to_ascii_lowercase().starts_with(&header) {
            start = Some(offset + line.len());
        }
        offset += line.len();
    }
    start.map(|begin| (begin, registry.len()))
}

/// `registry` with the override set, replacing another `dinput8` value of
/// the game's key.
#[must_use]
pub fn with_override(registry: &str, now: u64) -> String {
    let line = format!("{NAME}={VALUE}\n");
    let Some((start, end)) = section(registry) else {
        let text = registry.trim_end_matches('\n');
        return format!("{text}\n\n[{OVERRIDES}] {now}\n{line}");
    };
    let values: String = registry[start..end]
        .split_inclusive('\n')
        .filter(|value| !is_dinput8(value))
        .collect();
    let mut values = values.trim_end_matches('\n').to_owned();
    if !values.is_empty() {
        values.push('\n');
    }
    values.push_str(&line);
    // A blank line separates the key from the next one.
    if end < registry.len() {
        values.push('\n');
    }
    format!("{}{values}{}", &registry[..start], &registry[end..])
}

/// `registry` without the override; the key goes when nothing else is in it.
#[must_use]
pub fn without_override(registry: &str) -> String {
    let Some((start, end)) = section(registry) else {
        return registry.to_owned();
    };
    let kept: Vec<&str> = registry[start..end]
        .split_inclusive('\n')
        .filter(|value| !is_dinput8(value))
        .collect();
    if kept.iter().all(|value| {
        let value = value.trim();
        value.is_empty() || value.starts_with('#')
    }) {
        let header = registry[..start]
            .trim_end_matches('\n')
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let before = registry[..header].trim_end_matches('\n');
        let after = &registry[end..];
        return if after.is_empty() {
            format!("{before}\n")
        } else {
            format!("{before}\n\n{after}")
        };
    }
    format!(
        "{}{}{}",
        &registry[..start],
        kept.concat(),
        &registry[end..]
    )
}

fn is_dinput8(value: &str) -> bool {
    value
        .trim_start()
        .to_ascii_lowercase()
        .starts_with(&NAME.to_ascii_lowercase())
}

/// Why the prefix's registry was not changed.
#[derive(Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// Wine runs in the prefix and would write its registry back over the
    /// change when it stops.
    Running,
    Other(String),
}

/// Sets the override in the prefix's `user.reg`.
///
/// # Errors
/// A prefix in use, or a failure to read or write.
pub fn set_override(prefix: &Path) -> Result<(), RegistryError> {
    change(prefix, |registry| {
        with_override(
            registry,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        )
    })
}

/// Removes the override from the prefix's `user.reg`.
///
/// # Errors
/// A prefix in use, or a failure to read or write.
pub fn remove_override(prefix: &Path) -> Result<(), RegistryError> {
    change(prefix, without_override)
}

fn change(prefix: &Path, edit: impl FnOnce(&str) -> String) -> Result<(), RegistryError> {
    if crate::system::prefix_running(prefix) {
        return Err(RegistryError::Running);
    }
    let path = prefix.join("user.reg");
    let registry = std::fs::read_to_string(&path)
        .map_err(|e| RegistryError::Other(format!("{}: {e}", path.display())))?;
    let changed = edit(&registry);
    if changed == registry {
        return Ok(());
    }
    let temporary = path.with_extension("reg.angelica-new");
    std::fs::write(&temporary, changed)
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|e| RegistryError::Other(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTON: &str = "WINE REGISTRY Version 2\n;; All keys relative to \\\\User\\\\S-1-5-21-0-0-0-1000\n\n#arch=win64\n\n[Software\\\\Wine\\\\DllOverrides] 1760000000\n#time=1dc3f\n\"atl100\"=\"native,builtin\"\n\n[Software\\\\Wine\\\\X11 Driver] 1760000000\n\"Decorated\"=\"Y\"\n";

    #[test]
    fn the_override_is_added_to_the_games_key_and_removed_without_a_trace() {
        assert!(!has_override(PROTON));
        let set = with_override(PROTON, 1_760_000_001);
        assert!(has_override(&set));
        assert!(set.starts_with(PROTON.trim_end_matches('\n')));
        assert!(set.ends_with(
            "\n\n[Software\\\\Wine\\\\AppDefaults\\\\ffxiv_dx11.exe\\\\DllOverrides] 1760000001\n\"dinput8\"=\"native,builtin\"\n"
        ));
        assert_eq!(with_override(&set, 1_760_000_002), set, "set once");
        assert_eq!(without_override(&set), PROTON);
        assert_eq!(without_override(PROTON), PROTON);
    }

    #[test]
    fn other_values_of_the_games_key_stay() {
        let registry = format!(
            "{PROTON}\n[software\\\\wine\\\\appdefaults\\\\ffxiv_dx11.exe\\\\dlloverrides] 1\n#time=1\n\"dinput8\"=\"builtin\"\n\"xinput1_3\"=\"native\"\n\n[Next] 1\n\"a\"=\"b\"\n"
        );
        assert!(!has_override(&registry));
        let set = with_override(&registry, 2);
        assert!(has_override(&set));
        assert_eq!(
            set.matches("dinput8").count(),
            1,
            "the other value is replaced"
        );
        assert!(
            set.contains("\"xinput1_3\"=\"native\"\n\"dinput8\"=\"native,builtin\"\n\n[Next] 1")
        );
        let removed = without_override(&set);
        assert!(!removed.contains("dinput8"));
        assert!(removed.contains("\"xinput1_3\"=\"native\"\n\n[Next] 1"));
    }

    #[test]
    fn the_installation_is_where_the_games_runner_can_see_it() {
        let home = Path::new("/home/deck");
        assert_eq!(
            installation_dir(
                home,
                None,
                Path::new("/home/deck/.steam/steam/steamapps/common/FINAL FANTASY XIV Online/game")
            ),
            Path::new("/home/deck/.local/share/angelica")
        );
        assert_eq!(
            installation_dir(home, Some(Path::new("/data")), Path::new("/mnt/sd/game")),
            Path::new("/data/angelica")
        );
        assert_eq!(
            installation_dir(
                home,
                None,
                Path::new("/home/deck/.var/app/dev.goats.xivlauncher/.xlcore/ffxiv/game")
            ),
            Path::new("/home/deck/.var/app/dev.goats.xivlauncher/data/angelica")
        );
        assert_eq!(
            windows_path(Path::new("/home/deck/.local/share/angelica")),
            r"Z:\home\deck\.local\share\angelica"
        );
    }
}
