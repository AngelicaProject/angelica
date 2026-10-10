//! The fonts Angelica brings: Golos Text for everything the player reads
//! and presses, Quicksand for the word `angelica`, and `JetBrains Mono` for
//! ids, paths, and logs. Each is a static instance cut to Latin and Cyrillic;
//! all three are under the SIL Open Font License 1.1 (`fonts/OFL-*.txt`).
#![forbid(unsafe_code)]
use std::sync::Arc;

/// Golos Text medium: action words.
pub const MEDIUM: &str = "medium";
/// Golos Text semibold: names, the main action, and chosen values.
pub const STRONG: &str = "strong";
/// Quicksand semibold: the word `angelica`.
pub const WORDMARK: &str = "wordmark";

const FACES: [(&str, &[u8]); 5] = [
    ("golos", include_bytes!("../fonts/GolosText-Regular.ttf")),
    (
        "golos-medium",
        include_bytes!("../fonts/GolosText-Medium.ttf"),
    ),
    (
        "golos-semibold",
        include_bytes!("../fonts/GolosText-SemiBold.ttf"),
    ),
    (
        "quicksand-semibold",
        include_bytes!("../fonts/Quicksand-SemiBold.ttf"),
    ),
    (
        "jetbrains-mono",
        include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
    ),
];

/// The families and their own faces.
const FAMILIES: [(&str, &str); 3] = [
    (MEDIUM, "golos-medium"),
    (STRONG, "golos-semibold"),
    (WORDMARK, "quicksand-semibold"),
];

/// Angelica's font families, each followed by `fallbacks` in order: fonts
/// of the system for the scripts Angelica's own do not cover.
#[must_use]
pub fn definitions(fallbacks: Vec<(String, Vec<u8>)>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::empty();
    for (name, bytes) in FACES {
        fonts
            .font_data
            .insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
    }
    let mut families = vec![
        (egui::FontFamily::Proportional, "golos"),
        (egui::FontFamily::Monospace, "jetbrains-mono"),
    ];
    families.extend(
        FAMILIES
            .iter()
            .map(|(family, face)| (egui::FontFamily::Name((*family).into()), *face)),
    );
    let fallback_names: Vec<String> = fallbacks.iter().map(|(name, _)| name.clone()).collect();
    for (name, bytes) in fallbacks {
        fonts
            .font_data
            .insert(name, Arc::new(egui::FontData::from_owned(bytes)));
    }
    for (family, face) in families {
        let mut names = vec![face.to_owned()];
        // Golos Text covers what the others lack, such as Cyrillic in
        // Quicksand, before the system's fonts.
        if face != "golos" {
            names.push("golos".into());
        }
        names.extend(fallback_names.iter().cloned());
        fonts.families.insert(family, names);
    }
    fonts
}

/// Every family Angelica draws with.
#[cfg(test)]
fn all_families() -> Vec<egui::FontFamily> {
    let mut families = vec![egui::FontFamily::Proportional, egui::FontFamily::Monospace];
    families.extend(
        FAMILIES
            .iter()
            .map(|(family, _)| egui::FontFamily::Name((*family).into())),
    );
    families
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_has_its_own_face_first() {
        let fonts = definitions(Vec::new());
        for family in all_families() {
            let names = &fonts.families[&family];
            assert!(fonts.font_data.contains_key(&names[0]), "{family:?}");
        }
    }

    #[test]
    fn the_fonts_cover_every_interface_string() {
        use crate::{language::Language, manifest::Permission};
        let context = egui::Context::default();
        context.set_fonts(definitions(Vec::new()));
        // New fonts take effect when a frame begins.
        for _ in 0..2 {
            let mut output = context.run_ui(egui::RawInput::default(), |_| {});
            output.textures_delta.clear();
        }
        let mut texts = vec![
            "angelica ЁёЪъ «» — … · ← → ↑ ↓ ≤ ⌄".replace('⌄', ""),
            "Insert Esc".into(),
        ];
        let permissions = [
            Permission::ExcelText,
            Permission::TextCase,
            Permission::FilesReplace,
            Permission::ChatCommands,
            Permission::ChatPrint,
            Permission::HttpGet,
            Permission::StateSelf,
            Permission::ChatRead,
            Permission::GameUiHide,
            Permission::GameUiText,
        ];
        for language in Language::ALL {
            texts.push(language.name().into());
            texts.extend(
                language
                    .strings()
                    .entries()
                    .into_iter()
                    .map(|(_, text)| text.to_owned()),
            );
            for permission in permissions {
                texts.push(language.permission(permission).into());
                texts.push(language.class(permission.class()).into());
            }
        }
        context.fonts_mut(|fonts| {
            for family in all_families() {
                let font = egui::FontId::new(16.0, family.clone());
                for text in &texts {
                    // egui's own check takes the first face for the
                    // replacement; a glyph no face has has no width.
                    let missing: String = text
                        .chars()
                        .filter(|&c| fonts.glyph_width(&font, c) <= 0.0)
                        .collect();
                    assert!(
                        missing.is_empty(),
                        "{family:?} lacks {missing:?} of {text:?}"
                    );
                }
            }
        });
    }
}
