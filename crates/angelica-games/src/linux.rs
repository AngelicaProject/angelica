//! Where Steam and `XIVLauncher.Core` keep the game on Linux, and the Wine
//! prefixes it runs in. Every path is found from the user's home folder.
use std::path::{Path, PathBuf};

/// Steam's app id of the game: its prefix is `compatdata/39210/pfx`.
const STEAM_APP: &str = "39210";
/// The folder of the `XIVLauncher.Core` flatpak, which keeps its
/// `.xlcore` inside.
const XLCORE_FLATPAK: &str = ".var/app/dev.goats.xivlauncher";

/// Steam's folders: the native client's and the flatpak's.
pub fn steam_roots(home: &Path) -> Vec<PathBuf> {
    [
        ".steam/steam",
        ".local/share/Steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .iter()
    .map(|folder| home.join(folder))
    .filter(|root| root.is_dir())
    .collect()
}

/// `XIVLauncher.Core`'s folders: `~/.xlcore`, and the flatpak's.
pub fn xlcore_roots(home: &Path) -> Vec<PathBuf> {
    [
        home.join(".xlcore"),
        home.join(XLCORE_FLATPAK).join(".xlcore"),
    ]
    .into_iter()
    .filter(|root| root.is_dir())
    .collect()
}

/// The game path `XIVLauncher.Core` in `root` uses: `GamePath` from its
/// `launcher.ini`, or its own `ffxiv` folder. The flatpak names its own
/// folder as `~/.xlcore`; such a path is turned into where it really is.
pub fn xlcore_game(home: &Path, root: &Path) -> PathBuf {
    let configured = std::fs::read_to_string(root.join("launcher.ini"))
        .ok()
        .and_then(|text| ini_value(&text, "GamePath"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let Some(path) = configured else {
        return root.join("ffxiv");
    };
    match path.strip_prefix(home.join(".xlcore")) {
        Ok(inside) if root != home.join(".xlcore") => root.join(inside),
        _ => path,
    }
}

/// The value of `key` in an ini file's lines, `key=value`.
fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

pub fn wine_prefixes(home: &Path, game: &Path) -> Vec<PathBuf> {
    let game = std::fs::canonicalize(game).unwrap_or_else(|_| game.to_path_buf());
    let mut prefixes = Vec::new();
    // `<library>/steamapps/common/FINAL FANTASY XIV Online/game`.
    if let Some(steamapps) = game
        .parent()
        .and_then(Path::parent)
        .filter(|common| common.file_name().is_some_and(|name| name == "common"))
        .and_then(Path::parent)
    {
        prefixes.push(steamapps.join("compatdata").join(STEAM_APP).join("pfx"));
    }
    for root in xlcore_roots(home) {
        let configured = xlcore_game(home, &root).join("game");
        if std::fs::canonicalize(&configured).is_ok_and(|configured| configured == game) {
            prefixes.push(root.join("wineprefix"));
        }
    }
    if let Some(drive) = game
        .ancestors()
        .find(|folder| folder.file_name().is_some_and(|name| name == "drive_c"))
        && let Some(prefix) = drive.parent()
    {
        prefixes.push(prefix.to_path_buf());
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for prefix in prefixes {
        if prefix.join("user.reg").is_file() && !found.contains(&prefix) {
            found.push(prefix);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_in(folder: &Path) -> PathBuf {
        let game = folder.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("ffxiv_dx11.exe"), b"").unwrap();
        game
    }

    fn prefix(folder: &Path) {
        std::fs::create_dir_all(folder).unwrap();
        std::fs::write(folder.join("user.reg"), b"WINE REGISTRY Version 2\n").unwrap();
    }

    #[test]
    fn steams_game_runs_in_the_prefix_of_its_library() {
        let home = tempfile::tempdir().unwrap();
        let library = home.path().join("games/SteamLibrary");
        let game = game_in(&library.join("steamapps/common/FINAL FANTASY XIV Online"));
        assert!(
            wine_prefixes(home.path(), &game).is_empty(),
            "a game never started has no prefix"
        );
        prefix(&library.join("steamapps/compatdata/39210/pfx"));
        assert_eq!(
            wine_prefixes(home.path(), &game),
            vec![
                std::fs::canonicalize(&library)
                    .unwrap()
                    .join("steamapps/compatdata/39210/pfx")
            ]
        );
    }

    #[test]
    fn xivlauncher_core_names_its_game_and_prefix_also_from_its_flatpak() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let root = home.join(".xlcore");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(xlcore_game(home, &root), root.join("ffxiv"));
        let game = game_in(&root.join("ffxiv"));
        prefix(&root.join("wineprefix"));
        assert_eq!(wine_prefixes(home, &game), vec![root.join("wineprefix")]);

        let flatpak = home.join(XLCORE_FLATPAK).join(".xlcore");
        std::fs::create_dir_all(&flatpak).unwrap();
        std::fs::write(
            flatpak.join("launcher.ini"),
            format!(
                "AcceptLanguage=en\nGamePath = {}\n",
                home.join(".xlcore/game-copy").display()
            ),
        )
        .unwrap();
        assert_eq!(xlcore_game(home, &flatpak), flatpak.join("game-copy"));
        assert_eq!(xlcore_roots(home), vec![root, flatpak]);
    }

    #[test]
    fn a_game_inside_a_prefix_runs_in_that_prefix() {
        let home = tempfile::tempdir().unwrap();
        let wine = home.path().join("Games/ffxiv");
        prefix(&wine);
        let game = game_in(
            &wine.join("drive_c/Program Files (x86)/SquareEnix/FINAL FANTASY XIV - A Realm Reborn"),
        );
        assert_eq!(
            wine_prefixes(home.path(), &game),
            vec![std::fs::canonicalize(&wine).unwrap()]
        );
    }
}
