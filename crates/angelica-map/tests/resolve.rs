use angelica_map::{Absence, BuildRecord, Map, Value};
use angelica_pe::testing::{CODE, PeBuilder, RDATA};
use angelica_pe::{Image, Layout};

const BASE: u64 = 0x1_4000_0000;
const VTABLE: u32 = 0x2000;

/// Code at 0x1000 and a vtable at 0x2000 whose four slots point at the two
/// functions, alternately.
fn builder(vtable_base: u64) -> PeBuilder {
    let mut code = vec![0xCC; 0x100];
    // 0x1000: first function; at 0x1010, mov rax, [rcx+0x10].
    code[0x00..0x0A].copy_from_slice(&[0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20]);
    code[0x10..0x14].copy_from_slice(&[0x48, 0x8B, 0x41, 0x10]);
    // 0x1018: mov rax, [rip+0] — RIP-relative, not a field.
    code[0x18..0x1F].copy_from_slice(&[0x48, 0x8B, 0x05, 0, 0, 0, 0]);
    // 0x1040: second function.
    code[0x40..0x46].copy_from_slice(&[0x40, 0x53, 0x48, 0x83, 0xEC, 0x30]);
    // 0x1060: a leaf function, not in the exception directory:
    // mov rax, [rdx+0x18]; ret; then int3 padding.
    code[0x60..0x65].copy_from_slice(&[0x48, 0x8B, 0x42, 0x18, 0xC3]);
    // 0x1080: lea rax, [rip+disp] to the vtable; ret.
    code[0x80..0x83].copy_from_slice(&[0x48, 0x8D, 0x05]);
    let displacement = i64::from(VTABLE) - (0x1083 + 4);
    code[0x83..0x87].copy_from_slice(&i32::try_from(displacement).unwrap().to_le_bytes());
    code[0x87] = 0xC3;
    // 0x10A0 and 0x10B0: the same bytes twice.
    code[0xA0..0xA4].copy_from_slice(&[0xAB, 0xCD, 0xEF, 0x01]);
    code[0xB0..0xB4].copy_from_slice(&[0xAB, 0xCD, 0xEF, 0x01]);
    // 0x10C0: je +5, then a marker.
    code[0xC0..0xC4].copy_from_slice(&[0x74, 0x05, 0x66, 0x90]);

    let mut rdata = Vec::new();
    for target in [0x1000_u64, 0x1040, 0x1000, 0x1040] {
        rdata.extend_from_slice(&(vtable_base + target).to_le_bytes());
    }
    PeBuilder::new()
        .section(".text", CODE, &code)
        .section(".rdata", RDATA, &rdata)
        .functions(&[(0x1000, 0x1030), (0x1040, 0x1060)])
}

const ENTRIES: &str = r#"
[vtable."game.thing.vtable"]
locator = { signature = "48 8D 05 ?? ?? ?? ?? C3", at = 3, follow = "rel32" }
slots = 4

[global."game.thing.vtable_address"]
locator = { signature = "48 8D 05 ?? ?? ?? ?? C3", at = 3, follow = "rel32" }
type = "ptr"
checks = [{ equals = "game.thing.vtable" }]

[function."game.thing.first"]
locator = { signature = "48 89 5C 24 08 57 48 83 EC 20" }
abi = { args = ["ptr"], ret = "void" }
thread = "any"
checks = [{ vtable_slot = 0, code_slots = 4 }, { vtable = "game.thing.vtable", slot = 2 }]

[function."game.thing.second"]
locator = { vtable = "game.thing.vtable", slot = 1 }
abi = { args = [], ret = "u8" }
thread = "main"

[function."game.thing.not_slot"]
locator = { signature = "48 8D 05" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ vtable_slot = 0, code_slots = 4 }]

[function."game.thing.twice"]
locator = { signature = "AB CD EF 01" }
abi = { args = [], ret = "void" }
thread = "any"

[function."game.thing.missing"]
locator = { signature = "DE AD BE EF" }
abi = { args = [], ret = "void" }
thread = "any"

[function."game.thing.dependent"]
locator = { vtable = "game.thing.vtable", slot = 3 }
abi = { args = [], ret = "void" }
thread = "any"
depends = ["game.thing.twice"]

[function."game.thing.wrong_slot"]
locator = { signature = "40 53 48 83 EC 30" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ vtable = "game.thing.vtable", slot = 0 }]

[global."game.thing.in_data"]
locator = { signature = "48 8D 05 ?? ?? ?? ?? C3", at = 3, follow = "rel32" }
type = "ptr"

[location."game.thing.data_location"]
locator = { signature = "48 8D 05 ?? ?? ?? ?? C3", at = 3, follow = "rel32" }
thread = "any"

[field."game.thing.count"]
of = "game.thing"
type = "u32"
locator = { in = "game.thing.first", signature = "48 8B 41 10", operand = "disp" }

[field."game.thing.relative"]
of = "game.thing"
type = "u32"
locator = { in = "game.thing.first", signature = "48 8B 05", operand = "disp" }

