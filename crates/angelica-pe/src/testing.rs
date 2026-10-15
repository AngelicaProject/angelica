//! Synthetic PE images for tests.

use crate::{Image, Layout};

const FILE_ALIGNMENT: u32 = 0x200;
const SECTION_ALIGNMENT: u32 = 0x1000;
const OPTIONAL_HEADER_LEN: u16 = 240;
const PE_OFFSET: u32 = 0x80;

/// Section characteristics of a code section (code, execute, read).
pub const CODE: u32 = 0x6000_0020;
/// Section characteristics of a read-only data section.
pub const RDATA: u32 = 0x4000_0040;

/// Builds a minimal x64 PE32+ image with the given sections.
#[derive(Debug, Clone)]
pub struct PeBuilder {
    image_base: u64,
    entry_point: u32,
    sections: Vec<(String, u32, Vec<u8>)>,
    functions: Vec<(u32, u32)>,
    /// Chunks chained to a primary function, by their begins.
    chains: Vec<(u32, u32)>,
}

impl Default for PeBuilder {
    fn default() -> Self {
        Self {
            image_base: 0x1_4000_0000,
            entry_point: SECTION_ALIGNMENT,
            sections: Vec::new(),
            functions: Vec::new(),
            chains: Vec::new(),
        }
    }
}

