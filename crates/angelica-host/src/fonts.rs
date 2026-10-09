//! The fonts Angelica brings: Cormorant Garamond italic for names and
//! titles, Manrope for what the player reads and presses, and `JetBrains
//! Mono` for the mechanism. Each is a static instance cut to Latin and Cyrillic;
//! all three are under the SIL Open Font License 1.1 (`fonts/OFL-*.txt`).
#![forbid(unsafe_code)]
use std::sync::Arc;

/// The soft italic of names and large titles.
pub const SOFT: &str = "soft";
/// The light soft italic of the word *angelica* and search fields.
pub const SOFT_LIGHT: &str = "soft-light";
/// Manrope semibold: buttons and emphasis.
pub const STRONG: &str = "strong";

const FACES: [(&str, &[u8]); 5] = [
    (
        "cormorant-italic",
        include_bytes!("../fonts/CormorantGaramond-Italic.ttf"),
    ),
    (
        "cormorant-light-italic",
        include_bytes!("../fonts/CormorantGaramond-LightItalic.ttf"),
    ),
    ("manrope", include_bytes!("../fonts/Manrope-Regular.ttf")),
    (
        "manrope-semibold",
        include_bytes!("../fonts/Manrope-SemiBold.ttf"),
    ),
    (
        "jetbrains-mono",
        include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
    ),
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
    let families = [
        (egui::FontFamily::Proportional, "manrope"),
        (egui::FontFamily::Monospace, "jetbrains-mono"),
        (egui::FontFamily::Name(SOFT.into()), "cormorant-italic"),
        (
            egui::FontFamily::Name(SOFT_LIGHT.into()),
            "cormorant-light-italic",
        ),
        (egui::FontFamily::Name(STRONG.into()), "manrope-semibold"),
    ];
    let fallback_names: Vec<String> = fallbacks.iter().map(|(name, _)| name.clone()).collect();
    for (name, bytes) in fallbacks {
        fonts
            .font_data
            .insert(name, Arc::new(egui::FontData::from_owned(bytes)));
    }
    for (family, face) in families {
        let mut names = vec![face.to_owned()];
        // Manrope covers what the italic and the monospace lack, such as
        // arrows, before the system's fonts.
        if face != "manrope" {
            names.push("manrope".into());
        }
        names.extend(fallback_names.iter().cloned());
        fonts.families.insert(family, names);
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_has_its_own_face_first() {
        let fonts = definitions(Vec::new());
        for family in [
            egui::FontFamily::Proportional,
            egui::FontFamily::Monospace,
            egui::FontFamily::Name(SOFT.into()),
            egui::FontFamily::Name(SOFT_LIGHT.into()),
            egui::FontFamily::Name(STRONG.into()),
        ] {
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
            "angelica ЁёЪъ «» — … · ← → ≤ ✓".replace('✓', ""),
            "wasm sandbox · signed packages · your permission".into(),
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
            for family in [
                egui::FontFamily::Proportional,
                egui::FontFamily::Monospace,
                egui::FontFamily::Name(SOFT.into()),
                egui::FontFamily::Name(SOFT_LIGHT.into()),
                egui::FontFamily::Name(STRONG.into()),
            ] {
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
