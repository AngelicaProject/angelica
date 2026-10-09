//! The game's icons for plugin windows: an icon number names a texture in
//! the game's archives, which is decoded to RGBA pixels.
#![forbid(unsafe_code)]
use crate::{error::Kind, game_data::GameData};

/// The size of a texture file's header.
const HEADER: usize = 80;
/// The largest icon side the host decodes.
pub const MAX_SIDE: usize = 512;

/// An icon's pixels: rows of RGBA bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// The game paths of an icon, the high-resolution one first.
#[must_use]
pub fn paths(icon: u32) -> [String; 2] {
    let folder = icon / 1000 * 1000;
    [
        format!("ui/icon/{folder:06}/{icon:06}_hr1.tex"),
        format!("ui/icon/{folder:06}/{icon:06}.tex"),
    ]
}

/// Reads an icon from the game's archives, the high-resolution one if the
/// game has it; `None` when the game has neither.
///
/// # Errors
/// Names an unreadable archive or a texture [`decode`] refuses.
pub fn load(data: &GameData, icon: u32) -> Result<Option<Image>, String> {
    for path in paths(icon) {
        match data.read(&path) {
            Ok(file) => return decode(&file).map(Some).map_err(|e| format!("{path}: {e}")),
            Err(error) if error.kind == Kind::NotFound => {}
            Err(error) => return Err(error.message),
        }
    }
    Ok(None)
}

/// Decodes the first mip level of a texture file.
///
/// # Errors
/// Names a truncated file, a side over [`MAX_SIDE`], or a pixel format the
/// host does not decode.
pub fn decode(file: &[u8]) -> Result<Image, String> {
    if file.len() < HEADER {
        return Err("the texture is shorter than its header".into());
    }
    let format = u32::from_le_bytes([file[4], file[5], file[6], file[7]]);
    let width = usize::from(u16::from_le_bytes([file[8], file[9]]));
    let height = usize::from(u16::from_le_bytes([file[10], file[11]]));
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("the texture is {width}x{height}"));
    }
    let pixels = &file[HEADER..];
    let rgba = match format {
        // B4G4R4A4, B5G5R5A1, B8G8R8A8, B8G8R8X8.
        0x1440 => sixteen_bit(pixels, width * height, |value| {
            let channel = |shift: u16| ((value >> shift) & 0xF) as u8 * 17;
            [channel(8), channel(4), channel(0), channel(12)]
        }),
        0x1441 => sixteen_bit(pixels, width * height, |value| {
            let channel = |shift: u16| {
                let level = ((value >> shift) & 0x1F) as u8;
                (level << 3) | (level >> 2)
            };
            [
                channel(10),
                channel(5),
                channel(0),
                if value & 0x8000 == 0 { 0 } else { 255 },
            ]
        }),
        0x1450 | 0x1451 => {
            let size = width * height * 4;
            let source = pixels.get(..size).ok_or("the texture is truncated")?;
            Ok(source
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|bgra| {
                    let alpha = if format == 0x1451 { 255 } else { bgra[3] };
                    [bgra[2], bgra[1], bgra[0], alpha]
                })
                .collect())
        }
        // BC1, BC2, and BC3 (DXT1, DXT3, DXT5).
        0x3420 => blocks(pixels, width, height, 8, |block, out| {
            color_block(block, out, true);
        }),
        0x3430 => blocks(pixels, width, height, 16, |block, out| {
            color_block(&block[8..], out, false);
            for (index, pixel) in out.iter_mut().enumerate() {
                let nibble = (block[index / 2] >> ((index % 2) * 4)) & 0xF;
                pixel[3] = nibble * 17;
            }
        }),
        0x3431 => blocks(pixels, width, height, 16, |block, out| {
            color_block(&block[8..], out, false);
            alpha_block(&block[..8], out);
        }),
        other => Err(format!("the texture format {other:#06x} is not decoded")),
    }?;
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn sixteen_bit(
    pixels: &[u8],
    count: usize,
    color: impl Fn(u16) -> [u8; 4],
) -> Result<Vec<u8>, String> {
    let source = pixels.get(..count * 2).ok_or("the texture is truncated")?;
    Ok(source
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|bytes| color(u16::from_le_bytes([bytes[0], bytes[1]])))
        .collect())
}

/// Decodes 4x4 blocks of `size` bytes into RGBA rows.
fn blocks(
    pixels: &[u8],
    width: usize,
    height: usize,
    size: usize,
    decode: impl Fn(&[u8], &mut [[u8; 4]; 16]),
) -> Result<Vec<u8>, String> {
    let across = width.div_ceil(4);
    let down = height.div_ceil(4);
    let source = pixels
        .get(..across * down * size)
        .ok_or("the texture is truncated")?;
    let mut rgba = vec![0; width * height * 4];
    for (index, block) in source.chunks_exact(size).enumerate() {
        let mut out = [[0u8; 4]; 16];
        decode(block, &mut out);
        let (bx, by) = (index % across * 4, index / across * 4);
        for (pixel, color) in out.iter().enumerate() {
            let (x, y) = (bx + pixel % 4, by + pixel / 4);
            if x < width && y < height {
                let at = (y * width + x) * 4;
                rgba[at..at + 4].copy_from_slice(color);
            }
        }
    }
    Ok(rgba)
}

fn rgb565(value: u16) -> [u8; 3] {
    let red = ((value >> 11) & 0x1F) as u8;
    let green = ((value >> 5) & 0x3F) as u8;
    let blue = (value & 0x1F) as u8;
    [
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
    ]
}