impl PeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the entry point RVA. The default is the start of the first section.
    #[must_use]
    pub fn entry_point(mut self, rva: u32) -> Self {
        self.entry_point = rva;
        self
    }

    /// Adds a section. Sections are placed in order, each at the next
    /// section-aligned RVA after the headers (the first at `0x1000`).
    ///
    /// # Panics
    ///
    /// Panics when the name is longer than 8 bytes.
    #[must_use]
    pub fn section(mut self, name: &str, characteristics: u32, data: &[u8]) -> Self {
        assert!(name.len() <= 8, "section names have at most 8 bytes");
        self.sections
            .push((name.to_owned(), characteristics, data.to_vec()));
        self
    }

    /// Adds an exception directory with these function ranges (begin and end
    /// RVAs), in a `.pdata` section placed after the other sections.
    #[must_use]
    pub fn functions(mut self, ranges: &[(u32, u32)]) -> Self {
        self.functions = ranges.to_vec();
        self
    }

    /// Makes the function range that begins at `chunk` a chunk chained to the
    /// one that begins at `parent`, through its unwind information.
    #[must_use]
    pub fn chained(mut self, chunk: u32, parent: u32) -> Self {
        self.chains.push((chunk, parent));
        self
    }

    /// The RVA of the `.pdata` section, after the other sections.
    fn pdata_rva(&self) -> u32 {
        SECTION_ALIGNMENT
            + self
                .sections
                .iter()
                .map(|(_, _, data)| {
                    align(
                        u32::try_from(data.len())
                            .expect("test sections are small")
                            .max(1),
                        SECTION_ALIGNMENT,
                    )
                })
                .sum::<u32>()
    }

    /// The RVA the section at `index` will have.
    #[must_use]
    pub fn section_rva(&self, index: usize) -> u32 {
        self.layout().0[index].0
    }

    /// The image as a file on disk.
    #[must_use]
    pub fn build_file(&self) -> Vec<u8> {
        let (placed, size_of_image, headers_len) = self.layout();
        let file_len = placed
            .last()
            .map_or(headers_len, |&(_, raw_offset, raw_size)| {
                raw_offset + raw_size
            });
        let mut bytes = vec![0; file_len as usize];
        self.write_headers(&mut bytes, &placed, size_of_image);
        for ((_, _, data), &(_, raw_offset, _)) in self.all_sections().iter().zip(&placed) {
            bytes[raw_offset as usize..raw_offset as usize + data.len()].copy_from_slice(data);
        }
        bytes
    }

    /// The image as Windows maps it.
    #[must_use]
    pub fn build_mapped(&self) -> Vec<u8> {
        map_file(&self.build_file())
    }

    /// The sections, with the `.pdata` section when there are functions.
    fn all_sections(&self) -> Vec<(String, u32, Vec<u8>)> {
        let mut sections = self.sections.clone();
        if !self.functions.is_empty() {
            // The table, then the unwind information of chained chunks:
            // version 1 with the chain flag, no codes, and the parent entry.
            let table_len = 12 * u32::try_from(self.functions.len()).expect("few functions");
            let mut unwind = Vec::new();
            let mut data = Vec::new();
            for &(begin, end) in &self.functions {
                let info = match self.chains.iter().find(|&&(chunk, _)| chunk == begin) {
                    Some(&(_, parent)) => {
                        let &(parent_begin, parent_end) = self
                            .functions
                            .iter()
                            .find(|&&(start, _)| start == parent)
                            .expect("the parent is a function");
                        let at = self.pdata_rva()
                            + table_len
                            + u32::try_from(unwind.len()).expect("small");
                        unwind.extend([0x21, 0, 0, 0]);
                        for word in [parent_begin, parent_end, 0] {
                            unwind.extend(word.to_le_bytes());
                        }
                        at
                    }
                    None => 0,
                };
                for word in [begin, end, info] {
                    data.extend(word.to_le_bytes());
                }
            }
            data.extend(unwind);
            sections.push((".pdata".to_owned(), RDATA, data));
        }
        sections
    }

    /// RVA, file offset, and raw size of each section; the image size; and
    /// the aligned length of the headers.
    fn layout(&self) -> (Vec<(u32, u32, u32)>, u32, u32) {
        let section_table_end = PE_OFFSET
            + 4
            + 20
            + u32::from(OPTIONAL_HEADER_LEN)
            + 40 * u32::try_from(self.all_sections().len()).expect("few sections");
        let headers_len = align(section_table_end, FILE_ALIGNMENT);
        let mut rva = SECTION_ALIGNMENT;
        let mut raw_offset = headers_len;
        let mut placed = Vec::new();
        for (_, _, data) in &self.all_sections() {
            let len = u32::try_from(data.len()).expect("test sections are small");
            let raw_size = align(len.max(1), FILE_ALIGNMENT);
            placed.push((rva, raw_offset, raw_size));
            rva += align(len.max(1), SECTION_ALIGNMENT);
            raw_offset += raw_size;
        }
        (placed, rva, headers_len)
    }

    fn write_headers(&self, bytes: &mut [u8], placed: &[(u32, u32, u32)], size_of_image: u32) {
        bytes[..2].copy_from_slice(b"MZ");
        put32(bytes, 0x3C, PE_OFFSET);
        let pe = PE_OFFSET as usize;
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        let coff = pe + 4;
        put16(bytes, coff, 0x8664);
        put16(
            bytes,
            coff + 2,
            u16::try_from(self.all_sections().len()).expect("few sections"),
        );
        put16(bytes, coff + 16, OPTIONAL_HEADER_LEN);
        put16(bytes, coff + 18, 0x0022);
        let optional = coff + 20;
        put16(bytes, optional, 0x20B);
        put32(bytes, optional + 16, self.entry_point);
        bytes[optional + 24..optional + 32].copy_from_slice(&self.image_base.to_le_bytes());
        put32(bytes, optional + 32, SECTION_ALIGNMENT);
        put32(bytes, optional + 36, FILE_ALIGNMENT);
        put32(bytes, optional + 56, size_of_image);
        put32(
            bytes,
            optional + 60,
            placed
                .first()
                .map_or(FILE_ALIGNMENT, |&(_, raw_offset, _)| raw_offset),
        );
        put32(bytes, optional + 108, 16);
        if let (false, Some(&(rva, _, _))) = (self.functions.is_empty(), placed.last()) {
            put32(bytes, optional + 112 + 24, rva);
            put32(
                bytes,
                optional + 112 + 28,
                12 * u32::try_from(self.functions.len()).expect("few functions"),
            );
        }
        let table = optional + usize::from(OPTIONAL_HEADER_LEN);
        for (index, ((name, characteristics, data), &(rva, raw_offset, raw_size))) in
            self.all_sections().iter().zip(placed).enumerate()
        {
            let header = table + index * 40;
            bytes[header..header + name.len()].copy_from_slice(name.as_bytes());
            put32(
                bytes,
                header + 8,
                u32::try_from(data.len()).expect("test sections are small"),
            );
            put32(bytes, header + 12, rva);
            put32(bytes, header + 16, raw_size);
            put32(bytes, header + 20, raw_offset);
            put32(bytes, header + 36, *characteristics);
        }
    }
}

/// Lays out a PE file as Windows maps it: the headers at the start, each
/// section's data at its RVA, and zeros elsewhere up to the image size.
///
/// # Panics
///
/// Panics when the file is not a valid image.
#[must_use]
pub fn map_file(file: &[u8]) -> Vec<u8> {
    let image = Image::parse(file, Layout::File).expect("a valid PE file");
    let mut mapped = vec![0; image.size_of_image() as usize];
    let headers_len = image
        .sections()
        .iter()
        .map(|section| section.raw_offset as usize)
        .filter(|&offset| offset > 0)
        .min()
        .unwrap_or(file.len());
    mapped[..headers_len].copy_from_slice(&file[..headers_len]);
    for section in image.sections() {
        let data = image.section_data(section);
        let start = section.virtual_address as usize;
        mapped[start..start + data.len()].copy_from_slice(data);
    }
    mapped
}

fn align(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
