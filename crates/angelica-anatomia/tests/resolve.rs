use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use angelica_anatomia::{Anatomia, AnatomiaError, BuildRecord, Value, convert_map};
use angelica_map::Map;
use angelica_pe::testing::{CODE, PeBuilder, RDATA};
use angelica_pe::{Image, Layout};

const BASE: u64 = 0x1_4000_0000;
const VTABLE: u32 = 0x2000;

/// Code at 0x1000 and data at 0x2000: a vtable whose four slots point at
/// the two functions alternately, a pointer to the second function, and a
/// marker.
fn image_bytes() -> Vec<u8> {
    let mut code = vec![0xCC; 0x200];
    // 0x1000: the first function; at 0x1010, mov rax, [rcx+0x10].
    code[0x00..0x0A].copy_from_slice(&[0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20]);
    code[0x10..0x14].copy_from_slice(&[0x48, 0x8B, 0x41, 0x10]);
    // 0x1040: the second function.
    code[0x40..0x46].copy_from_slice(&[0x40, 0x53, 0x48, 0x83, 0xEC, 0x30]);
    // 0x1080: lea rax, [rip+disp] to the vtable; ret.
    code[0x80..0x83].copy_from_slice(&[0x48, 0x8D, 0x05]);
    let displacement = i64::from(VTABLE) - (0x1083 + 4);
    code[0x83..0x87].copy_from_slice(&i32::try_from(displacement).unwrap().to_le_bytes());
    code[0x87] = 0xC3;
    // 0x10C0: mov eax, 0x12345678; ret.
    code[0xC0..0xC6].copy_from_slice(&[0xB8, 0x78, 0x56, 0x34, 0x12, 0xC3]);
    // 0x10D0: call 0x1040; ret.
    code[0xD0] = 0xE8;
    let call = 0x1040_i64 - (0x10D1 + 4);
    code[0xD1..0xD5].copy_from_slice(&i32::try_from(call).unwrap().to_le_bytes());
    code[0xD5] = 0xC3;
    // 0x10E0: jmp short to 0x10E4; nop; nop; ret.
    code[0xE0..0xE5].copy_from_slice(&[0xEB, 0x02, 0x90, 0x90, 0xC3]);

    let mut rdata = Vec::new();
    for target in [0x1000_u64, 0x1040, 0x1000, 0x1040] {
        rdata.extend_from_slice(&(BASE + target).to_le_bytes());
    }
    // 0x2020: a pointer to the second function; 0x2028: a marker.
    rdata.extend_from_slice(&(BASE + 0x1040).to_le_bytes());
    rdata.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
    PeBuilder::new()
        .section(".text", CODE, &code)
        .section(".rdata", RDATA, &rdata)
        .functions(&[(0x1000, 0x1030), (0x1040, 0x1060)])
        .build_file()
}

fn fixture() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/map"))
}

fn load(converted: &BTreeMap<String, String>) -> Anatomia {
    let sources: Vec<(String, String)> = converted
        .iter()
        .filter(|(path, _)| {
            [
                "types/",
                "functions/",
                "globals/",
                "meaning/",
                "sheets/",
                "windows/",
            ]
            .iter()
            .any(|folder| path.starts_with(folder))
        })
        .map(|(path, text)| (path.clone(), text.clone()))
        .collect();
    Anatomia::from_sources(&converted["anatomia.toml"], &sources).unwrap()
}

#[test]
fn the_converted_fixture_resolves_as_the_map_does() {
    let bytes = image_bytes();
    let image = Image::parse(&bytes, Layout::File).unwrap();
    let map = Map::load(fixture()).unwrap();
    let converted = convert_map(fixture()).unwrap();
    let anatomia = load(&converted.files);
    let rename = |name: &str| {
        converted
            .renamed
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    };

    // Every constant offset confirmed, so each entry has a value to compare.
    let constants: BTreeSet<String> = map
        .entries()
        .filter(|entry| {
            matches!(
                entry.kind,
                angelica_map::EntryKind::Field {
                    offset: Some(_),
                    ..
                }
            )
        })
        .map(|entry| entry.name.clone())
        .collect();
    let expected = map.resolve_confirming(&image, BASE, &constants);
    let found = anatomia.resolve(
        &image,
        BASE,
        &constants.iter().map(|name| rename(name)).collect(),
    );

    assert!(
        expected.absent().next().is_none(),
        "{:?}",
        expected.absent().collect::<Vec<_>>()
    );
    let expected: BTreeMap<String, u32> = expected
        .present()
        .map(|(name, value)| (rename(name), value.get()))
        .collect();
    let found_values: BTreeMap<String, u32> = found
        .present()
        .map(|(name, value)| (name.to_owned(), value.get()))
        .collect();
    assert_eq!(found_values, expected);
    assert_eq!(found.confirmed().len(), constants.len());

    let record = BuildRecord {
        game_version: "2026.10.16.0000.0000".to_owned(),
        exe_sha256: "0".repeat(64),
        anatomia_sha256: anatomia.sha256().to_owned(),
        resolution: found,
    };
    assert_eq!(
        BuildRecord::read(&record.to_toml(), &anatomia).unwrap(),
        record
    );
}

