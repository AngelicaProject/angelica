use crate::Signature;

const MACHINE_AMD64: u16 = 0x8664;
const MAGIC_PE32_PLUS: u16 = 0x20B;
const COFF_HEADER_LEN: usize = 20;
const SECTION_HEADER_LEN: usize = 40;
const SCN_CNT_CODE: u32 = 0x0000_0020;
const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
/// Offset of `NumberOfRvaAndSizes` in the PE32+ optional header; the data
/// directories follow it.
const DATA_DIRECTORY_COUNT: usize = 108;
const EXCEPTION_DIRECTORY: usize = 3;
const IMPORT_DIRECTORY: usize = 1;
const IMPORT_DESCRIPTOR_LEN: usize = 20;
/// The most functions read from one DLL's import lookup table, so damaged
/// bytes never make a reader walk the whole image.
const MOST_IMPORTS: usize = 65_536;
const RUNTIME_FUNCTION_LEN: usize = 12;

/// How the bytes of an image are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The executable file as stored on disk: sections at their file offsets.
    File,
    /// The module as mapped by Windows: sections at their RVAs.
    Mapped,
}

/// Why bytes are not a supported PE image.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    #[error("the image ends inside its {0}")]
    Truncated(&'static str),
    #[error("the image has no DOS or PE signature")]
    NotPe,
    #[error("the image is for machine {0:#06x}, not x64")]
    UnsupportedMachine(u16),
    #[error("the optional header magic is {0:#06x}, not PE32+")]
    NotPe32Plus(u16),
    #[error("section `{0}` lies outside the image")]
    SectionOutOfBounds(String),
}

/// A section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The name, without trailing NUL bytes.
    pub name: String,
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

impl Section {
    /// Whether the section holds executable code.
    #[must_use]
    pub fn is_code(&self) -> bool {
        self.characteristics & (SCN_CNT_CODE | SCN_MEM_EXECUTE) != 0
    }

    /// The number of bytes of the section that have data in the image: the
    /// virtual size, limited to the raw size in the file layout.
    fn data_len(&self, layout: Layout) -> u32 {
        let virtual_size = if self.virtual_size == 0 {
            self.raw_size
        } else {
            self.virtual_size
        };
        match layout {
            Layout::File => virtual_size.min(self.raw_size),
            Layout::Mapped => virtual_size,
        }
    }

    fn contains(&self, rva: u32, layout: Layout) -> bool {
        rva >= self.virtual_address
            && u64::from(rva) < u64::from(self.virtual_address) + u64::from(self.data_len(layout))
    }
}

/// A 64-bit PE image over borrowed bytes.
#[derive(Debug, Clone)]
pub struct Image<'a> {
    bytes: &'a [u8],
    layout: Layout,
    preferred_base: u64,
    entry: u32,
    mapped_size: u32,
    sections: Vec<Section>,
    /// Begin and end RVAs of the exception directory's entries, by begin,
    /// and the begin of the function each belongs to: its own, or for a
    /// chunk the compiler split off, its chained primary entry's.
    /// Exception directory entries by begin: begin, end, the begin of the
    /// function they belong to, and their unwind information.
    functions: Vec<(u32, u32, u32, u32)>,
    /// The RVA and size of the import directory, when the image has one.
    imports: Option<(u32, u32)>,
}

impl<'a> Image<'a> {
    /// Reads the headers of an x64 PE32+ image.
    ///
    /// # Errors
    ///
    /// Returns an [`ImageError`] when the bytes are not an x64 PE32+ image in
    /// the given layout or a section lies outside them.
    pub fn parse(bytes: &'a [u8], layout: Layout) -> Result<Self, ImageError> {
        if bytes.get(..2) != Some(b"MZ") {
            return Err(ImageError::NotPe);
        }
        let pe = read_u32(bytes, 0x3C).ok_or(ImageError::Truncated("DOS header"))? as usize;
        if bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(ImageError::NotPe);
        }
        let coff = pe + 4;
        let header = bytes
            .get(coff..coff + COFF_HEADER_LEN)
            .ok_or(ImageError::Truncated("COFF header"))?;
        let machine = u16_at(header, 0);
        if machine != MACHINE_AMD64 {
            return Err(ImageError::UnsupportedMachine(machine));
        }
        let section_count = usize::from(u16_at(header, 2));
        let optional_len = usize::from(u16_at(header, 16));

