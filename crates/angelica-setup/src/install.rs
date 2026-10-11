//! What the installer writes and removes (`docs/architecture/installer.md`).
use std::path::{Path, PathBuf};

/// A string only Angelica's proxy carries: it names the proxy's log.
const PROXY_MARK: &[u8] = b"angelica-loader.log";
/// The folders that hold the player's plugins and what they chose.
pub const PLAYER_DATA: [&str; 4] = ["plugins", "grants", "state", "storage"];

/// Why installing into the game folder stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum PlaceError {
    /// Another tool's `dinput8.dll` is in the game folder.
    OtherTool,
    /// The game holds the proxy open.
    InUse,
    Other(String),
}

/// `%LOCALAPPDATA%\Angelica`.
#[must_use]
pub fn installation_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Angelica")
}

/// Whether `bytes` are Angelica's proxy of any version.
#[must_use]
pub fn is_angelica_proxy(bytes: &[u8]) -> bool {
    bytes
        .windows(PROXY_MARK.len())
        .any(|window| window == PROXY_MARK)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let temporary = path.with_extension("angelica-new");
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes the version folder, names it in `current.json`, and records the
/// channel unless the player chose one; the player's plugins, grants,
/// settings, and storage stay.
///
/// # Errors
/// A failure to write.
pub fn install_files(
    installation: &Path,
    version: &str,
    channel: &str,
    files: &[(&str, &[u8], &[u8])],
) -> Result<(), String> {
    let folder = installation.join("versions").join(version);
    for (name, bytes, signature) in files {
        if *name == "dinput8.dll" {
            continue;
        }
        write(&folder.join(name), bytes)?;
        write(&folder.join(format!("{name}.sig")), signature)?;
    }
    write(
        &installation.join("current.json"),
        serde_json::json!({"format": 1, "version": version})
            .to_string()
            .as_bytes(),
    )?;
    let settings = installation.join("state/_angelica.json");
    let mut value = std::fs::read(&settings)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .filter(|value| value.get("format") == Some(&1.into()))
        .unwrap_or_else(|| serde_json::json!({"format": 1}));
    if value.get("channel").is_none() {
        value["channel"] = channel.into();
    }
    write(&settings, value.to_string().as_bytes())?;
    // Older version folders go when nothing uses them.
    if let Ok(listing) = std::fs::read_dir(installation.join("versions")) {
        for item in listing.filter_map(Result::ok) {
            if item.file_name() != version {
                let _ = std::fs::remove_dir_all(item.path());
            }
        }
    }
    Ok(())
}

/// Whether the user can write into `game` without administrator rights.
#[must_use]
pub fn writable(game: &Path) -> bool {
    let probe = game.join(".angelica-probe");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Puts the proxy and `angelica.json` into the game folder.
///
/// # Errors
/// Another tool's `dinput8.dll`, a proxy the game holds open, or another
/// failure to write.
pub fn place(game: &Path, installation: &Path, proxy: &[u8]) -> Result<(), PlaceError> {
    let target = game.join("dinput8.dll");
    if let Ok(existing) = std::fs::read(&target)
        && !is_angelica_proxy(&existing)
    {
        return Err(PlaceError::OtherTool);
    }
    if target.exists()
        && std::fs::OpenOptions::new()
            .write(true)
            .open(&target)
            .is_err()
    {
        return Err(PlaceError::InUse);
    }
    write(&target, proxy).map_err(PlaceError::Other)?;
    let config = serde_json::json!({ "installation_dir": installation }).to_string();
    write(&game.join("angelica.json"), config.as_bytes()).map_err(PlaceError::Other)
}

/// Removes the proxy, when it is Angelica's, and `angelica.json` from the
/// game folder.
///
/// # Errors
/// A proxy the game holds open, or another failure.
pub fn remove_from_game(game: &Path) -> Result<(), PlaceError> {
    let proxy = game.join("dinput8.dll");
    if std::fs::read(&proxy).is_ok_and(|bytes| is_angelica_proxy(&bytes)) {
        std::fs::remove_file(&proxy).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                PlaceError::InUse
            } else {
                PlaceError::Other(e.to_string())
            }
        })?;
    }
    match std::fs::remove_file(game.join("angelica.json")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(PlaceError::Other(e.to_string())),
        _ => Ok(()),
    }
}