const MANIFEST: &str = "format = \"1.0\"\nnamespace = \"ffxiv\"\n";

fn resolve(files: &[(&str, &str)]) -> (Anatomia, BTreeMap<String, Result<u32, String>>) {
    let sources: Vec<(String, String)> = files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect();
    let anatomia = Anatomia::from_sources(MANIFEST, &sources).unwrap();
    let bytes = image_bytes();
    let image = Image::parse(&bytes, Layout::File).unwrap();
    let resolution = anatomia.resolve(&image, BASE, &BTreeSet::new());
    let results = resolution
        .results()
        .iter()
        .map(|(name, result)| {
            (
                name.clone(),
                result
                    .as_ref()
                    .map(|value| value.get())
                    .map_err(|absence| absence.0.clone()),
            )
        })
        .collect();
    (anatomia, results)
}

#[test]
fn every_step_kind_finds_its_value() {
    let functions = r#"
[function."game.first"]
type = "fn(this: ptr, flag: u8)"
thread = "main"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]
checks = [{ bytes = "48 8B 41 10", at = 0x10 }, { contains = "48 8B 41 10" }, { vtable_slot = 0, code_slots = 4 }]

[function."game.second"]
type = "fn() -> u8"
thread = "any"
find = [{ scan = "E8 ?? ?? ?? ?? C3" }, { operand = "target" }]
checks = [{ find = [{ scan = "E8 ?? ?? ?? ?? C3" }, { rel32 = 1 }] }, { equals = "game.by_slot" }]

[function."game.by_slot"]
type = "fn() -> u8"
thread = "any"
find = [{ vtable = "game.table", slot = "second" }]

[function."game.by_pointer"]
type = "fn() -> u8"
thread = "any"
find = [{ scan = "AA BB CC DD EE", section = "data" }, { offset = -8 }, { deref = "ptr" }]

[location."game.after_jump"]
thread = "main"
find = [{ scan = "EB 02 90 90" }, { rel8 = 1 }]

[location."game.constant"]
thread = "main"
find = [{ scan = "B8 ?? ?? ?? ?? C3" }]
checks = [{ operand = "imm", equals = 0x12345678 }]
"#;
    let globals = r#"
[vtable."game.table"]
find = [{ scan = "48 8D 05 ?? ?? ?? ?? C3" }, { operand = "target" }]
code_slots = 4
slots = { first = 0, second = { slot = 1, type = "fn() -> u8" } }

[global."game.table_address"]
type = "ptr"
find = [{ entry = "game.table" }]
"#;
    let types = r#"
[struct."game.thing"]
doc = "A thing."

[struct."game.thing".fields.count]
type = "u32"
find = [{ in = "game.first" }, { scan = "48 8B 41 10" }, { operand = "disp" }]
aliases = ["old_count"]

[struct."game.thing".fields.flags]
type = "u8"
at = 0x18

[struct."game.thing".fields.shown]
type = "bool"
bits = { in = "flags", at = 2 }

[struct."game.thing".fields.same_count]
type = "u32"
find = [{ entry = "game.thing.old_count" }]
"#;
    let (anatomia, results) = resolve(&[
        ("functions/game.toml", functions),
        ("globals/game.toml", globals),
        ("types/game.toml", types),
    ]);
    assert!(anatomia.broken().is_empty(), "{:?}", anatomia.broken());
    let expected: BTreeMap<String, Result<u32, String>> = [
        ("game.first", Ok(0x1000)),
        ("game.second", Ok(0x1040)),
        ("game.by_slot", Ok(0x1040)),
        ("game.by_pointer", Ok(0x1040)),
        ("game.after_jump", Ok(0x10E4)),
        ("game.constant", Ok(0x10C0)),
        ("game.table", Ok(VTABLE)),
        ("game.table_address", Ok(VTABLE)),
        ("game.thing.count", Ok(0x10)),
        ("game.thing.same_count", Ok(0x10)),
        (
            "game.thing.flags",
            Err("a constant offset that no build record confirms for this executable".to_owned()),
        ),
    ]
    .into_iter()
    .map(|(name, result)| (name.to_owned(), result))
    .collect();
    assert_eq!(results, expected);
    assert_eq!(
        anatomia.canonical("game.thing.old_count"),
        "game.thing.count"
    );
}

#[test]
fn what_cannot_be_read_is_absent_alone() {
    let functions = r#"
[function."game.first"]
type = "fn(ptr)"
thread = "main"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]