        let optional_start = coff + COFF_HEADER_LEN;
        let optional = bytes
            .get(optional_start..optional_start + optional_len)
            .filter(|optional| optional.len() >= 60)
            .ok_or(ImageError::Truncated("optional header"))?;
        let magic = u16_at(optional, 0);
        if magic != MAGIC_PE32_PLUS {
            return Err(ImageError::NotPe32Plus(magic));
        }
        let entry_point = u32_at(optional, 16);
        let image_base = u64::from(u32_at(optional, 24)) | (u64::from(u32_at(optional, 28)) << 32);
        let size_of_image = u32_at(optional, 56);

        let table_start = optional_start + optional_len;
        let table = bytes
            .get(table_start..table_start + section_count * SECTION_HEADER_LEN)
            .ok_or(ImageError::Truncated("section table"))?;
        let sections = table
            .as_chunks::<SECTION_HEADER_LEN>()
            .0
            .iter()
            .map(parse_section)
            .collect::<Vec<_>>();

        let mut image = Self {
            bytes,
            layout,
            preferred_base: image_base,
            entry: entry_point,
            mapped_size: size_of_image,
            sections,
            functions: Vec::new(),
            imports: data_directory(optional, IMPORT_DIRECTORY).filter(|&(rva, _)| rva != 0),
        };
        for section in &image.sections {
            let (start, len) = image.section_span(section);
            if start + len > bytes.len() {
                return Err(ImageError::SectionOutOfBounds(section.name.clone()));
            }
        }
        if let Some((rva, size)) = data_directory(optional, EXCEPTION_DIRECTORY)
            && size > 0
        {
            let table = image
                .bytes_at(rva, size as usize)
                .ok_or(ImageError::Truncated("exception directory"))?;
            let entries: Vec<(u32, u32, u32)> = table
                .as_chunks::<RUNTIME_FUNCTION_LEN>()
                .0
                .iter()
                .map(|entry| (u32_at(entry, 0), u32_at(entry, 4), u32_at(entry, 8)))
                .filter(|&(begin, end, _)| begin < end)
                .collect();
            let unwinds: std::collections::HashMap<u32, u32> = entries
                .iter()
                .map(|&(begin, _, unwind)| (begin, unwind))
                .collect();
            let mut functions: Vec<(u32, u32, u32, u32)> = entries
                .iter()
                .map(|&(begin, end, unwind)| {
                    (
                        begin,
                        end,
                        image.chain_root(begin, unwind, &unwinds),
                        unwind,
                    )
                })
                .collect();
            functions.sort_unstable();
            image.functions = functions;
        }
        Ok(image)
    }

    /// The RVAs of the import address table slots through which the image
    /// calls `function` of `dll`, imported by name; the names compare
    /// without regard to ASCII case. The loader writes the function's
    /// address into each slot. Empty when the image does not import it by
    /// name, or when the import directory cannot be read: in the mapped
    /// layout, a DLL whose descriptor has no lookup table separate from its
    /// address table, which holds addresses by then.
    #[must_use]
    pub fn import_slots(&self, dll: &str, function: &str) -> Vec<u32> {
        let Some((directory, size)) = self.imports else {
            return Vec::new();
        };
        let mut slots = Vec::new();
        let count = size as usize / IMPORT_DESCRIPTOR_LEN;
        for index in 0..count {
            let at = directory
                .saturating_add(u32::try_from(index * IMPORT_DESCRIPTOR_LEN).unwrap_or(u32::MAX));
            let Some(descriptor) = self.bytes_at(at, IMPORT_DESCRIPTOR_LEN) else {
                break;
            };
            let (lookup, name, addresses) = (
                u32_at(descriptor, 0),
                u32_at(descriptor, 12),
                u32_at(descriptor, 16),
            );
            if name == 0 && addresses == 0 {
                break;
            }
            if !self
                .c_string(name)
                .is_some_and(|found| found.eq_ignore_ascii_case(dll.as_bytes()))
            {
                continue;
            }
            let lookup = match (lookup, self.layout) {
                (0, Layout::Mapped) => continue,
                (0, Layout::File) => addresses,
                (lookup, _) => lookup,
            };
            for entry in 0..MOST_IMPORTS {
                let offset = u32::try_from(entry * 8).unwrap_or(u32::MAX);
                let Some(value) = self.read_u64(lookup.saturating_add(offset)) else {
                    break;
                };
                if value == 0 {
                    break;
                }
                // By ordinal: no name to compare.
                if value & (1 << 63) != 0 {
                    continue;
                }
                let Ok(hint_name) = u32::try_from(value) else {
                    continue;
                };
                if self
                    .c_string(hint_name.saturating_add(2))
                    .is_some_and(|found| found == function.as_bytes())
                {
                    slots.push(addresses.saturating_add(offset));
                }
            }
        }
        slots
    }

    /// The bytes of the NUL-terminated string at `rva`, at most 256 long.
    fn c_string(&self, rva: u32) -> Option<&'a [u8]> {
        let section = self.section_at(rva)?;
        let data = self.section_data(section);
        let start = (rva - section.virtual_address) as usize;
        let rest = data.get(start..)?;
        let end = rest.iter().take(256).position(|&byte| byte == 0)?;
        Some(&rest[..end])
    }

    #[must_use]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The preferred load address from the optional header.
    #[must_use]
    pub fn image_base(&self) -> u64 {
        self.preferred_base
    }

    /// The RVA of the entry point.
    #[must_use]
    pub fn entry_point(&self) -> u32 {
        self.entry
    }

    #[must_use]
    pub fn size_of_image(&self) -> u32 {
        self.mapped_size
    }

    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The first section with this name.
    #[must_use]
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.name == name)
    }

    /// The section whose data contains the RVA.
    #[must_use]
    pub fn section_at(&self, rva: u32) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.contains(rva, self.layout))
    }

    /// The data of a section, starting at its virtual address.
    #[must_use]
    pub fn section_data(&self, section: &Section) -> &'a [u8] {
        let (start, len) = self.section_span(section);
        &self.bytes[start..start + len]
    }

    /// `len` bytes at an RVA, when they all lie in the data of one section.
    #[must_use]
    pub fn bytes_at(&self, rva: u32, len: usize) -> Option<&'a [u8]> {
        let section = self.section_at(rva)?;
        let data = self.section_data(section);
        let offset = (rva - section.virtual_address) as usize;
        data.get(offset..offset.checked_add(len)?)
    }

    #[must_use]
    pub fn read_u32(&self, rva: u32) -> Option<u32> {
        self.bytes_at(rva, 4).map(|bytes| u32_at(bytes, 0))
    }

    #[must_use]
    pub fn read_u64(&self, rva: u32) -> Option<u64> {
        self.bytes_at(rva, 8)
            .map(|bytes| u64::from(u32_at(bytes, 0)) | (u64::from(u32_at(bytes, 4)) << 32))
    }

    /// Whether the RVA lies in the data of a code section.
    #[must_use]
    pub fn is_code(&self, rva: u32) -> bool {
        self.section_at(rva).is_some_and(Section::is_code)
    }

    /// The begin and end RVAs of the exception directory entry that contains
    /// the RVA: for a function start, the function's primary range.
    #[must_use]
    pub fn function_at(&self, rva: u32) -> Option<std::ops::Range<u32>> {
        let index = self.functions.partition_point(|&(begin, ..)| begin <= rva);
        let &(begin, end, ..) = self.functions.get(index.checked_sub(1)?)?;
        (rva < end).then_some(begin..end)
    }

    /// The start of the function whose code holds the RVA: the begin of the
    /// exception directory entry that contains it, or of the function that
    /// entry is chained to.
    #[must_use]
    pub fn function_root(&self, rva: u32) -> Option<u32> {
        let index = self.functions.partition_point(|&(begin, ..)| begin <= rva);
        let &(_, end, root, _) = self.functions.get(index.checked_sub(1)?)?;
        (rva < end).then_some(root)
    }

    /// The length of the prologue of the function that starts at `start`,
    /// from its unwind information. The prologue runs only when the
    /// function is entered, so no code of the function branches into it.
    #[must_use]
    pub fn prologue_len(&self, start: u32) -> Option<u32> {
        const CHAIN_INFO: u8 = 0x4;
        let index = self.functions.partition_point(|&(begin, ..)| begin < start);
        let &(begin, _, root, unwind) = self.functions.get(index)?;
        if begin != start || root != start || unwind == 0 {
            return None;
        }
        let header = self.bytes_at(unwind, 4)?;
        let version = header[0] & 0x7;
        if !(1..=2).contains(&version) || (header[0] >> 3) & CHAIN_INFO != 0 {
            return None;
        }
        Some(u32::from(header[1]))
    }

    /// Every range of code of the function that starts at `start`: its
    /// primary exception directory entry and the chunks chained to it, by
    /// begin. Empty when no entry starts there.
    #[must_use]
    pub fn function_chunks(&self, start: u32) -> Vec<std::ops::Range<u32>> {
        if self
            .function_at(start)
            .is_none_or(|range| range.start != start)
        {
            return Vec::new();
        }
        self.functions
            .iter()
            .filter(|&&(_, _, root, _)| root == start)
            .map(|&(begin, end, ..)| begin..end)
            .collect()
    }

    /// The begin of the function an exception directory entry belongs to:
    /// its own, or, while its unwind information chains to a parent entry,
    /// that entry's, at most 32 links deep. A parent's own unwind
    /// information is taken from the directory, `unwinds` by begin.
    fn chain_root(
        &self,
        begin: u32,
        unwind: u32,
        unwinds: &std::collections::HashMap<u32, u32>,
    ) -> u32 {
        const CHAIN_INFO: u8 = 0x4;
        let (mut root, mut unwind) = (begin, unwind);
        for _ in 0..32 {
            let Some(header) = (unwind != 0).then(|| self.bytes_at(unwind, 4)).flatten() else {
                break;
            };
            if (header[0] >> 3) & CHAIN_INFO == 0 {
                break;
            }
            // The chained entry follows the unwind codes, which are padded
            // to an even count.
            let codes = (usize::from(header[2]) + 1) & !1;
            let Some(parent) = u32::try_from(4 + 2 * codes)
                .ok()
                .and_then(|offset| unwind.checked_add(offset))
                .and_then(|at| self.bytes_at(at, RUNTIME_FUNCTION_LEN))
            else {
                break;
            };
            root = u32_at(parent, 0);
            unwind = unwinds
                .get(&root)
                .copied()
                .unwrap_or_else(|| u32_at(parent, 8));
        }
        root
    }

    /// The begin RVA of the first exception directory entry after the RVA.
    #[must_use]
    pub fn next_function_after(&self, rva: u32) -> Option<u32> {
        let index = self.functions.partition_point(|&(begin, ..)| begin <= rva);
        self.functions.get(index).map(|&(begin, ..)| begin)
    }

    /// The target of a 32-bit relative operand at `rva`, as a call, a jump,
    /// or a RIP-relative access with no bytes after the operand computes it:
    /// the address after the operand plus the signed displacement. `None`
    /// when the operand is not in the image or the target is outside it.
    #[must_use]
    pub fn follow_rel32(&self, rva: u32) -> Option<u32> {
        self.follow_rel32_after(rva, 0)
    }

    /// As [`Image::follow_rel32`], for an instruction that has `trailing`
    /// bytes (an immediate) after the operand.
    #[must_use]
    pub fn follow_rel32_after(&self, rva: u32, trailing: u32) -> Option<u32> {
        #[allow(clippy::cast_possible_wrap)]
        let displacement = i64::from(self.read_u32(rva)? as i32);
        let next = i64::from(rva) + 4 + i64::from(trailing);
        let target = u32::try_from(next + displacement).ok()?;
        (target < self.mapped_size).then_some(target)
    }

    /// The RVAs of every match of a signature in the code sections, in
    /// ascending order.
    #[must_use]
    pub fn scan_code(&self, signature: &Signature) -> Vec<u32> {
        let mut found = Vec::new();
        for section in self.sections.iter().filter(|section| section.is_code()) {
            let data = self.section_data(section);
            // A section's data never exceeds the 32-bit image size, so every
            // offset converts.
            found.extend(signature.find_in(data).into_iter().filter_map(|offset| {
                u32::try_from(offset)
                    .ok()
                    .map(|offset| section.virtual_address + offset)
            }));
        }
        found.sort_unstable();
        found
    }

    fn section_span(&self, section: &Section) -> (usize, usize) {
        let start = match self.layout {
            Layout::File => section.raw_offset,
            Layout::Mapped => section.virtual_address,
        };
        (start as usize, section.data_len(self.layout) as usize)
    }
}

fn parse_section(header: &[u8; SECTION_HEADER_LEN]) -> Section {
    let name_bytes = &header[..8];
    let name_len = name_bytes.iter().position(|&byte| byte == 0).unwrap_or(8);
    Section {
        name: String::from_utf8_lossy(&name_bytes[..name_len]).into_owned(),
        virtual_size: u32_at(header, 8),
        virtual_address: u32_at(header, 12),
        raw_size: u32_at(header, 16),
        raw_offset: u32_at(header, 20),
        characteristics: u32_at(header, 36),
    }
}

/// The RVA and size of a data directory, when the optional header has it.
fn data_directory(optional: &[u8], index: usize) -> Option<(u32, u32)> {
    let count = read_u32(optional, DATA_DIRECTORY_COUNT)? as usize;
    if index >= count {
        return None;
    }
    let offset = DATA_DIRECTORY_COUNT + 4 + index * 8;
    Some((read_u32(optional, offset)?, read_u32(optional, offset + 4)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes.get(offset..offset + 4).map(|bytes| u32_at(bytes, 0))
}

/// A little-endian `u16`; the caller guarantees the bytes exist.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// A little-endian `u32`; the caller guarantees the bytes exist.
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
