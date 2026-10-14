//! Checks of replacement game files against the game's original before the
//! host serves them (`docs/formats/game-structures.md`). Only formats the
//! host can check are accepted; the game parses what it reads without
//! checks of its own.
#![forbid(unsafe_code)]

mod model;

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
pub const FORMATS: &[&str] = &["fdt", "mdl", "mtrl", "tex"];

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
        Some("mtrl") => check_material(original, replacement),
        Some("mdl") => {
            model::model(original).map_err(|problem| format!("the original: {problem}"))?;
            model::model(replacement)
        }
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

/// The version word of every material the game ships.
const MATERIAL_VERSION: u32 = 0x0103_0000;
/// The shader packages the game's materials use.
const MATERIAL_SHADERS: [&str; 23] = [
    "bg.shpk",
    "bgcolorchange.shpk",
    "bgcrestchange.shpk",
    "bgprop.shpk",
    "bguvscroll.shpk",
    "character.shpk",
    "characterglass.shpk",
    "characterinc.shpk",
    "characterlegacy.shpk",
    "characterocclusion.shpk",
    "characterreflection.shpk",
    "characterscroll.shpk",
    "characterstockings.shpk",
    "charactertattoo.shpk",
    "charactertransparency.shpk",
    "crystal.shpk",
    "hair.shpk",
    "iris.shpk",
    "lightshaft.shpk",
    "river.shpk",
    "skin.shpk",
    "verticalfog.shpk",
    "water.shpk",
];

/// What a material names, for comparing a replacement with its original.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Material {
    shader: Vec<u8>,
}

/// Reads and checks a material: the version, a size that the header
/// states and its sections fill exactly, strings that end inside the
/// string table, texture paths of plain lower-case ASCII, a color table
/// whose size its flags give (the 16-row layout or 8 by 32 values, with a
/// dye table or without), constants inside the value list on 4-byte
/// boundaries, and samplers that name one of the textures or none. Every
/// material the game ships passes these checks.
fn material(bytes: &[u8]) -> Result<Material, String> {
    let short = || "a material ends before its sections do".to_owned();
    if bytes.len() < 16 {
        return Err(short());
    }
    let half = |at: usize| usize::from(u16_at(bytes, at));
    if u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != MATERIAL_VERSION {
        return Err("not a material of the game's version".into());
    }
    if half(4) != bytes.len() {
        return Err(format!(
            "a material states {} bytes and has {}",
            half(4),
            bytes.len()
        ));
    }
    let (data_size, strings_size, shader_name) = (half(6), half(8), half(10));
    let (textures, uv_sets, color_sets, extra) = (
        usize::from(bytes[12]),
        usize::from(bytes[13]),
        usize::from(bytes[14]),
        usize::from(bytes[15]),
    );
    let mut at = 16;
    let names_at = at;
    at += 4 * (textures + uv_sets + color_sets);
    let strings = at;
    at += strings_size;
    let flags_at = at;
    at += extra + data_size;
    if at + 12 > bytes.len() {
        return Err(short());
    }
    let string = |offset: usize| -> Result<&[u8], String> {
        let table = &bytes[strings..strings + strings_size];
        let rest = table
            .get(offset..)
            .filter(|rest| !rest.is_empty())
            .ok_or("a name starts outside the string table")?;
        let end = rest
            .iter()
            .position(|&byte| byte == 0)
            .ok_or("a name does not end inside the string table")?;
        Ok(&rest[..end])
    };
    for index in 0..textures + uv_sets + color_sets {
        let name = string(half(names_at + 4 * index))?;
        let path = index < textures;
        if path
            && (name.is_empty()
                || name.len() >= 260
                || !name
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_uppercase()))
        {
            return Err("a texture path is 1 to 259 lower-case ASCII characters".into());
        }
    }
    let shader = string(shader_name)?.to_vec();
    // The flags are the first bytes of the extra data, as many as there are.
    let flags = (0..extra.min(4)).fold(0usize, |flags, byte| {
        flags | usize::from(bytes[flags_at + byte]) << (8 * byte)
    });
    color_tables(flags, data_size)?;
    let (values, keys, constants, samplers) = (half(at), half(at + 2), half(at + 4), half(at + 6));
    at += 12;
    let constants_at = at + 8 * keys;
    let samplers_at = constants_at + 8 * constants;
    if samplers_at + 12 * samplers + values != bytes.len() {
        return Err("a material's sections do not fill it exactly".into());
    }
    if values % 4 != 0 {
        return Err("shader values are whole 4-byte words".into());
    }
    for constant in 0..constants {
        let (offset, size) = (
            half(constants_at + 8 * constant + 4),
            half(constants_at + 8 * constant + 6),
        );
        if offset % 4 != 0 || size % 4 != 0 || offset + size > values {
            return Err("a constant lies outside the shader values".into());
        }
    }
    for sampler in 0..samplers {
        let texture = usize::from(bytes[samplers_at + 12 * sampler + 8]);
        if texture >= textures && texture != 0xFF {
            return Err(format!("a sampler names texture {texture} of {textures}"));
        }
    }
    Ok(Material { shader })
}

