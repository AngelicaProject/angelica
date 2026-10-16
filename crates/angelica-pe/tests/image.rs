use angelica_pe::testing::{CODE, PeBuilder, RDATA};
use angelica_pe::{Image, ImageError, Layout, Signature};

/// A code section with a call to the data section and a repeated pattern,
/// and a data section that contains the same pattern.
fn sample() -> PeBuilder {
    let mut code = vec![0xCC; 0x40];
    // At 0x10: call rel32 to the start of the data section (RVA 0x2000).
    code[0x10] = 0xE8;
    let target = 0x2000_i64 - (0x1000 + 0x11 + 4);
    code[0x11..0x15].copy_from_slice(&i32::try_from(target).unwrap().to_le_bytes());
    code[0x20..0x24].copy_from_slice(&[0x48, 0x8B, 0x05, 0x99]);
    code[0x30..0x34].copy_from_slice(&[0x48, 0x8B, 0x0D, 0x99]);
    PeBuilder::new()
        .entry_point(0x1000)
        .section(".text", CODE, &code)
        .section(".rdata", RDATA, &[0x48, 0x8B, 0x05, 0x99])
}

fn both_layouts(builder: &PeBuilder, check: impl Fn(&Image<'_>)) {
    let file = builder.build_file();
    let mapped = builder.build_mapped();
    check(&Image::parse(&file, Layout::File).unwrap());
    check(&Image::parse(&mapped, Layout::Mapped).unwrap());
}

#[test]
fn reads_headers_and_sections() {
    both_layouts(&sample(), |image| {
        assert_eq!(image.image_base(), 0x1_4000_0000);
        assert_eq!(image.entry_point(), 0x1000);
        assert_eq!(image.size_of_image(), 0x3000);
        let names = image
            .sections()
            .iter()
            .map(|section| section.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, [".text", ".rdata"]);
        assert!(image.section(".text").unwrap().is_code());
        assert!(!image.section(".rdata").unwrap().is_code());
        assert_eq!(image.section_at(image.entry_point()).unwrap().name, ".text");
    });
}

#[test]
fn answers_in_rvas_in_both_layouts() {
    both_layouts(&sample(), |image| {
        assert_eq!(
            image.bytes_at(0x1020, 4),
            Some(&[0x48, 0x8B, 0x05, 0x99][..])
        );
        assert_eq!(
            image.bytes_at(0x2000, 4),
            Some(&[0x48, 0x8B, 0x05, 0x99][..])
        );
        // Past the data of the last section.
        assert_eq!(image.bytes_at(0x2002, 4), None);
        assert_eq!(image.bytes_at(0x0FFF, 1), None);
    });
}

#[test]
fn follows_relative_operands() {
    both_layouts(&sample(), |image| {
        assert_eq!(image.follow_rel32(0x1011), Some(0x2000));
        // A displacement that points outside the image.
        assert_eq!(image.follow_rel32(0x1000), None);
    });
}

#[test]
fn scans_only_code_sections() {
    let signature = Signature::parse("48 8B ?? 99").unwrap();
    both_layouts(&sample(), |image| {
        assert_eq!(image.scan_code(&signature), vec![0x1020, 0x1030]);
    });
}

#[test]
fn refuses_what_is_not_an_x64_pe32_plus_image() {
    let file = sample().build_file();
    assert_eq!(
        Image::parse(b"", Layout::File).unwrap_err(),
        ImageError::NotPe
    );
    assert_eq!(
        Image::parse(&file[..0x3E], Layout::File).unwrap_err(),
        ImageError::Truncated("DOS header")
    );

    let mut no_pe = file.clone();
    no_pe[0x80] = b'X';
    assert_eq!(
        Image::parse(&no_pe, Layout::File).unwrap_err(),
        ImageError::NotPe
    );

    let mut x86 = file.clone();
    x86[0x84..0x86].copy_from_slice(&0x014C_u16.to_le_bytes());
    assert_eq!(
        Image::parse(&x86, Layout::File).unwrap_err(),
        ImageError::UnsupportedMachine(0x014C)
    );

    let mut pe32 = file.clone();
    pe32[0x98..0x9A].copy_from_slice(&0x010B_u16.to_le_bytes());
    assert_eq!(
        Image::parse(&pe32, Layout::File).unwrap_err(),
        ImageError::NotPe32Plus(0x010B)
    );

    assert_eq!(
        Image::parse(&file[..0x100], Layout::File).unwrap_err(),
        ImageError::Truncated("optional header")
    );
    assert_eq!(
        Image::parse(&file[..0x190], Layout::File).unwrap_err(),
        ImageError::Truncated("section table")
    );
}

#[test]
fn refuses_a_section_outside_the_bytes() {
    let file = sample().build_file();
    let truncated = &file[..0x402];
    assert_eq!(
        Image::parse(truncated, Layout::File).unwrap_err(),
        ImageError::SectionOutOfBounds(".rdata".to_owned())
    );
    // The file layout is shorter than the mapped image.
    assert_eq!(
        Image::parse(&file, Layout::Mapped).unwrap_err(),
        ImageError::SectionOutOfBounds(".text".to_owned())
    );
}

#[test]
fn reads_function_ranges_from_the_exception_directory() {
    let builder = sample().functions(&[(0x1020, 0x1030), (0x1000, 0x1010)]);
    both_layouts(&builder, |image| {
        assert_eq!(
            image.section(".pdata").map(angelica_pe::Section::is_code),
            Some(false)
        );
        assert_eq!(image.function_at(0x1000), Some(0x1000..0x1010));
        assert_eq!(image.function_at(0x102F), Some(0x1020..0x1030));
        assert_eq!(image.function_at(0x1010), None);
        assert_eq!(image.function_at(0x0FFF), None);
        assert!(image.is_code(0x1000));
        assert!(!image.is_code(0x2000));
    });
}

#[test]
fn a_function_has_the_chunks_chained_to_it() {
    let builder = sample()
        .functions(&[
            (0x1000, 0x1008),
            (0x1008, 0x1010),
            (0x1020, 0x1030),
            (0x1010, 0x1018),
        ])
        .chained(0x1008, 0x1000)
        .chained(0x1010, 0x1008);
    both_layouts(&builder, |image| {
        assert_eq!(
            image.function_chunks(0x1000),
            [0x1000..0x1008, 0x1008..0x1010, 0x1010..0x1018],
            "a chunk chained through another belongs to the same function"
        );
        assert_eq!(image.function_chunks(0x1020), vec![0x1020_u32..0x1030]);
        assert!(
            image.function_chunks(0x1004).is_empty(),
            "not a function's start"
        );
        assert_eq!(image.function_at(0x1009), Some(0x1008..0x1010));
    });
}

#[test]
fn reads_little_endian_words() {
    both_layouts(&sample(), |image| {
        assert_eq!(image.read_u32(0x2000), Some(0x9905_8B48));
        assert_eq!(image.read_u64(0x1020), Some(0xCCCC_CCCC_9905_8B48));
        assert_eq!(image.read_u64(0x2000), None);
    });
}

#[test]
fn a_function_has_the_prologue_its_unwind_information_gives() {
    let builder = sample()
        .functions(&[(0x1000, 0x1008), (0x1008, 0x1010), (0x1020, 0x1030)])
        .chained(0x1008, 0x1000)
        .prologue(0x1000, 6);
    both_layouts(&builder, |image| {
        assert_eq!(image.prologue_len(0x1000), Some(6));
        assert_eq!(image.prologue_len(0x1008), None, "a chained chunk");
        assert_eq!(image.prologue_len(0x1020), None, "no unwind information");
        assert_eq!(image.prologue_len(0x1004), None, "not a function's start");
        assert_eq!(image.function_root(0x100C), Some(0x1000));
        assert_eq!(image.function_root(0x1024), Some(0x1020));
        assert_eq!(image.function_root(0x1018), None);
    });
}

/// An `.idata` section at RVA 0x1000 that imports `GetTickCount` from
/// `KERNEL32.dll`, and `ShowCursor`, ordinal 7, and `SetCursor` from
/// `USER32.dll`: the descriptors, then each DLL's lookup and address tables,
/// then the hint and name entries and the DLL names. `lookup` gives the
/// descriptors their lookup tables; without, they name the address tables
/// only, as old linkers did.
fn imports(lookup: bool) -> PeBuilder {
    const BASE: u32 = 0x1000;
    let mut data = vec![0u8; 0x200];
    let put32 = |data: &mut Vec<u8>, at: usize, value: u32| {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let put64 = |data: &mut Vec<u8>, at: usize, value: u64| {
        data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    };
    let mut strings = 0x100;
    let mut string = |data: &mut Vec<u8>, text: &[u8], hint: bool| {
        let at = strings;
        let start = if hint { at + 2 } else { at };
        data[start..start + text.len()].copy_from_slice(text);
        strings = start + text.len() + 1;
        BASE + u32::try_from(at).unwrap()
    };
    // KERNEL32: lookup table at 0x40, address table at 0x50.
    let tick = string(&mut data, b"GetTickCount", true);
    // USER32: lookup table at 0x60, address table at 0x80.
    let show = string(&mut data, b"ShowCursor", true);
    let set = string(&mut data, b"SetCursor", true);
    let kernel = string(&mut data, b"KERNEL32.dll", false);
    let user = string(&mut data, b"USER32.dll", false);
    for (table, entries) in [
        (0x40, vec![u64::from(tick)]),
        (0x50, vec![u64::from(tick)]),
        (0x60, vec![u64::from(show), (1 << 63) | 7, u64::from(set)]),
        (0x80, vec![u64::from(show), (1 << 63) | 7, u64::from(set)]),
    ] {
        for (index, entry) in entries.into_iter().enumerate() {
            put64(&mut data, table + index * 8, entry);
        }
    }
    for (index, (lookup_at, name, addresses)) in [(0x40, kernel, 0x50), (0x60, user, 0x80)]
        .into_iter()
        .enumerate()
    {
        let at = index * 20;
        put32(&mut data, at, if lookup { BASE + lookup_at } else { 0 });
        put32(&mut data, at + 12, name);
        put32(&mut data, at + 16, BASE + addresses);
    }
    PeBuilder::new()
        .section(".idata", RDATA, &data)
        .directory(1, BASE, 3 * 20)
}

#[test]
fn import_slots_are_found_by_dll_and_function_name() {
    both_layouts(&imports(true), |image| {
        assert_eq!(
            image.import_slots("user32.dll", "SetCursor"),
            vec![0x1000 + 0x80 + 16]
        );
        assert_eq!(
            image.import_slots("USER32.DLL", "ShowCursor"),
            vec![0x1000 + 0x80]
        );
        assert_eq!(
            image.import_slots("KERNEL32.dll", "GetTickCount"),
            vec![0x1000 + 0x50]
        );
        // Names of functions compare exactly; the DLL must match too.
        assert!(image.import_slots("user32.dll", "setcursor").is_empty());
        assert!(image.import_slots("kernel32.dll", "SetCursor").is_empty());
        assert!(image.import_slots("gdi32.dll", "SetCursor").is_empty());
    });
}

#[test]
fn without_a_lookup_table_only_the_file_names_its_imports() {
    let builder = imports(false);
    let file = builder.build_file();
    let image = Image::parse(&file, Layout::File).unwrap();
    assert_eq!(
        image.import_slots("USER32.dll", "SetCursor"),
        vec![0x1000 + 0x80 + 16]
    );
    // Mapped, the address table holds addresses, not names.
    let mapped = builder.build_mapped();
    let image = Image::parse(&mapped, Layout::Mapped).unwrap();
    assert!(image.import_slots("USER32.dll", "SetCursor").is_empty());
}

#[test]
fn a_damaged_import_directory_finds_nothing_and_does_not_panic() {
    let file = imports(true).build_file();
    let offset = Image::parse(&file, Layout::File)
        .unwrap()
        .section(".idata")
        .unwrap()
        .raw_offset as usize;
    let mut seed = 0x1d67_07e3_u64;
    for _ in 0..20_000 {
        let mut damaged = file.clone();
        for _ in 0..4 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let at = offset + usize::try_from(seed % 0x200).unwrap();
            damaged[at] = u8::try_from(seed >> 56).unwrap();
        }
        let image = Image::parse(&damaged, Layout::File).unwrap();
        let _ = image.import_slots("USER32.dll", "SetCursor");
    }
}