[field."game.thing.outside_function"]
of = "game.thing"
type = "u32"
locator = { in = "game.thing.second", signature = "48 8B 41 10", operand = "disp" }

[function."game.thing.leaf"]
locator = { signature = "48 8B 42 18 C3" }
abi = { args = ["ptr", "ptr"], ret = "ptr" }
thread = "any"

[field."game.thing.leaf_size"]
of = "game.thing"
type = "u64"
locator = { in = "game.thing.leaf", signature = "48 8B 42 18", operand = "disp" }

[field."game.thing.past_leaf"]
of = "game.thing"
type = "u64"
locator = { in = "game.thing.leaf", signature = "48 8D 05", operand = "disp" }

[field."game.thing.name"]
of = "game.thing"
type = "ptr"
offset = 0x8

[patch."game.thing.skip_check"]
locator = { signature = "74 05 66 90" }
original = "74 05"
replacement = "EB 05"

[patch."game.thing.changed_site"]
locator = { signature = "74 05 66 90" }
original = "75 05"
replacement = "EB 05"
"#;

fn map() -> Map {
    Map::from_sources(
        "format = 2\n",
        &[("entries/game.toml".to_owned(), ENTRIES.to_owned())],
    )
    .unwrap()
}

fn expected() -> Vec<(&'static str, Result<Value, Absence>)> {
    vec![
        ("game.thing.changed_site", Err(Absence::PatchMismatch)),
        ("game.thing.count", Ok(Value::Offset(0x10))),
        ("game.thing.data_location", Err(Absence::NotCode)),
        (
            "game.thing.dependent",
            Err(Absence::Dependency("game.thing.twice".to_owned())),
        ),
        ("game.thing.first", Ok(Value::Address(0x1000))),
        ("game.thing.in_data", Ok(Value::Address(VTABLE))),
        ("game.thing.leaf", Ok(Value::Address(0x1060))),
        ("game.thing.leaf_size", Ok(Value::Offset(0x18))),
        ("game.thing.missing", Err(Absence::NoMatch)),
        ("game.thing.name", Ok(Value::Offset(0x8))),
        (
            "game.thing.not_slot",
            Err(Absence::CheckFailed {
                index: 0,
                reason: Box::new(Absence::NotInVtable {
                    slot: 0,
                    code_slots: 4,
                }),
            }),
        ),
        ("game.thing.outside_function", Err(Absence::NoMatch)),
        ("game.thing.past_leaf", Err(Absence::NoMatch)),
        ("game.thing.relative", Err(Absence::NoOperand)),
        ("game.thing.second", Ok(Value::Address(0x1040))),
        ("game.thing.skip_check", Ok(Value::Address(0x10C0))),
        ("game.thing.twice", Err(Absence::Ambiguous(2))),
        ("game.thing.vtable", Ok(Value::Address(VTABLE))),
        ("game.thing.vtable_address", Ok(Value::Address(VTABLE))),
        (
            "game.thing.wrong_slot",
            Err(Absence::CheckDiffers {
                index: 0,
                found: 0x1000,
            }),
        ),
    ]
}

fn results(map: &Map, image: &Image<'_>, load_base: u64) -> Vec<(String, Result<Value, Absence>)> {
    let resolution = map.resolve(image, load_base);
    resolution
        .results()
        .iter()
        .map(|(name, result)| (name.clone(), result.clone()))
        .collect()
}