/// Checks that the color tables the flags of a material name take
/// `data_size` bytes: none, or the table of the 16-row layout or of 8
/// values by 32 rows, with its dye table when the flags say so.
fn color_tables(flags: usize, data_size: usize) -> Result<(), String> {
    if data_size == 0 {
        return Ok(());
    }
    let (width, height) = ((flags >> 4) & 0xF, (flags >> 8) & 0xF);
    let (table, dye) = match (width, height) {
        (0, 0) => (512, 32),
        (3, 5) => (2048, 128),
        _ => return Err("a color table is 16 rows or 8 values by 32".into()),
    };
    let expected = if flags & 0x4 == 0 {
        0
    } else {
        table + if flags & 0x8 == 0 { 0 } else { dye }
    };
    if data_size != expected {
        return Err(format!(
            "the color tables the flags name take {expected} bytes, not {data_size}"
        ));
    }
    Ok(())
}

/// A material may change everything it names but its shader package,
/// which stays one the game's materials use, or the original's.
fn check_material(original: &[u8], replacement: &[u8]) -> Result<(), String> {
    let original = material(original).map_err(|problem| format!("the original: {problem}"))?;
    let replacement = material(replacement)?;
    let known = MATERIAL_SHADERS
        .iter()
        .any(|shader| shader.as_bytes() == replacement.shader.as_slice());
    if !known && replacement.shader != original.shader {
        return Err(format!(
            "{} is not a shader package the game's materials use",
            String::from_utf8_lossy(&replacement.shader)
        ));
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

    /// A material with one texture and sampler, the color table of the
    /// current layout with its dye table, and a constant, using `shader`.
    fn material_file(shader: &str) -> Vec<u8> {
        let texture = b"chara/common/texture/skin_d.tex\0";
        let mut strings = texture.to_vec();
        let shader_at = u16::try_from(strings.len()).unwrap();
        strings.extend_from_slice(shader.as_bytes());
        strings.push(0);
        let data = vec![0x11u8; 2048 + 128];
        let values = [0u8; 16];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MATERIAL_VERSION.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]); // size, filled in below
        bytes.extend_from_slice(&u16::try_from(data.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(strings.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&shader_at.to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 0, 4]);
        bytes.extend_from_slice(&[0, 0, 0, 0x80]); // the texture's name at 0
        bytes.extend_from_slice(&strings);
        bytes.extend_from_slice(&0x0000_053Cu32.to_le_bytes()); // a table 8 by 32, with dyes
        bytes.extend_from_slice(&data);
        for value in [16u16, 1, 1, 1, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 0, 0, 0, 2, 0, 0, 0]); // a shader key
        bytes.extend_from_slice(&[9, 0, 0, 0, 4, 0, 8, 0]); // a constant at 4, 8 bytes
        bytes.extend_from_slice(&[7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // a sampler of texture 0
        bytes.extend_from_slice(&values);
        let size = u16::try_from(bytes.len()).unwrap();
        bytes[4..6].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    #[test]
    fn materials_are_whole_and_name_what_they_have() {
        let path = "chara/equipment/e0001/material/v0001/mt_c0101e0001_top_a.mtrl";
        let original = material_file("character.shpk");
        check(path, &original, &original).unwrap();
        check(path, &original, &material_file("skin.shpk")).unwrap();
        let refused = |replacement: &[u8], problem: &str| {
            let error = check(path, &original, replacement).unwrap_err();
            assert!(error.contains(problem), "{error}");
        };
        refused(&material_file("mine.shpk"), "not a shader package");
        let mut version = original.clone();
        version[2] = 4;
        refused(&version, "version");
        refused(&original[..original.len() - 4], "states");
        let mut longer = original.clone();
        longer.extend_from_slice(&[0; 4]);
        let size = u16::try_from(longer.len()).unwrap();
        longer[4..6].copy_from_slice(&size.to_le_bytes());
        refused(&longer, "fill it exactly");
        let mut name = original.clone();
        name[16] = 200;
        refused(&name, "outside the string table");
        let mut upper = original.clone();
        upper[24] = b'C';
        refused(&upper, "lower-case");
        // The header, one name entry, then the two strings.
        let flags = 20 + 32 + 15;
        let mut table = original.clone();
        table[flags] = 0x34; // a table 8 by 32 without dyes, with the dyes' bytes
        refused(&table, "take 2048 bytes, not 2176");
        let mut shape = original.clone();
        shape[flags + 1] = 0x06;
        refused(&shape, "16 rows");
        let constant = original.len() - 16 - 12 - 8 + 4;
        let mut outside = original.clone();
        outside[constant] = 12;
        refused(&outside, "outside the shader values");
        let sampler = original.len() - 16 - 12 + 8;
        let mut texture = original.clone();
        texture[sampler] = 1;
        refused(&texture, "names texture 1 of 1");
        texture[sampler] = 0xFF;
        check(path, &original, &texture).unwrap();
    }

    /// Every material the installed game ships passes the checks and may
    /// replace itself: `ANGELICA_GAME_PATH` is its `game` folder.
    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH"]
    fn every_material_of_the_installed_game_passes() {
        let game = std::path::PathBuf::from(
            std::env::var("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH"),
        );
        let data = angelica_sqpack::GameData::open(game.parent().unwrap()).unwrap();
        let mut checked = 0;
        for category in ["bg", "bgcommon", "chara", "common", "vfx"] {
            data.each_file("ffxiv", category, |file| {
                let Ok(bytes) = file else { return };
                if !bytes.starts_with(&MATERIAL_VERSION.to_le_bytes()) {
                    return;
                }
                checked += 1;
                if let Err(problem) = check_material(&bytes, &bytes) {
                    panic!("{category}: {problem}");
                }
            })
            .unwrap();
        }
        assert!(checked > 50_000, "{checked}");
    }

    /// Every model the installed game ships passes the checks:
    /// `ANGELICA_GAME_PATH` is its `game` folder. The failures are counted
    /// by rule, so one run shows every rule a shipped model breaks.
    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH"]
    fn every_model_of_the_installed_game_passes() {
        let game = std::path::PathBuf::from(
            std::env::var("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH"),
        );
        let data = angelica_sqpack::GameData::open(game.parent().unwrap()).unwrap();
        let mut checked = 0;
        let mut failed: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for category in ["bg", "bgcommon", "chara", "common", "vfx"] {
            data.each_file("ffxiv", category, |file| {
                let Ok(bytes) = file else { return };
                if bytes.len() < 0x44
                    || !matches!(
                        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                        0x0100_0005 | 0x0100_0006
                    )
                {
                    return;
                }
                checked += 1;
                if let Err(problem) = model::model(&bytes) {
                    let rule: String = problem
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .filter(|c| !c.is_ascii_digit())
                        .collect();
                    let entry = failed.entry(format!("{category}: {rule}")).or_default();
                    if *entry < 3 {
                        eprintln!("{category}: {problem}");
                    }
                    *entry += 1;
                }
            })
            .unwrap();
        }
        assert!(failed.is_empty(), "{checked} models: {failed:#?}");
        assert!(checked > 10_000, "{checked}");
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
        let material = material_file("character.shpk");
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
            let _ = check("chara/a.mtrl", &material, &mutator.mutate(&material));
            let _ = check("chara/a.mtrl", &mutator.mutate(&material), &material);
        }
    }
}
