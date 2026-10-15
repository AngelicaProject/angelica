//! `SqPack` data files: file headers and deflate blocks.

use std::fs::File;
use std::io::Read;

/// A standard file: stored as blocks.
const STANDARD: u32 = 2;
/// A model file: its sections' blocks, from which the `.mdl` file is
/// rebuilt.
const MODEL: u32 = 3;
/// A texture file: an uncompressed `.tex` header, then the blocks of each
/// mip level.
const TEXTURE: u32 = 4;
/// The header of a model entry: sizes, offsets, and block counts of its
/// sections.
const MODEL_ENTRY: usize = 0xD0;
/// The header of a rebuilt `.mdl` file.
const MDL_HEADER: usize = 0x44;
/// Most mip levels a texture file describes.
const MAX_LODS: u32 = 16;
/// A block whose data is stored without compression.
const UNCOMPRESSED_BLOCK: u32 = 32_000;
/// Largest file accepted, far above any Excel file.
const MAX_FILE_SIZE: u32 = 64 * 1024 * 1024;
/// Largest block accepted; the game writes blocks of at most 16,000 bytes.
const MAX_BLOCK_SIZE: u32 = 1024 * 1024;

/// Reads `length` bytes at `offset` without moving a shared file position,
/// so several threads can read one file at once.
fn read_exact(file: &File, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0; length];
    let mut filled = 0;
    while filled < length {
        let position = offset + filled as u64;
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(file, &mut buffer[filled..], position);
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(file, &mut buffer[filled..], position);
        match read {
            Ok(0) => return Err(format!("the file ends before offset {position}")),
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(buffer)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

/// Reads the standard or texture file whose header starts at `offset`.
pub(crate) fn read_file(file: &File, offset: u64) -> Result<Vec<u8>, String> {
    // Header: size, type, raw size, two unknown words, block count.
    let header = read_exact(file, offset, 24)?;
    let header_size = u64::from(u32_at(&header, 0));
    let kind = u32_at(&header, 4);
    let raw_size = u32_at(&header, 8);
    let block_count = u32_at(&header, 20);
    // A model entry's header is laid out differently past its first words.
    if kind == MODEL {
        if header_size > 4 * 1024 * 1024 {
            return Err("archive file header exceeds limits".into());
        }
        return read_model(file, offset, header_size);
    }
    if header_size > 4 * 1024 * 1024 || block_count > 131_072 {
        return Err("archive file header exceeds limits".into());
    }
    if raw_size > MAX_FILE_SIZE {
        return Err(format!("the file is {raw_size} bytes"));
    }
    match kind {
        STANDARD => {}
        TEXTURE => return read_texture(file, offset, header_size, raw_size, block_count),
        _ => {
            return Err(format!(
                "file type {kind} is not a standard, model, or texture file"
            ));
        }
    }
    // Every block holds at least one byte, except in an empty file.
    if block_count > raw_size.max(1) {
        return Err(format!("{block_count} blocks for {raw_size} bytes"));
    }
    let block_count = block_count as usize;
    // Block table: offset from the end of the header, compressed and
    // uncompressed sizes.
    let table = read_exact(file, offset + 24, block_count * 8)?;
    let mut data = Vec::with_capacity(raw_size as usize);
    for block in table.as_chunks::<8>().0 {
        let block_offset = offset + header_size + u64::from(u32_at(block, 0));
        read_block(file, block_offset, &mut data)?;
        if data.len() > raw_size as usize {
            return Err(format!(
                "the blocks hold more than the stated {raw_size} bytes"
            ));
        }
    }
    if data.len() != raw_size as usize {
        return Err(format!(
            "the blocks hold {} bytes, the header states {raw_size}",
            data.len()
        ));
    }
    Ok(data)
}

/// Reads a texture file. After the header come, per mip level, the offset
/// of its blocks from the end of the header, its compressed and
/// uncompressed sizes, the index of its first block size, and its block
/// count; then the stored size of every block. The `.tex` header is stored
/// uncompressed before the first level's blocks.
fn read_texture(
    file: &File,
    offset: u64,
    header_size: u64,
    raw_size: u32,
    lod_count: u32,
) -> Result<Vec<u8>, String> {
    if lod_count == 0 || lod_count > MAX_LODS {
        return Err(format!("a texture with {lod_count} mip levels"));
    }
    let lods = read_exact(file, offset + 24, lod_count as usize * 20)?;
    let lods: Vec<[u32; 5]> = lods
        .as_chunks::<20>()
        .0
        .iter()
        .map(|lod| std::array::from_fn(|index| u32_at(lod, index * 4)))
        .collect();
    let block_count: u32 = lods.iter().map(|lod| lod[4]).sum();
    if block_count > raw_size.max(1) {
        return Err(format!("{block_count} blocks for {raw_size} bytes"));
    }
    let sizes = read_exact(
        file,
        offset + 24 + u64::from(lod_count) * 20,
        block_count as usize * 2,
    )?;
    let sizes: Vec<u64> = sizes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|size| u64::from(u16::from_le_bytes(*size)))
        .collect();
    let tex_header = lods[0][0];
    if tex_header > raw_size {
        return Err(format!("a texture header of {tex_header} bytes"));
    }
    let mut data = read_exact(file, offset + header_size, tex_header as usize)?;
    let mut block = offset + header_size + u64::from(tex_header);
    for size in sizes {
        read_block(file, block, &mut data)?;
        if data.len() > raw_size as usize {
            return Err(format!(
                "the blocks hold more than the stated {raw_size} bytes"
            ));
        }
        block += size;
    }
    if data.len() != raw_size as usize {
        return Err(format!(
            "the blocks hold {} bytes, the header states {raw_size}",
            data.len()
        ));
    }
    Ok(data)
}

/// Reads a model file and rebuilds the `.mdl` file the game reads from
/// disk. The entry's header gives, per section (stack, runtime, and the
/// vertex, edge geometry, and index data of each of three levels of
/// detail), its offset from the end of the header and its block count;
/// the stored size of every block follows the header. The rebuilt file is
/// a 0x44-byte header (version, stack and runtime sizes, vertex
/// declaration and material counts, each level's vertex and index offsets
/// and sizes, the level count, and two flags), then the sections in that
/// order.
fn read_model(file: &File, offset: u64, header_size: u64) -> Result<Vec<u8>, String> {
    let entry = read_exact(file, offset, MODEL_ENTRY)?;
    let half = |at: usize| usize::from(u16::from_le_bytes([entry[at], entry[at + 1]]));
    let raw_size = u32_at(&entry, 8);
    if raw_size > MAX_FILE_SIZE {
        return Err(format!("the file is {raw_size} bytes"));
    }
    // Sections in file order: stack, runtime, then vertex, edge, and index
    // data of each level; their offsets and block counts.
    let mut sections: Vec<(u64, usize)> = vec![
        (u64::from(u32_at(&entry, 112)), half(178)),
        (u64::from(u32_at(&entry, 116)), half(180)),
    ];
    for level in 0..3 {
        for (offsets, counts) in [(120, 182), (132, 188), (144, 194)] {
            sections.push((
                u64::from(u32_at(&entry, offsets + 4 * level)),
                half(counts + 2 * level),
            ));
        }
    }
    let block_count: usize = sections.iter().map(|section| section.1).sum();
    if block_count > raw_size.max(1) as usize {
        return Err(format!("{block_count} blocks for {raw_size} bytes"));
    }
    let sizes = read_exact(file, offset + MODEL_ENTRY as u64, block_count * 2)?;
    let mut sizes = sizes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|size| u64::from(u16::from_le_bytes(*size)))
        .collect::<Vec<_>>()
        .into_iter();
    let mut data = vec![0; MDL_HEADER];
    let mut lengths = Vec::with_capacity(sections.len());
    let mut starts = Vec::with_capacity(sections.len());
    for (section, blocks) in &sections {
        let start = data.len();
        let mut block = offset + header_size + section;
        for _ in 0..*blocks {
            read_block(file, block, &mut data)?;
            if data.len() > MDL_HEADER + raw_size as usize {
                return Err(format!(
                    "the blocks hold more than the stated {raw_size} bytes"
                ));
            }
            block += sizes.next().unwrap_or_default();
        }
        starts.push(start);
        lengths.push(data.len() - start);
    }
    let word = |value: usize| u32::try_from(value).unwrap_or(u32::MAX).to_le_bytes();
    let mut header = Vec::with_capacity(MDL_HEADER);
    header.extend_from_slice(&entry[20..24]);
    header.extend_from_slice(&word(lengths[0]));
    header.extend_from_slice(&word(lengths[1]));
    header.extend_from_slice(&entry[200..204]);
    // A level without vertex or index data, or one that shares the
    // previous level's, has offset 0.
    for kind in [0, 2] {
        let mut previous = None;
        for level in 0..3 {
            let section = 2 + 3 * level + kind;
            let start = (sections[section].1 > 0 && previous != Some(starts[section]))
                .then_some(starts[section]);
            if sections[section].1 > 0 {
                previous = Some(starts[section]);
            }
            header.extend_from_slice(&word(start.unwrap_or(0)));
        }
    }
    for kind in [0, 2] {
        for level in 0..3 {
            header.extend_from_slice(&word(lengths[2 + 3 * level + kind]));
        }
    }
    header.extend_from_slice(&entry[204..208]);
    header[MDL_HEADER - 1] = 0;
    data[..MDL_HEADER].copy_from_slice(&header);
    Ok(data)
}

