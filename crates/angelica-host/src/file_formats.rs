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
        Some("tex") if path.starts_with("common/font/") => {
            check_font_texture(original, replacement)
        }
        Some("tex") => check_texture(original, replacement),
        Some("fdt") => check_font_table(original, replacement),
        _ => Err(format!(
            "{path}: the host can check only these file formats: {}",
            FORMATS.join(", ")
        )),
    }
}

/// The texture kinds: one of these bits is the whole attribute word of
/// every texture the game ships (2D, 3D, and cube; it ships no arrays).
const TEXTURE_2D: u32 = 0x0080_0000;
const TEXTURE_3D: u32 = 0x0100_0000;
const TEXTURE_CUBE: u32 = 0x0200_0000;
/// The pixel formats the game ships textures in. The second nibble from
/// the top is the format's kind (1 integer, 2 float, 3 BC1-3, 6 BC4/5/7),
/// the third the log2 of its bits per pixel.
const TEXTURE_FORMATS: [u32; 13] = [
    0x1130, 0x1131, 0x1440, 0x1441, 0x1450, 0x1451, 0x2460, 0x3420, 0x3430, 0x3431, 0x6120, 0x6230,
    0x6432,
];
/// The largest side of a replacement texture; the game ships up to 4096.
const TEXTURE_MAX_SIDE: u32 = 8192;
/// The most mip levels a header describes.
const TEXTURE_MAX_MIPS: usize = 13;
/// The deepest 3D texture.
const TEXTURE_MAX_DEPTH: u32 = 256;

/// What a texture's header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Texture {
    kind: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    mips: usize,
}

/// Reads and checks a texture's header against its data: one known kind
/// and format, sides within limits (multiples of 4 for block formats), mip
/// levels that the size allows, levels of detail among them, and every
/// level's surfaces after the header and inside the file, unused offsets
/// zero. These rules hold for every texture the game ships; a few of its
/// textures point every level at the same surface, so levels may overlap.
fn texture(bytes: &[u8]) -> Result<Texture, String> {
    if bytes.len() < TEXTURE_HEADER {
        return Err("a texture is shorter than its header".into());
    }
    let word =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let half = |at: usize| u32::from(u16_at(bytes, at));
    let kind = word(0);
    if ![TEXTURE_2D, TEXTURE_3D, TEXTURE_CUBE].contains(&kind) {
        return Err(format!("{kind:#x} is not a 2D, 3D, or cube texture"));
    }
    let format = word(4);
    if !TEXTURE_FORMATS.contains(&format) {
        return Err(format!(
            "texture format {format:#x} is not one the game uses"
        ));
    }
    let (width, height, depth) = (half(8), half(10), half(12));
    let mips = usize::from(bytes[14] & 0x7F);
    let array = bytes[15];
    let blocks = matches!(format >> 12, 3 | 6);
    if width == 0 || height == 0 || width.max(height) > TEXTURE_MAX_SIDE {
        return Err(format!("a texture's sides are 1 to {TEXTURE_MAX_SIDE}"));
    }
    if blocks && (width % 4 != 0 || height % 4 != 0) {
        return Err("a block-compressed texture's sides are multiples of 4".into());
    }
    let deep = kind == TEXTURE_3D;
    if (deep && !(1..=TEXTURE_MAX_DEPTH).contains(&depth)) || (!deep && depth != 1) {
        return Err(format!(
            "only a 3D texture has a depth, 1 to {TEXTURE_MAX_DEPTH}"
        ));
    }
    if array > 1 {
        return Err("a texture is not an array".into());
    }
    let largest = width.max(height).max(if deep { depth } else { 1 });
    let most_mips = usize::try_from(u32::BITS - largest.leading_zeros()).unwrap_or(0);
    if mips == 0 || mips > most_mips.min(TEXTURE_MAX_MIPS) {
        return Err(format!("{mips} mip levels for a {width}x{height} texture"));
    }
    if (0..3).any(|lod| word(16 + 4 * lod) as usize >= mips) {
        return Err("a level of detail names a missing mip level".into());
    }
    let bits = 1u64 << ((format & 0xF0) >> 4);
    for level in 0..TEXTURE_MAX_MIPS {
        let offset = u64::from(word(28 + 4 * level));
        if level >= mips {
            if offset != 0 {
                return Err(format!("mip level {level} is not used but has an offset"));
            }
            continue;
        }
        if offset < TEXTURE_HEADER as u64 {
            return Err(format!("mip level {level} starts inside the header"));
        }
        let (w, h) = (
            u64::from((width >> level).max(1)),
            u64::from((height >> level).max(1)),
        );
        let slice = if blocks {
            w.div_ceil(4) * h.div_ceil(4) * bits * 2
        } else {
            w * h * bits / 8
        };
        let slices = match kind {
            TEXTURE_3D => u64::from((depth >> level).max(1)),
            TEXTURE_CUBE => 6,
            _ => 1,
        };
        if offset + slice * slices > bytes.len() as u64 {
            return Err(format!("mip level {level} ends past the file"));
        }
    }
    Ok(Texture {
        kind,
        format,
        width,
        height,
        depth,
        mips,
    })
}