/// Removes the installation; with `keep`, the player's data stays for a
/// later installation.
pub fn remove_installation(installation: &Path, keep: bool) {
    let Ok(listing) = std::fs::read_dir(installation) else {
        return;
    };
    for item in listing.filter_map(Result::ok) {
        let name = item.file_name();
        if keep && PLAYER_DATA.iter().any(|kept| name == *kept) {
            continue;
        }
        let path = item.path();
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
    if !keep {
        let _ = std::fs::remove_dir(installation);
    }
}

/// The game folder an installation's proxy was placed into, from the
/// uninstall entry's record.
#[must_use]
pub fn recorded_game(installation: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(installation.join("game.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("game")?.as_str().map(PathBuf::from)
}

/// Records the game folder for the uninstaller.
///
/// # Errors
/// A failure to write.
pub fn record_game(installation: &Path, game: &Path) -> Result<(), String> {
    write(
        &installation.join("game.json"),
        serde_json::json!({ "game": game }).to_string().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROXY: &[u8] = b"MZ...angelica-loader.log...";

    #[test]
    fn installing_keeps_the_players_data_and_choices_and_replaces_older_versions() {
        let root = tempfile::tempdir().unwrap();
        let installation = root.path();
        std::fs::create_dir_all(installation.join("versions/0.1.0")).unwrap();
        std::fs::create_dir_all(installation.join("state")).unwrap();
        std::fs::write(
            installation.join("state/_angelica.json"),
            r#"{"format":1,"language":"ru","channel":"nightly"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(installation.join("storage/harmonia")).unwrap();
        let files: [(&str, &[u8], &[u8]); 3] = [
            ("runtime.dll", b"runtime", &[1; 64]),
            ("plugin-installer.exe", b"installer", &[2; 64]),
            ("dinput8.dll", PROXY, &[3; 64]),
        ];
        install_files(installation, "0.2.0", "stable", &files).unwrap();
        let version = installation.join("versions/0.2.0");
        assert_eq!(
            std::fs::read(version.join("runtime.dll")).unwrap(),
            b"runtime"
        );
        assert_eq!(
            std::fs::read(version.join("plugin-installer.exe.sig")).unwrap(),
            [2; 64]
        );
        assert!(
            !version.join("dinput8.dll").exists(),
            "the proxy goes into the game folder"
        );
        assert!(!installation.join("versions/0.1.0").exists());
        let current: serde_json::Value =
            serde_json::from_slice(&std::fs::read(installation.join("current.json")).unwrap())
                .unwrap();
        assert_eq!(current["version"], "0.2.0");
        let settings: serde_json::Value = serde_json::from_slice(
            &std::fs::read(installation.join("state/_angelica.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["channel"], "nightly", "the player's choice stays");
        assert_eq!(settings["language"], "ru");
        assert!(installation.join("storage/harmonia").exists());
    }

    #[test]
    fn another_tools_proxy_is_never_replaced_and_ours_is() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        let installation = root.path().join("Angelica");
        std::fs::write(game.join("dinput8.dll"), b"MZ another tool").unwrap();
        assert_eq!(
            place(&game, &installation, PROXY),
            Err(PlaceError::OtherTool)
        );
        assert_eq!(
            std::fs::read(game.join("dinput8.dll")).unwrap(),
            b"MZ another tool"
        );
        assert!(!game.join("angelica.json").exists());

        std::fs::write(game.join("dinput8.dll"), b"old angelica-loader.log").unwrap();
        place(&game, &installation, PROXY).unwrap();
        assert_eq!(std::fs::read(game.join("dinput8.dll")).unwrap(), PROXY);
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(game.join("angelica.json")).unwrap()).unwrap();
        assert_eq!(config["installation_dir"], installation.to_str().unwrap());

        remove_from_game(&game).unwrap();
        assert!(!game.join("dinput8.dll").exists());
        assert!(!game.join("angelica.json").exists());
        std::fs::write(game.join("dinput8.dll"), b"MZ another tool").unwrap();
        remove_from_game(&game).unwrap();
        assert!(
            game.join("dinput8.dll").exists(),
            "another tool's file stays"
        );
    }

    #[test]
    fn removing_can_keep_the_players_data() {
        let root = tempfile::tempdir().unwrap();
        let installation = root.path().join("Angelica");
        for folder in [
            "versions/0.2.0",
            "plugins/harmonia",
            "storage/harmonia",
            "logs",
        ] {
            std::fs::create_dir_all(installation.join(folder)).unwrap();
        }
        std::fs::write(installation.join("current.json"), b"{}").unwrap();
        remove_installation(&installation, true);
        assert!(installation.join("plugins/harmonia").exists());
        assert!(installation.join("storage/harmonia").exists());
        assert!(!installation.join("versions").exists());
        assert!(!installation.join("logs").exists());
        assert!(!installation.join("current.json").exists());
        remove_installation(&installation, false);
        assert!(!installation.exists());
    }
}
