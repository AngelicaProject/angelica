//! Where the game is installed (`docs/architecture/installer.md`).
use std::path::{Path, PathBuf};

/// The game's executable, relative to its `game` folder.
const EXECUTABLE: &str = "ffxiv_dx11.exe";
const STEAM_FOLDER: &str = "steamapps/common/FINAL FANTASY XIV Online/game";
const SQUARE_ENIX: &str =
    r"C:\Program Files (x86)\SquareEnix\FINAL FANTASY XIV - A Realm Reborn\game";

/// Whether `game` is the game's `game` folder.
#[must_use]
pub fn is_game(game: &Path) -> bool {
    game.join(EXECUTABLE).is_file()
}

/// The `game` folder for what a player typed or pasted: the folder itself,
/// or the installation that holds it.
#[must_use]
pub fn resolve(typed: &str) -> Option<PathBuf> {
    let path = PathBuf::from(typed.trim().trim_matches('"'));
    [path.clone(), path.join("game")]
        .into_iter()
        .find(|candidate| is_game(candidate))
}

/// Every installation found, in order: Steam libraries, the Square Enix
/// launcher's default folder, and `XIVLauncher`'s game path; each once.
#[must_use]
pub fn find() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(steam) = crate::system::steam_path() {
        let steam = PathBuf::from(steam);
        let libraries = std::fs::read_to_string(steam.join("steamapps/libraryfolders.vdf"))
            .map(|text| library_folders(&text))
            .unwrap_or_default();
        candidates.push(steam.join(STEAM_FOLDER));
        candidates.extend(libraries.iter().map(|library| library.join(STEAM_FOLDER)));
    }
    candidates.push(PathBuf::from(SQUARE_ENIX));
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let config = Path::new(&appdata).join("XIVLauncher/launcherConfigV3.json");
        if let Some(path) = std::fs::read_to_string(config)
            .ok()
            .and_then(|text| xivlauncher_game(&text))
        {
            candidates.push(path.join("game"));
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        let normal = std::fs::canonicalize(&candidate).unwrap_or(candidate.clone());
        if is_game(&candidate)
            && !found
                .iter()
                .any(|seen| std::fs::canonicalize(seen).unwrap_or(seen.clone()) == normal)
        {
            found.push(candidate);
        }
    }
    found
}

/// The library folders `libraryfolders.vdf` lists: its `"path"` values.
#[must_use]
pub fn library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut parts = line.split('"').filter(|part| !part.trim().is_empty());
            (parts.next()? == "path").then(|| parts.next())?
        })
        .map(|path| PathBuf::from(path.replace("\\\\", "\\")))
        .collect()
}

/// The game path in `XIVLauncher`'s configuration.
#[must_use]
pub fn xivlauncher_game(json: &str) -> Option<PathBuf> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("GamePath")?.as_str()?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_libraries_are_read_from_their_list() {
        let vdf = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps" { "39210" "1" }
	}
}"#;
        assert_eq!(
            library_folders(vdf),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary")
            ]
        );
    }

    #[test]
    fn the_launchers_game_path_and_typed_folders_resolve_to_the_game_folder() {
        assert_eq!(
            xivlauncher_game(r#"{"GamePath": "E:\\FFXIV", "Other": 1}"#),
            Some(PathBuf::from(r"E:\FFXIV"))
        );
        assert_eq!(xivlauncher_game(r#"{"GamePath": ""}"#), None);
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join(EXECUTABLE), b"").unwrap();
        assert_eq!(resolve(root.path().to_str().unwrap()), Some(game.clone()));
        let quoted = format!("\"{}\"", game.display());
        assert_eq!(resolve(&quoted), Some(game));
        assert_eq!(resolve(r"C:\nowhere"), None);
    }
}
