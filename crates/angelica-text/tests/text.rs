use angelica_text::{
    guard, sestring,
    table::{ANY_LANGUAGE, Column, Table, Variant, Writer},
};

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn well_formed_rule_matches_the_shared_vectors() {
    let vectors = include_str!("well_formed.vectors.txt");
    let mut checked = 0;
    for line in vectors
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut parts = line.split('\t');
        let expected = parts.next().unwrap() == "yes";
        let name = parts.next().unwrap();
        let bytes = hex(parts.next().unwrap_or(""));
        assert_eq!(sestring::is_well_formed(&bytes), expected, "{name}");
        checked += 1;
    }
    assert!(checked > 50);
    assert!(sestring::is_cell_text(b""));
}

fn sample() -> Vec<u8> {
    let columns = [
        Column {
            index: 3,
            offset: 8,
        },
        Column {
            index: 0,
            offset: 0,
        },
    ];
    let mut writer = Writer::new(2, b"DOMAIN").unwrap();
    writer.sheet("Item", Variant::Rows, &columns).unwrap();
    writer
        .sheet(
            "quest/Text",
            Variant::Subrows,
            &[Column {
                index: 1,
                offset: 4,
            }],
        )
        .unwrap();
    writer
        .sheet(
            "Empty",
            Variant::Rows,
            &[Column {
                index: 0,
                offset: 0,
            }],
        )
        .unwrap();
    writer
        .cell(
            "Item",
            7,
            0,
            3,
            guard(b"DOMAIN", b"Potion"),
            "Зелье".as_bytes(),
        )
        .unwrap();
    writer
        .cell(
            "Item",
            2,
            0,
            0,
            guard(b"DOMAIN", b"Ether"),
            "Эфир".as_bytes(),
        )
        .unwrap();
    writer.cell("Item", 7, 0, 0, 1, "Зелье".as_bytes()).unwrap();
    writer.cell("quest/Text", 1, 2, 1, 5, b"").unwrap();
    writer.finish().unwrap()
}

#[test]
fn writer_output_parses_and_reads_back() {
    let bytes = sample();
    let table = Table::parse(&bytes).unwrap();
    assert_eq!(table.language(), 2);
    assert_eq!(table.domain(), b"DOMAIN");
    assert_eq!(table.sheet_count(), 2, "sheets without cells are left out");
    assert_eq!(table.cell_count(), 4);
    let item = table.sheet("Item").unwrap();
    assert_eq!(item.variant, Variant::Rows);
    assert_eq!(
        item.columns().collect::<Vec<_>>(),
        [
            Column {
                index: 0,
                offset: 0
            },
            Column {
                index: 3,
                offset: 8
            }
        ]
    );
    let row = item.row(7, 0);
    assert_eq!(row.len(), 2);
    assert_eq!((row[0].column, row[1].column), (0, 3));
    assert_eq!(row[1].guard, guard(b"DOMAIN", b"Potion"));
    assert_eq!(row[1].text, "Зелье".as_bytes());
    assert_eq!(row[0].text, row[1].text);
    assert!(item.row(3, 0).is_empty());
    assert_eq!(table.sheet("quest/Text").unwrap().row(1, 2)[0].text, b"");
    assert!(table.sheet("Missing").is_none());
}

