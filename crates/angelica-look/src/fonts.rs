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

/// The faces of Angelica's fonts by name, in the build that carries their
/// bytes.
#[cfg(feature = "faces")]
pub const FACES: [(&str, &[u8]); 5] = [
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

/// The names of the faces, which [`definitions_of`] expects.
pub const FACE_NAMES: [&str; 5] = [
    "golos",
    "golos-medium",
    "golos-semibold",
    "quicksand-semibold",
    "jetbrains-mono",
];

/// The families and their own faces.
const FAMILIES: [(&str, &str); 3] = [
    (MEDIUM, "golos-medium"),
    (STRONG, "golos-semibold"),
    (WORDMARK, "quicksand-semibold"),
];

/// Angelica's font families, each followed by `fallbacks` in order: fonts
/// of the system for the scripts Angelica's own do not cover.
#[cfg(feature = "faces")]
#[must_use]
pub fn definitions(fallbacks: Vec<(String, Vec<u8>)>) -> egui::FontDefinitions {
    definitions_of(
        FACES
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), egui::FontData::from_static(bytes)))
            .collect(),
        fallbacks,
    )
}

/// Angelica's font families from `faces`, by the names of
/// [`FACE_NAMES`], each followed by `fallbacks` in order.
#[must_use]
pub fn definitions_of(
    faces: Vec<(String, egui::FontData)>,
    fallbacks: Vec<(String, Vec<u8>)>,
) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::empty();
    for (name, data) in faces {
        fonts.font_data.insert(name, Arc::new(data));
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
#[must_use]
pub fn all_families() -> Vec<egui::FontFamily> {
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

    #[cfg(feature = "faces")]
    #[test]
    fn every_family_has_its_own_face_first() {
        let fonts = definitions(Vec::new());
        for family in all_families() {
            let names = &fonts.families[&family];
            assert!(fonts.font_data.contains_key(&names[0]), "{family:?}");
        }
    }
}
