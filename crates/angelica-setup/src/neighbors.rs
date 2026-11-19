//! The launcher and other tools found by the installer.
use std::path::Path;

/// Tools the installer names beside the game.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Neighbors {
    /// `XIVLauncher` or `XIVLauncher.Core` is installed.
    pub xivlauncher: bool,
    /// `ReShade` in the game folder.
    pub reshade: bool,
    /// A `dinput8.dll` of another program, which Angelica will not replace.
    pub other_proxy: bool,
}

impl Neighbors {
    /// Looks beside `game`, the game's `game` folder.
    #[must_use]
    pub fn find(game: &Path, xivlauncher: bool) -> Self {
        let reshade = ["dxgi.dll", "d3d11.dll"].iter().any(|name| {
            std::fs::read(game.join(name)).is_ok_and(|bytes| {
                bytes
                    .windows(b"ReShade".len())
                    .any(|window| window == b"ReShade")
            })
        }) || game.join("ReShade.ini").exists();
        let other_proxy = std::fs::read(game.join("dinput8.dll"))
            .is_ok_and(|bytes| !crate::install::is_angelica_proxy(&bytes));
        Self {
            xivlauncher,
            reshade,
            other_proxy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_beside_the_game_are_named_and_another_proxy_stops_the_installation() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        assert_eq!(Neighbors::find(&game, false), Neighbors::default());

        std::fs::write(game.join("dxgi.dll"), b"MZ...ReShade...").unwrap();
        std::fs::write(game.join("dinput8.dll"), b"MZ another tool").unwrap();
        let found = Neighbors::find(&game, true);
        assert!(found.xivlauncher);
        assert!(found.reshade);
        assert!(found.other_proxy);

        std::fs::write(game.join("dinput8.dll"), b"angelica-loader.log").unwrap();
        assert!(!Neighbors::find(&game, true).other_proxy);
    }
}