fn named(
    expected: Vec<(&'static str, Result<Value, Absence>)>,
) -> Vec<(String, Result<Value, Absence>)> {
    expected
        .into_iter()
        .map(|(name, result)| (name.to_owned(), result))
        .collect()
}

#[test]
fn resolves_every_kind_of_locator_and_check() {
    let file = builder(BASE).build_file();
    let image = Image::parse(&file, Layout::File).unwrap();
    assert_eq!(results(&map(), &image, BASE), named(expected()));
}

#[test]
fn the_mapped_module_resolves_like_the_file() {
    // In the game, the module may be loaded elsewhere and its vtables hold
    // relocated addresses.
    let relocated = 0x7FF6_1000_0000;
    let mapped = builder(relocated).build_mapped();
    let image = Image::parse(&mapped, Layout::Mapped).unwrap();
    assert_eq!(results(&map(), &image, relocated), named(expected()));
}

#[test]
fn a_wrong_load_base_makes_slot_locators_absent() {
    let file = builder(BASE).build_file();
    let image = Image::parse(&file, Layout::File).unwrap();
    let resolution = map().resolve(&image, BASE + 0x1000_0000);
    assert_eq!(
        resolution.get("game.thing.second"),
        Some(&Err(Absence::Dependency("game.thing.vtable".to_owned())))
    );
    assert_eq!(
        resolution.get("game.thing.vtable"),
        Some(&Err(Absence::SlotNotCode(0)))
    );
}

#[test]
fn lists_constant_offsets() {
    let file = builder(BASE).build_file();
    let image = Image::parse(&file, Layout::File).unwrap();
    assert_eq!(
        map().resolve(&image, BASE).constant_offsets(),
        ["game.thing.name"]
    );
}

#[test]
fn writes_a_deterministic_build_record() {
    let file = builder(BASE).build_file();
    let image = Image::parse(&file, Layout::File).unwrap();
    let map = map();
    let record = BuildRecord {
        game_version: "2026.09.15.0000.0000".to_owned(),
        exe_sha256: "ab".repeat(32),
        entries_sha256: map.entries_sha256().to_owned(),
        resolution: map.resolve(&image, BASE),
    };
    let text = record.to_toml();
    let parsed = BuildRecord::from_toml(&text, &map, &image).unwrap();
    assert_eq!(
        parsed.resolution.present().collect::<Vec<_>>(),
        record.resolution.present().collect::<Vec<_>>()
    );
    assert_eq!(
        parsed.resolution.absent().count(),
        record.resolution.absent().count()
    );
    assert!(BuildRecord::matches(&text, &record.exe_sha256, &record.entries_sha256).unwrap());
    assert!(!BuildRecord::matches(&text, &"cd".repeat(32), &record.entries_sha256).unwrap());
    assert!(BuildRecord::from_toml(&text.replace("0x1000", "0xFFFFFFFF"), &map, &image).is_err());
    assert!(
        BuildRecord::from_toml(
            &text.replace("\"game.thing.name\" = 0x8", "\"game.thing.name\" = 0x9"),
            &map,
            &image
        )
        .is_err()
    );
    assert_eq!(text, record.to_toml());
    assert!(text.starts_with("# Generated by `angelica map verify`. Do not edit.\ngame_version = \"2026.09.15.0000.0000\"\n"));
    assert!(text.contains("\n[present]\n\"game.thing.count\" = 0x10\n"));
    assert!(text.contains("\"game.thing.first\" = 0x1000\n"));
    assert!(text.contains("\n[absent]\n\"game.thing.changed_site\" = \"bytes at the patch site differ from `original`\"\n"));
    assert!(text.contains("\"game.thing.twice\" = \"signature matched 2 addresses\"\n"));
    assert!(text.ends_with("\n[constant_offsets]\nnames = [\"game.thing.name\"]\n"));
    // The record is valid TOML.
    let value = text.parse::<toml::Table>().unwrap();
    assert_eq!(
        value["present"]["game.thing.second"].as_integer(),
        Some(0x1040)
    );
}

const FACTS: &str = r#"
[function."game.facts.primary"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ contains = "11 22 ?? 44" }]

[function."game.facts.in_chunk"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ contains = "55 66 77 88" }]

[function."game.facts.outside"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ contains = "99 99 99 99" }]

[function."game.facts.twice"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ contains = "AA BB CC DD" }]

[function."game.facts.other"]
locator = { signature = "40 53 48 83 EC 30" }
abi = { args = [], ret = "void" }
thread = "any"
checks = [{ contains = "55 66 77 88", in = "game.facts.primary" }]
"#;

#[test]
fn a_contains_check_searches_the_whole_function() {
    let mut code = vec![0xCC; 0x60];
    // 0x1000: the function's primary range.
    code[0x00..0x0D].copy_from_slice(&[
        0x48, 0x89, 0x5C, 0x24, 0x08, 0x11, 0x22, 0x33, 0x44, 0xAA, 0xBB, 0xCC, 0xDD,
    ]);
    // 0x1020: a chunk chained to it.
    code[0x20..0x28].copy_from_slice(&[0x55, 0x66, 0x77, 0x88, 0xAA, 0xBB, 0xCC, 0xDD]);
    // 0x1040: another function.
    code[0x40..0x4A].copy_from_slice(&[0x40, 0x53, 0x48, 0x83, 0xEC, 0x30, 0x99, 0x99, 0x99, 0x99]);
    let file = PeBuilder::new()
        .section(".text", CODE, &code)
        .functions(&[(0x1000, 0x1010), (0x1020, 0x1030), (0x1040, 0x1050)])
        .chained(0x1020, 0x1000)
        .build_file();
    let image = Image::parse(&file, Layout::File).unwrap();
    let map = Map::from_sources(
        "format = 2\n",
        &[("entries/game.toml".to_owned(), FACTS.to_owned())],
    )
    .unwrap();
    let resolution = map.resolve(&image, BASE);
    let failed = |reason| {
        Some(Err(Absence::CheckFailed {
            index: 0,
            reason: Box::new(reason),
        }))
    };
    assert_eq!(
        resolution.get("game.facts.primary").cloned(),
        Some(Ok(Value::Address(0x1000)))
    );
    assert_eq!(
        resolution.get("game.facts.in_chunk").cloned(),
        Some(Ok(Value::Address(0x1000))),
        "a fact in a chained chunk is part of the function"
    );
    assert_eq!(
        resolution.get("game.facts.outside").cloned(),
        failed(Absence::NoMatch),
        "bytes of another function are not this one's"
    );
    assert_eq!(
        resolution.get("game.facts.twice").cloned(),
        failed(Absence::Ambiguous(2))
    );
    assert_eq!(
        resolution.get("game.facts.other").cloned(),
        Some(Ok(Value::Address(0x1040)))
    );
}