[function."game.newer_key"]
type = "fn()"
thread = "main"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]
calling = "vectorcall"

[function."game.newer_step"]
type = "fn()"
thread = "main"
find = [{ pattern = "48 89" }]

[function."game.newer_check"]
type = "fn()"
thread = "main"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]
checks = [{ entropy = 3 }]

[function."game.uses_newer"]
type = "fn()"
thread = "main"
find = [{ entry = "game.newer_step" }]

[function."game.missing_reference"]
type = "fn()"
thread = "main"
find = [{ entry = "game.nothing" }]

[function."game.loop_a"]
type = "fn()"
thread = "main"
find = [{ entry = "game.loop_b" }]

[function."game.loop_b"]
type = "fn()"
thread = "main"
find = [{ entry = "game.loop_a" }]

[function."game.twice"]
type = "fn()"
thread = "main"
find = [{ scan = "CC CC" }]

[gadget."game.unknown_kind"]
find = [{ scan = "CC" }]

[function."other.wrong_area"]
type = "fn()"
thread = "main"
find = [{ scan = "CC" }]
"#;
    let types = r#"
[struct."game.thing"]
[struct."game.thing".fields.unknown_type]
type = "game.nowhere"
at = 0

[struct."game.thing".fields.good]
type = "u32"
find = [{ in = "game.first" }, { scan = "48 8B 41 10" }, { operand = "disp" }]
"#;
    let (anatomia, results) = resolve(&[
        ("functions/game.toml", functions),
        ("types/game.toml", types),
    ]);
    let reason = |name: &str| match results.get(name) {
        Some(Err(reason)) => reason.clone(),
        other => panic!("{name}: {other:?}"),
    };
    assert_eq!(results["game.first"], Ok(0x1000));
    assert_eq!(results["game.thing.good"], Ok(0x10));
    assert!(reason("game.newer_key").contains("unknown key `calling`"));
    assert!(reason("game.newer_step").contains("step kind `pattern`"));
    assert!(reason("game.newer_check").contains("not a check kind"));
    assert!(reason("game.uses_newer").contains("depends on absent entry `game.newer_step`"));
    assert!(reason("game.missing_reference").contains("`game.nothing`, which is not an entry"));
    assert!(
        reason("game.loop_a").contains("refers to itself")
            || reason("game.loop_b").contains("refers to itself")
    );
    assert!(reason("game.twice").contains("matched"));
    assert!(reason("game.thing.unknown_type").contains("`game.nowhere`"));
    assert!(anatomia.broken()["game.unknown_kind"].contains("`gadget`"));
    assert!(anatomia.broken()["other.wrong_area"].contains("area"));
}

#[test]
fn a_newer_minor_version_reads_and_another_major_does_not() {
    let newer = Anatomia::from_sources(
        "format = \"1.7\"\nnamespace = \"ffxiv\"\nmirrors = []\n",
        &[],
    );
    assert_eq!(newer.unwrap().minor, 7);
    assert!(matches!(
        Anatomia::from_sources("format = \"2.0\"\nnamespace = \"ffxiv\"\n", &[]),
        Err(AnatomiaError::Format { .. })
    ));
    assert!(matches!(
        Anatomia::from_sources(
            "format = \"1.0\"\nnamespace = \"ffxiv\"\nmirrors = []\n",
            &[]
        ),
        Err(AnatomiaError::Manifest { .. })
    ));
}

#[test]
fn an_undeclared_extension_makes_its_entry_absent() {
    let functions = r#"
[function."game.first"]
type = "fn()"
thread = "main"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]
ext = { "someone.notes" = { seen = "7.35" } }
"#;
    let (anatomia, _) = resolve(&[("functions/game.toml", functions)]);
    assert!(anatomia.broken()["game.first"].contains("not declared"));

    let declared = Anatomia::from_sources(
        "format = \"1.0\"\nnamespace = \"ffxiv\"\nextensions = [\"someone.notes\"]\n",
        &[("functions/game.toml".to_owned(), functions.to_owned())],
    )
    .unwrap();
    assert!(declared.broken().is_empty());
    assert!(matches!(
        declared.get("game.first").map(|entry| entry.ext.len()),
        Some(1)
    ));
}

#[test]
fn present_values_are_addresses_or_offsets() {
    let types = "[struct.\"game.thing\"]\n[struct.\"game.thing\".fields.count]\ntype = \"u32\"\nfind = [{ scan = \"48 8B 41 10\" }]\n";
    let (_, results) = resolve(&[("types/game.toml", types)]);
    assert!(
        results["game.thing.count"]
            .as_ref()
            .unwrap_err()
            .contains("a field needs an offset")
    );
    let _ = Value::Offset(0);
}