/// A BC1 color block; `punch_through` allows its transparent mode.
fn color_block(block: &[u8], out: &mut [[u8; 4]; 16], punch_through: bool) {
    let first = u16::from_le_bytes([block[0], block[1]]);
    let second = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(first), rgb565(second));
    let mix = |x: u8, y: u8, wx: u16, wy: u16| {
        u8::try_from((u16::from(x) * wx + u16::from(y) * wy) / (wx + wy)).unwrap_or(u8::MAX)
    };
    let mut palette = [
        [a[0], a[1], a[2], 255],
        [b[0], b[1], b[2], 255],
        [0; 4],
        [0; 4],
    ];
    if first > second || !punch_through {
        for channel in 0..3 {
            palette[2][channel] = mix(a[channel], b[channel], 2, 1);
            palette[3][channel] = mix(a[channel], b[channel], 1, 2);
        }
        palette[2][3] = 255;
        palette[3][3] = 255;
    } else {
        for channel in 0..3 {
            palette[2][channel] = mix(a[channel], b[channel], 1, 1);
        }
        palette[2][3] = 255;
    }
    let indexes = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    for (pixel, color) in out.iter_mut().enumerate() {
        *color = palette[((indexes >> (pixel * 2)) & 3) as usize];
    }
}

/// A BC3 alpha block.
fn alpha_block(block: &[u8], out: &mut [[u8; 4]; 16]) {
    let (a0, a1) = (u16::from(block[0]), u16::from(block[1]));
    let mut alphas = [a0, a1, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for step in 1..7u16 {
            alphas[usize::from(step) + 1] = (a0 * (7 - step) + a1 * step) / 7;
        }
    } else {
        for step in 1..5u16 {
            alphas[usize::from(step) + 1] = (a0 * (5 - step) + a1 * step) / 5;
        }
        alphas[7] = 255;
    }
    let mut bits = 0u64;
    for (index, &byte) in block[2..8].iter().enumerate() {
        bits |= u64::from(byte) << (index * 8);
    }
    for (pixel, color) in out.iter_mut().enumerate() {
        let alpha = alphas[((bits >> (pixel * 3)) & 7) as usize];
        color[3] = u8::try_from(alpha).unwrap_or(u8::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(format: u32, width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
        let mut file = vec![0; HEADER];
        file[4..8].copy_from_slice(&format.to_le_bytes());
        file[8..10].copy_from_slice(&width.to_le_bytes());
        file[10..12].copy_from_slice(&height.to_le_bytes());
        file.extend_from_slice(pixels);
        file
    }

    #[test]
    fn icon_paths_put_high_resolution_first() {
        assert_eq!(
            paths(20001),
            ["ui/icon/020000/020001_hr1.tex", "ui/icon/020000/020001.tex"]
        );
    }

    #[test]
    fn uncompressed_formats_become_rgba() {
        let bgra = decode(&texture(0x1450, 1, 1, &[1, 2, 3, 4])).unwrap();
        assert_eq!(bgra.rgba, [3, 2, 1, 4]);
        // Opaque pure red in B5G5R5A1.
        let red = decode(&texture(0x1441, 1, 1, &0xFC00u16.to_le_bytes())).unwrap();
        assert_eq!(red.rgba, [255, 0, 0, 255]);
        // Half-transparent green in B4G4R4A4.
        let green = decode(&texture(0x1440, 1, 1, &0x80F0u16.to_le_bytes())).unwrap();
        assert_eq!(green.rgba, [0, 255, 0, 136]);
    }

    #[test]
    fn compressed_blocks_fill_the_image() {
        // One BC1 block: endpoints white and black, every pixel index 0.
        let block = [0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
        let image = decode(&texture(0x3420, 4, 4, &block)).unwrap();
        assert_eq!(image.rgba.len(), 64);
        assert!(
            image
                .rgba
                .chunks(4)
                .all(|pixel| pixel == [255, 255, 255, 255])
        );
        // A 2x2 image still reads one whole block.
        assert_eq!(
            decode(&texture(0x3420, 2, 2, &block)).unwrap().rgba.len(),
            16
        );
        // BC3 with alpha endpoints 255 and 0, every alpha index 1.
        let mut bc3 = [0u8; 16];
        bc3[0] = 255;
        bc3[2..8].copy_from_slice(&[0x49, 0x92, 0x24, 0x49, 0x92, 0x24]);
        bc3[8..10].copy_from_slice(&[0xFF, 0xFF]);
        let image = decode(&texture(0x3431, 4, 4, &bc3)).unwrap();
        assert!(image.rgba.chunks(4).all(|pixel| pixel[3] == 0));
    }

    #[test]
    fn icons_need_the_game_files() {
        assert!(load(&GameData::new(None), 20001).is_err());
    }

    /// The installed game: `ANGELICA_GAME_PATH` is its `game` folder.
    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH"]
    fn loads_the_installed_games_icons() {
        let game = std::path::PathBuf::from(
            std::env::var("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH"),
        );
        let data = GameData::new(game.parent());
        // Gil, the icon of `Item` row 1.
        let gil = load(&data, 65002).unwrap().unwrap();
        assert_eq!(gil.rgba.len(), gil.width * gil.height * 4);
        assert!(gil.rgba.chunks(4).any(|pixel| pixel[3] > 0));
        assert_eq!(load(&data, 999_999).unwrap(), None);
    }

    #[test]
    fn broken_textures_are_refused() {
        assert!(decode(&[0; 10]).is_err());
        assert!(
            decode(&texture(0x1450, 2, 2, &[0; 4])).is_err(),
            "truncated"
        );
        assert!(decode(&texture(0x1450, 0, 2, &[])).is_err());
        assert!(decode(&texture(0x1450, 4096, 4096, &[])).is_err());
        assert!(decode(&texture(0x9999, 1, 1, &[0; 4])).is_err());
    }
}
