//! Checks of replacement game files against the game's original before the
//! host serves them (`docs/formats/game-structures.md`). Only formats the
//! host can check are accepted; the game parses what it reads without
//! checks of its own.
#![forbid(unsafe_code)]

/// The size of a texture file's header.
const TEXTURE_HEADER: usize = 80;
const FDT_HEADER: usize = 0x20;
const FDT_FONT_HEADER: usize = 0x20;
const FDT_GLYPH: usize = 16;
const FDT_KERNING_HEADER: usize = 0x10;
const FDT_KERNING_PAIR: usize = 16;
const FDT_MAX_GLYPHS: usize = 0x10000;
/// Font atlas pages: four channels of each of the game's seven font
/// textures (`font1.tex` to `font7.tex`, `font_lobby1.tex` to
/// `font_lobby7.tex`).
const FDT_MAX_PAGES: u16 = 4 * 7;

/// The file extensions the host can check.
pub const FORMATS: &[&str] = &["fdt", "tex"];

/// Checks `replacement` for the game file at `path`, whose original bytes
/// are `original`.
///
/// # Errors
/// Names the rule the replacement breaks, or the missing check for its
/// format.
pub fn check(path: &str, original: &[u8], replacement: &[u8]) -> Result<(), String> {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("tex") => check_texture(original, replacement),
        Some("fdt") => check_font_table(original, replacement),
        _ => Err(format!(
            "{path}: the host can check only these file formats: {}",
            FORMATS.join(", ")
        )),
    }
}

/// A texture keeps the original's header and size; only its pixels change.
fn check_texture(original: &[u8], replacement: &[u8]) -> Result<(), String> {
    if original.len() < TEXTURE_HEADER {
        return Err("the original texture is shorter than its header".into());
    }
    if replacement.len() != original.len() {
        return Err(format!(
            "a texture keeps the original's size: {} bytes, not {}",
            original.len(),
            replacement.len()
        ));
    }
    if replacement[..TEXTURE_HEADER] != original[..TEXTURE_HEADER] {
        return Err("a texture keeps the original's header: format, size, and mip levels".into());
    }
    Ok(())
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize
}

/// The parts of a font table: its glyph count and the offset of its
/// kerning header, after checking the layout.
fn font_table(bytes: &[u8]) -> Result<(usize, usize), String> {
    if bytes.len() < FDT_HEADER + FDT_FONT_HEADER + FDT_KERNING_HEADER || &bytes[..8] != b"fcsv0100"
    {
        return Err("not an fcsv0100 font table".into());
    }
    let (fthd, knhd) = (u32_at(bytes, 8), u32_at(bytes, 12));
    if fthd != FDT_HEADER || &bytes[fthd..fthd + 4] != b"fthd" {
        return Err("the font header is not at 0x20".into());
    }
    let glyphs = u32_at(bytes, fthd + 4);
    let kerning = u32_at(bytes, fthd + 8);
    if glyphs > FDT_MAX_GLYPHS
        || knhd != fthd + FDT_FONT_HEADER + glyphs * FDT_GLYPH
        || knhd + FDT_KERNING_HEADER > bytes.len()
        || &bytes[knhd..knhd + 4] != b"knhd"
    {
        return Err("the kerning header does not follow the glyph records".into());
    }
    if u32_at(bytes, knhd + 4) != kerning
        || bytes.len() != knhd + FDT_KERNING_HEADER + kerning * FDT_KERNING_PAIR
    {
        return Err("the kerning table does not match its header".into());
    }
    Ok((glyphs, knhd))
}

