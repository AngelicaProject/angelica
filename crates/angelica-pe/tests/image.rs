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