#[test]
fn every_truncation_and_trailing_byte_is_refused() {
    let bytes = sample();
    for length in 0..bytes.len() {
        assert!(Table::parse(&bytes[..length]).is_err(), "{length}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Table::parse(&longer).is_err());
}

#[test]
fn malformed_headers_and_records_are_refused() {
    let bytes = sample();
    let mutate = |at: usize, value: u8| {
        let mut copy = bytes.clone();
        copy[at] = value;
        Table::parse(&copy).map(|_| ())
    };
    assert!(mutate(0, b'X').is_err(), "magic");
    assert!(mutate(8, 2).is_err(), "version");
    assert!(mutate(48, 9).is_err(), "language");
    assert!(mutate(49, 65).is_err(), "domain length");
    assert!(mutate(50, 1).is_err(), "reserved");
    assert!(mutate(127, 1).is_err(), "domain padding");
    // The first sheet's variant.
    assert!(mutate(128 + 8, 2).is_err(), "variant");
    let mut any = bytes.clone();
    any[48] = ANY_LANGUAGE;
    assert!(Table::parse(&any).is_ok());
}

#[test]
fn writer_refuses_cells_the_reader_would_refuse() {
    let mut writer = Writer::new(ANY_LANGUAGE, b"").unwrap();
    writer
        .sheet(
            "Item",
            Variant::Rows,
            &[Column {
                index: 0,
                offset: 0,
            }],
        )
        .unwrap();
    assert!(
        writer
            .sheet(
                "Item",
                Variant::Rows,
                &[Column {
                    index: 0,
                    offset: 0
                }]
            )
            .is_err()
    );
    assert!(writer.cell("Item", 1, 1, 0, 0, b"x").is_err(), "subrow");
    assert!(
        writer.cell("Item", 1, 0, 1, 0, b"x").is_err(),
        "not a text column"
    );
    assert!(
        writer.cell("Other", 1, 0, 0, 0, b"x").is_err(),
        "undeclared"
    );
    assert!(writer.cell("Item", 1, 0, 0, 0, b"a\0b").is_err(), "NUL");
    assert!(writer.cell("Item", 1, 0, 0, 0, &[0x02]).is_err(), "macro");
    assert!(
        writer.cell("Item", 1, 0, 0, 0, b"_rsv_x").is_err(),
        "reserved"
    );
    writer.cell("Item", 1, 0, 0, 0, b"x").unwrap();
    writer.cell("Item", 1, 0, 0, 0, b"y").unwrap();
    assert!(writer.finish().is_err(), "repeat");
    assert!(Writer::new(9, b"").is_err());
    assert!(Writer::new(0, &[0; 65]).is_err());
}

#[test]
fn owned_table_reads_like_the_parsed_one() {
    let owned = angelica_text::table::OwnedTable::new(sample()).unwrap();
    let table = owned.table();
    assert_eq!(table.cell_count(), 4);
    assert_eq!(
        table.sheet("Item").unwrap().row(2, 0)[0].text,
        "Эфир".as_bytes()
    );
    assert!(angelica_text::table::OwnedTable::new(vec![0; 10]).is_err());
}

/// A small deterministic generator, so a failing case can be replayed.
struct Mutator(u64);

impl Mutator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound.max(1) as u64).unwrap()
    }

    /// `bytes` with one to four random changes: bits, bytes, length-like
    /// fields set to edge values, truncation, or a repeated slice.
    fn mutate(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        for _ in 0..=self.below(4) {
            let at = self.below(bytes.len());
            match self.below(5) {
                0 if !bytes.is_empty() => bytes[at] ^= 1 << self.below(8),
                1 if !bytes.is_empty() => bytes[at] = self.next().to_le_bytes()[0],
                2 if bytes.len() >= at + 8 => {
                    let edges = [0, 1, 0xFF, 0xFFFF_FFFF, u64::MAX, bytes.len() as u64];
                    let value = edges[self.below(edges.len())];
                    let width = [2, 4, 8][self.below(3)];
                    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
                }
                3 => bytes.truncate(at),
                _ => {
                    let end = (at + self.below(64)).min(bytes.len());
                    let slice = bytes[at..end].to_vec();
                    let to = self.below(bytes.len());
                    bytes.splice(to..to, slice);
                }
            }
        }
        bytes
    }
}

#[test]
fn damaged_tables_are_refused_without_panicking() {
    let sample = sample();
    let mut mutator = Mutator(0x9E37_79B9_7F4A_7C15);
    let mut accepted = 0;
    for _ in 0..100_000 {
        let bytes = mutator.mutate(&sample);
        let Ok(table) = Table::parse(&bytes) else {
            continue;
        };
        accepted += 1;
        for sheet in table.sheets() {
            for cell in sheet.cells() {
                assert!(!sheet.row(cell.row, cell.subrow).is_empty());
                let _ = sestring::plain(cell.text);
            }
            assert!(table.sheet(sheet.name).is_some());
        }
    }
    // Some mutations keep a valid table, such as a changed text byte.
    assert!(accepted > 0);
}

#[test]
fn plain_text_leaves_out_macros() {
    assert_eq!(sestring::plain(b"Potion"), "Potion");
    // A line break, then a color macro around a word.
    assert_eq!(
        sestring::plain(b"a\x02\x10\x01\x03b \x02\x48\x04\xF2\x02\x25\x03red\x02\x48\x02\x01\x03"),
        "a\nb red"
    );
    assert_eq!(
        sestring::plain(b"x\x02\x1D\x01\x03y\x02\x1F\x01\x03z\x02\x16\x01\x03"),
        "x y-z"
    );
    assert_eq!(sestring::plain("Зелье".as_bytes()), "Зелье");
    // A broken macro and bytes that are not UTF-8 stay visible.
    assert_eq!(sestring::plain(b"a\x02\x48"), "a\u{2}H");
    assert_eq!(sestring::plain(b"\xFFok"), "\u{FFFD}ok");
}