/// A font table keeps the original's headers (texture size, font size,
/// line height, ascent) and kerning; its glyph records are sorted and lie
/// inside the atlas.
fn check_font_table(original: &[u8], replacement: &[u8]) -> Result<(), String> {
    let (_, original_knhd) = font_table(original).map_err(|e| format!("the original: {e}"))?;
    let (glyphs, knhd) = font_table(replacement)?;
    let header = |bytes: &[u8]| {
        let mut header = bytes[..FDT_HEADER + FDT_FONT_HEADER].to_vec();
        // Glyph count and kerning offset change with the records.
        header[12..16].fill(0);
        header[FDT_HEADER + 4..FDT_HEADER + 8].fill(0);
        header
    };
    if header(original) != header(replacement) {
        return Err("a font table keeps the original's headers".into());
    }
    if original[original_knhd..] != replacement[knhd..] {
        return Err("a font table keeps the original's kerning".into());
    }
    let (width, height) = (
        usize::from(u16_at(replacement, FDT_HEADER + 0x10)),
        usize::from(u16_at(replacement, FDT_HEADER + 0x12)),
    );
    let mut previous = 0u32;
    for index in 0..glyphs {
        let at = FDT_HEADER + FDT_FONT_HEADER + index * FDT_GLYPH;
        let record = &replacement[at..at + FDT_GLYPH];
        let utf8 = u32::from_le_bytes(record[..4].try_into().expect("4 bytes"));
        // The game's tables list some characters twice; equal neighbours are allowed.
        if utf8 < previous {
            return Err(format!("glyph {index} is out of order"));
        }
        previous = utf8;
        let page = u16_at(record, 6);
        let (x, y) = (
            usize::from(u16_at(record, 8)),
            usize::from(u16_at(record, 10)),
        );
        let (glyph_width, glyph_height) = (usize::from(record[12]), usize::from(record[13]));
        if page >= FDT_MAX_PAGES || x + glyph_width > width || y + glyph_height > height {
            return Err(format!("glyph {index} lies outside the font's atlas"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(glyphs: &[(u32, u16, u16)]) -> Vec<u8> {
        let mut bytes = b"fcsv0100".to_vec();
        let knhd = FDT_HEADER + FDT_FONT_HEADER + glyphs.len() * FDT_GLYPH;
        bytes.extend_from_slice(&0x20u32.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(knhd).unwrap().to_le_bytes());
        bytes.resize(FDT_HEADER, 0);
        bytes.extend_from_slice(b"fthd");
        bytes.extend_from_slice(&u32::try_from(glyphs.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&1024u16.to_le_bytes());
        bytes.extend_from_slice(&1024u16.to_le_bytes());
        bytes.extend_from_slice(&12f32.to_le_bytes());
        bytes.extend_from_slice(&16i32.to_le_bytes());
        bytes.extend_from_slice(&12i32.to_le_bytes());
        for &(utf8, x, page) in glyphs {
            let mut record = [0u8; FDT_GLYPH];
            record[..4].copy_from_slice(&utf8.to_le_bytes());
            record[6..8].copy_from_slice(&page.to_le_bytes());
            record[8..10].copy_from_slice(&x.to_le_bytes());
            record[12] = 10;
            record[13] = 16;
            bytes.extend_from_slice(&record);
        }
        bytes.extend_from_slice(b"knhd");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&[7; FDT_KERNING_PAIR]);
        bytes
    }

    #[test]
    fn font_tables_may_gain_glyphs_inside_the_atlas() {
        let original = font(&[(0x41, 0, 0), (0x42, 10, 0)]);
        assert!(check("common/font/axis_12.fdt", &original, &original).is_ok());
        let added = font(&[(0x41, 0, 0), (0x42, 10, 0), (0xD090, 20, 9)]);
        assert!(check("common/font/axis_12.fdt", &original, &added).is_ok());
        for broken in [
            font(&[(0x42, 0, 0), (0x41, 10, 0)]),
            font(&[(0x41, 1020, 0)]),
            font(&[(0x41, 0, 28)]),
        ] {
            assert!(check("common/font/axis_12.fdt", &original, &broken).is_err());
        }
        let mut other_size = added.clone();
        other_size[FDT_HEADER + 0x18] = 17;
        assert!(
            check("common/font/axis_12.fdt", &original, &other_size).is_err(),
            "line height"
        );
        let mut kerning = added.clone();
        let last = kerning.len() - 1;
        kerning[last] = 8;
        assert!(
            check("common/font/axis_12.fdt", &original, &kerning).is_err(),
            "kerning"
        );
        assert!(
            check(
                "common/font/axis_12.fdt",
                &original,
                &added[..added.len() - 1]
            )
            .is_err()
        );
    }

    #[test]
    fn textures_keep_their_header_and_size() {
        let mut original = vec![0u8; TEXTURE_HEADER + 64];
        original[4] = 0x40;
        original[5] = 0x14;
        let mut painted = original.clone();
        painted[TEXTURE_HEADER + 3] = 0xFF;
        assert!(check("common/font/font1.tex", &original, &painted).is_ok());
        let mut header = painted.clone();
        header[8] = 1;
        assert!(check("common/font/font1.tex", &original, &header).is_err());
        assert!(
            check(
                "common/font/font1.tex",
                &original,
                &painted[..painted.len() - 1]
            )
            .is_err()
        );
        assert!(
            check("chara/model.mdl", &original, &painted).is_err(),
            "no check for models"
        );
    }

    #[test]
    fn damaged_replacements_are_refused_without_panicking() {
        let font_original = font(&[(0x41, 0, 0), (0x42, 10, 0)]);
        let font_added = font(&[(0x41, 0, 0), (0x42, 10, 0), (0xD090, 20, 9)]);
        let mut texture = vec![0u8; TEXTURE_HEADER + 64];
        texture[4] = 0x40;
        let mut mutator = crate::mutations::Mutator::new(7);
        for _ in 0..50_000 {
            let _ = check(
                "common/font/axis_12.fdt",
                &font_original,
                &mutator.mutate(&font_added),
            );
            let _ = check(
                "common/font/axis_12.fdt",
                &mutator.mutate(&font_original),
                &font_added,
            );
            let _ = check("common/font/font1.tex", &texture, &mutator.mutate(&texture));
        }
    }
}