/// Appends one block: a 16-byte header (size, unknown, compressed size or
/// the uncompressed marker, data size) and its data.
fn read_block(file: &File, offset: u64, output: &mut Vec<u8>) -> Result<(), String> {
    let header = read_exact(file, offset, 16)?;
    let stored = u32_at(&header, 8);
    let size = u32_at(&header, 12);
    if size > MAX_BLOCK_SIZE || (stored != UNCOMPRESSED_BLOCK && stored > MAX_BLOCK_SIZE) {
        return Err(format!(
            "a block of {size} bytes stored as {stored} is too large"
        ));
    }
    let size = size as usize;
    if stored == UNCOMPRESSED_BLOCK {
        output.extend_from_slice(&read_exact(file, offset + 16, size)?);
        return Ok(());
    }
    let compressed = read_exact(file, offset + 16, stored as usize)?;
    let start = output.len();
    output.resize(start + size, 0);
    let mut decoder = flate2::read::DeflateDecoder::new(compressed.as_slice());
    decoder
        .read_exact(&mut output[start..])
        .map_err(|error| format!("a block does not inflate to {size} bytes: {error}"))?;
    if decoder
        .read(&mut [0_u8; 1])
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("a block inflates beyond its declared size".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn word(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// A standard file at offset 0 with one stored and one deflated block.
    fn two_block_file() -> Vec<u8> {
        let mut deflated =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        deflated.write_all(b"world").expect("deflate");
        let deflated = deflated.finish().expect("deflate");
        let header_size = 128_u32;
        let mut bytes = Vec::new();
        for value in [header_size, STANDARD, 10, 0, 0, 2] {
            bytes.extend_from_slice(&word(value));
        }
        // Block table: offsets relative to the end of the header.
        bytes.extend_from_slice(&word(0));
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&word(128));
        bytes.extend_from_slice(&[0; 4]);
        bytes.resize(header_size as usize, 0);
        for value in [16, 0, UNCOMPRESSED_BLOCK, 5] {
            bytes.extend_from_slice(&word(value));
        }
        bytes.extend_from_slice(b"hello");
        bytes.resize(header_size as usize + 128, 0);
        let stored = u32::try_from(deflated.len()).expect("small");
        for value in [16, 0, stored, 5] {
            bytes.extend_from_slice(&word(value));
        }
        bytes.extend_from_slice(&deflated);
        bytes
    }

    fn temporary_file(name: &str, bytes: &[u8]) -> (std::path::PathBuf, File) {
        let path = std::env::temp_dir().join(format!("aeria-sqpack-{name}-{}", std::process::id()));
        std::fs::write(&path, bytes).expect("write");
        let file = File::open(&path).expect("open");
        (path, file)
    }

    #[test]
    fn standard_files_join_stored_and_deflated_blocks() {
        let (path, file) = temporary_file("blocks", &two_block_file());
        let data = read_file(&file, 0);
        drop(file);
        std::fs::remove_file(path).expect("remove");
        assert_eq!(data.as_deref(), Ok(b"helloworld".as_slice()));
    }

    /// A texture file at offset 0: a 4-byte `.tex` header, then one mip
    /// level of two stored blocks.
    fn texture_file() -> Vec<u8> {
        let header_size = 128_u32;
        let mut bytes = Vec::new();
        for value in [header_size, TEXTURE, 8, 0, 0, 1] {
            bytes.extend_from_slice(&word(value));
        }
        // The level: blocks after the 4-byte `.tex` header, sizes, first
        // block size index, block count.
        for value in [4, 64, 4, 0, 2] {
            bytes.extend_from_slice(&word(value));
        }
        // Stored block sizes.
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.resize(header_size as usize, 0);
        bytes.extend_from_slice(b"TEX!");
        for part in [b"ab", b"cd"] {
            let start = bytes.len();
            for value in [16, 0, UNCOMPRESSED_BLOCK, 2] {
                bytes.extend_from_slice(&word(value));
            }
            bytes.extend_from_slice(part);
            bytes.resize(start + 32, 0);
        }
        bytes
    }

    #[test]
    fn texture_files_join_their_header_and_level_blocks() {
        let (path, file) = temporary_file("texture", &texture_file());
        let data = read_file(&file, 0);
        drop(file);
        std::fs::remove_file(path).expect("remove");
        assert_eq!(data.as_deref(), Ok(b"TEX!abcd".as_slice()));
    }

    /// A model file at offset 0: a stack block, a runtime block, and one
    /// level's vertex and index blocks, all stored.
    fn model_file() -> Vec<u8> {
        let header_size = 0x100_u32;
        let mut entry = vec![0u8; MODEL_ENTRY];
        entry[..4].copy_from_slice(&word(header_size));
        entry[4..8].copy_from_slice(&word(MODEL));
        entry[8..12].copy_from_slice(&word(10));
        entry[20..24].copy_from_slice(&word(0x0100_0006));
        // Section offsets from the end of the header: stack, runtime,
        // level 0's vertex data, and its index data.
        for (at, value) in [(112, 0u32), (116, 32), (120, 64), (144, 96)] {
            entry[at..at + 4].copy_from_slice(&word(value));
        }
        // One block each.
        for at in [178, 180, 182, 194] {
            entry[at..at + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        entry[200..202].copy_from_slice(&1u16.to_le_bytes());
        entry[202..204].copy_from_slice(&2u16.to_le_bytes());
        entry[204] = 1;
        let mut bytes = entry;
        for _ in 0..4 {
            bytes.extend_from_slice(&32_u16.to_le_bytes());
        }
        bytes.resize(header_size as usize, 0);
        for part in [&b"st"[..], b"run", b"vert", b"i"] {
            let start = bytes.len();
            let size = u32::try_from(part.len()).expect("small");
            for value in [16, 0, UNCOMPRESSED_BLOCK, size] {
                bytes.extend_from_slice(&word(value));
            }
            bytes.extend_from_slice(part);
            bytes.resize(start + 32, 0);
        }
        bytes
    }

    #[test]
    fn model_files_are_rebuilt_with_their_header() {
        let (path, file) = temporary_file("model", &model_file());
        let data = read_file(&file, 0).expect("a model");
        drop(file);
        std::fs::remove_file(path).expect("remove");
        let header = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().expect("four"));
        assert_eq!(&data[MDL_HEADER..], b"strunverti");
        assert_eq!(header(0), 0x0100_0006);
        assert_eq!((header(4), header(8)), (2, 3), "stack and runtime sizes");
        assert_eq!(data[12..16], [1, 0, 2, 0], "declarations and materials");
        let start = u32::try_from(MDL_HEADER).expect("small");
        assert_eq!(header(16), start + 5, "level 0's vertex data");
        assert_eq!(header(28), start + 9, "level 0's index data");
        assert_eq!((header(40), header(52)), (4, 1), "their sizes");
        assert_eq!(data[64], 1, "one level");
    }

    #[test]
    fn a_size_that_disagrees_with_the_blocks_is_rejected() {
        let mut bytes = two_block_file();
        bytes[8..12].copy_from_slice(&word(9));
        let (path, file) = temporary_file("size", &bytes);
        let data = read_file(&file, 0);
        drop(file);
        std::fs::remove_file(path).expect("remove");
        assert!(data.is_err());
    }
}