/// A texture may change its format, size, and mip levels, but stays the
/// kind of texture the original is.
fn check_texture(original: &[u8], replacement: &[u8]) -> Result<(), String> {
    let original = texture(original).map_err(|problem| format!("the original: {problem}"))?;
    let replacement = texture(replacement)?;
    if replacement.kind != original.kind {
        return Err("a texture stays 2D, 3D, or cube as the original is".into());
    }
    Ok(())
}

/// A font texture keeps the original's header and size, which the font
/// tables' glyphs are placed in; only its pixels change.
fn check_font_texture(original: &[u8], replacement: &[u8]) -> Result<(), String> {
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
pub(crate) mod tests {
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

    /// A texture of `kind` and `format` with its mip levels after the
    /// header, in order.
    pub(crate) fn texture_file(
        kind: u32,
        format: u32,
        width: u16,
        height: u16,
        mips: u8,
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; TEXTURE_HEADER];
        bytes[..4].copy_from_slice(&kind.to_le_bytes());
        bytes[4..8].copy_from_slice(&format.to_le_bytes());
        bytes[8..10].copy_from_slice(&width.to_le_bytes());
        bytes[10..12].copy_from_slice(&height.to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        bytes[14] = mips;
        let bits = 1usize << ((format & 0xF0) >> 4);
        let faces = if kind == TEXTURE_CUBE { 6 } else { 1 };
        for level in 0..usize::from(mips) {
            let offset = u32::try_from(bytes.len()).unwrap();
            bytes[28 + 4 * level..32 + 4 * level].copy_from_slice(&offset.to_le_bytes());
            let w = usize::from((width >> level).max(1));
            let h = usize::from((height >> level).max(1));
            let size = if matches!(format >> 12, 3 | 6) {
                w.div_ceil(4) * h.div_ceil(4) * bits * 2
            } else {
                w * h * bits / 8
            };
            bytes.resize(bytes.len() + size * faces, 0x5A);
        }
        bytes
    }

    #[test]
    fn textures_change_format_and_size_but_not_kind() {
        const BC1: u32 = 0x3420;
        const BC7: u32 = 0x6432;
        const BGRA: u32 = 0x1450;
        let path = "chara/equipment/e0001/texture/v01_c0101e0001_top_d.tex";
        let original = texture_file(TEXTURE_2D, BC1, 512, 512, 10);
        for replacement in [
            texture_file(TEXTURE_2D, BC7, 2048, 1024, 12),
            texture_file(TEXTURE_2D, BGRA, 3, 5, 1),
            original.clone(),
        ] {
            check(path, &original, &replacement).unwrap();
        }
        let refused = |replacement: &[u8], problem: &str| {
            let error = check(path, &original, replacement).unwrap_err();
            assert!(error.contains(problem), "{error}");
        };
        refused(&texture_file(TEXTURE_CUBE, BC1, 64, 64, 1), "stays 2D");
        refused(
            &texture_file(TEXTURE_2D, 0x6330, 64, 64, 1),
            "not one the game uses",
        );
        refused(&texture_file(TEXTURE_2D, BC1, 6, 8, 1), "multiples of 4");
        refused(
            &texture_file(TEXTURE_2D, BC1, 16384, 4, 1),
            "sides are 1 to",
        );
        refused(&texture_file(TEXTURE_2D, BC1, 64, 64, 8), "mip levels");
        let full = texture_file(TEXTURE_2D, BC1, 64, 64, 3);
        refused(&full[..full.len() - 1], "ends past the file");
        let mut shared = full.clone();
        for level in 1..3 {
            shared[28 + 4 * level..32 + 4 * level].copy_from_slice(&80u32.to_le_bytes());
        }
        check(path, &original, &shared).unwrap();
        let mut inside = full.clone();
        inside[32..36].copy_from_slice(&40u32.to_le_bytes());
        refused(&inside, "inside the header");
        let mut stray = full.clone();
        stray[28 + 4 * 5..32 + 4 * 5].copy_from_slice(&999u32.to_le_bytes());
        refused(&stray, "not used");
        let mut lod = full.clone();
        lod[16] = 3;
        refused(&lod, "level of detail");
        let mut array = full.clone();
        array[15] = 2;
        refused(&array, "not an array");
        let mut flags = full;
        flags[0] |= 0x04;
        refused(&flags, "not a 2D");
        let cube = texture_file(TEXTURE_CUBE, BGRA, 16, 16, 5);
        check("chara/common/texture/cube.tex", &cube, &cube).unwrap();
        assert!(
            check(path, &original[..40], &original)
                .unwrap_err()
                .contains("the original"),
        );
    }

    #[test]
    fn font_textures_keep_their_header_and_size() {
        let original = texture_file(TEXTURE_2D, 0x1440, 8, 8, 1);
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

    /// Every texture the installed game ships passes the checks:
    /// `ANGELICA_GAME_PATH` is its `game` folder. Takes minutes.
    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH"]
    fn every_texture_of_the_installed_game_passes() {
        let game = std::path::PathBuf::from(
            std::env::var("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH"),
        );
        let data = angelica_sqpack::GameData::open(game.parent().unwrap()).unwrap();
        let mut checked = 0;
        for category in ["common", "bgcommon", "chara", "ui", "vfx"] {
            data.each_file("ffxiv", category, |file| {
                let Ok(bytes) = file else { return };
                // Other files of the category: a texture's first word is
                // one kind bit, its format's kind 1 to 6.
                let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                if bytes.len() < TEXTURE_HEADER
                    || word(0).count_ones() != 1
                    || word(0) & 0x13C0_0000 == 0
                    || !matches!(word(4) >> 12, 1..=6)
                {
                    return;
                }
                checked += 1;
                if let Err(problem) = texture(&bytes) {
                    panic!("{category}: {problem}: {:?}", &bytes[..32]);
                }
            })
            .unwrap();
        }
        assert!(checked > 100_000, "{checked}");
    }

    #[test]
    fn damaged_replacements_are_refused_without_panicking() {
        let font_original = font(&[(0x41, 0, 0), (0x42, 10, 0)]);
        let font_added = font(&[(0x41, 0, 0), (0x42, 10, 0), (0xD090, 20, 9)]);
        let texture = texture_file(TEXTURE_2D, 0x3420, 64, 64, 4);
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
            let _ = check("chara/a.tex", &texture, &mutator.mutate(&texture));
            let _ = check("chara/a.tex", &mutator.mutate(&texture), &texture);
        }
    }
}
