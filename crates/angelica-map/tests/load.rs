use angelica_map::{Check, EntryKind, Locator, Map, MapError, Thread, ValueKind};

const MAP: &str = "format = 2\n";

fn map(files: &[(&str, &str)]) -> Result<Map, MapError> {
    let files = files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect::<Vec<_>>();
    Map::from_sources(MAP, &files)
}

fn entry_error(text: &str) -> String {
    match map(&[("entries/game.toml", text)]) {
        Err(MapError::InvalidEntry { message, .. }) => message,
        other => panic!("expected an invalid entry, got {other:?}"),
    }
}

#[test]
fn reads_every_kind_of_entry() {
    let map = map(&[(
        "entries/game.toml",
        r#"
[function."game.thing.update"]
locator = { signature = "48 89 5C 24 ??", at = 0 }
abi = { args = ["ptr", "u8"], ret = "void" }
thread = "main"
doc = "Updates a thing."
origin = { source = "notes", commit = "abc" }

[location."game.thing.update_branch"]
locator = { signature = "74 05", at = 1 }
thread = "any"

[global."game.thing.instance"]
locator = { signature = "48 8B 0D ?? ?? ?? ??", at = 3, follow = "rel32" }
type = "ptr"

[vtable."game.thing.vtable"]
locator = { signature = "48 8D 05 ?? ?? ?? ??", at = 3, follow = "rel32" }
slots = 4

[field."game.thing.count"]
of = "game.thing"
type = "u32"
locator = { in = "game.thing.update", signature = "8B 41 10", operand = "disp" }

[field."game.thing.name"]
of = "game.thing"
type = "ptr"
offset = 0x8

[patch."game.thing.skip_check"]
locator = { signature = "74 05 48" }
original = "74 05"
replacement = "EB 05"
checks = [{ equals = "game.thing.update_branch" }, { vtable_slot = 1, code_slots = 4 }, { vtable = "game.thing.vtable", slot = 3 }]
depends = ["game.thing.instance"]
"#,
    )])
    .unwrap();

    assert_eq!(map.len(), 7);
    let update = map.get("game.thing.update").unwrap();
    assert_eq!(
        update.kind,
        EntryKind::Function {
            abi: angelica_map::Abi {
                args: vec![ValueKind::Ptr, ValueKind::U8],
                ret: ValueKind::Void
            },
            thread: Thread::Main,
        }
    );
    assert_eq!(update.doc.as_deref(), Some("Updates a thing."));
    assert_eq!(
        update.origin.as_ref().map(|origin| origin.source.as_str()),
        Some("notes")
    );
    assert!(matches!(
        map.get("game.thing.count").unwrap().locator,
        Some(Locator::Operand { ref function, .. }) if function == "game.thing.update"
    ));
    assert_eq!(
        map.get("game.thing.name").unwrap().kind,
        EntryKind::Field {
            of: "game.thing".to_owned(),
            value: ValueKind::Ptr,
            offset: Some(8)
        }
    );
    let patch = map.get("game.thing.skip_check").unwrap();
    assert_eq!(
        patch.checks[0],
        Check::Equals("game.thing.update_branch".to_owned())
    );
    assert_eq!(
        patch.checks[1],
        Check::InVtable {
            slot: 1,
            code_slots: 4
        }
    );
    assert_eq!(
        patch.references(),
        [
            "game.thing.update_branch",
            "game.thing.vtable",
            "game.thing.instance"
        ]
    );
}

#[test]
fn refuses_an_unsupported_format() {
    assert_eq!(
        Map::from_sources("", &[]).unwrap_err(),
        MapError::Format {
            found: None,
            supported: 2
        }
    );
    assert_eq!(
        Map::from_sources("format = 1", &[]).unwrap_err(),
        MapError::Format {
            found: Some(1),
            supported: 2
        }
    );
    assert!(matches!(
        Map::from_sources("format = 2\nextra = 1", &[]),
        Err(MapError::Toml { .. })
    ));
}

#[test]
fn refuses_unknown_tables_and_fields() {
    assert!(matches!(
        map(&[("entries/game.toml", "[method.\"game.a\"]\n")]),
        Err(MapError::Toml { .. })
    ));
    assert!(matches!(
        map(&[(
            "entries/game.toml",
            "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\ncolor = 1\n"
        )]),
        Err(MapError::Toml { .. })
    ));
}

#[test]
fn refuses_invalid_names_and_wrong_areas() {
    let global = |name: &str| {
        format!("[global.\"{name}\"]\nlocator = {{ signature = \"90\" }}\ntype = \"ptr\"\n")
    };
    for name in [
        "game",
        "Game.thing",
        "game..thing",
        "game.2thing",
        "game.thing-name",
        "game.thing.",
    ] {
        assert!(
            matches!(
                map(&[("entries/game.toml", &global(name))]),
                Err(MapError::InvalidName { .. })
            ),
            "{name}"
        );
    }
    assert!(matches!(
        map(&[("entries/game.toml", &global("other.thing"))]),
        Err(MapError::WrongArea { name, .. }) if name == "other.thing"
    ));
    assert!(matches!(
        map(&[
            ("entries/game.toml", &global("game.thing")),
            ("entries/game/nested.toml", "")
        ]),
        Err(MapError::UnexpectedFile { .. })
    ));
}

#[test]
fn refuses_a_duplicate_name_across_tables() {
    let text = "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n\
                [location.\"game.a\"]\nlocator = { signature = \"90\" }\nthread = \"any\"\n";
    assert_eq!(
        map(&[("entries/game.toml", text)]).unwrap_err(),
        MapError::Duplicate {
            name: "game.a".to_owned()
        }
    );
}

#[test]
fn refuses_fields_that_do_not_fit_the_kind() {
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\nthread = \"any\"\n"
        ),
        "`thread` does not apply to a global"
    );
    assert_eq!(
        entry_error("[global.\"game.a\"]\ntype = \"ptr\"\n"),
        "`locator` is required"
    );
    assert_eq!(
        entry_error("[function.\"game.a\"]\nlocator = { signature = \"90\" }\nthread = \"any\"\n"),
        "`abi` is required"
    );
    assert_eq!(
        entry_error(
            "[location.\"game.a\"]\nlocator = { signature = \"90\" }\nthread = \"render\"\n"
        ),
        "`thread` is `main` or `any`, not `render`"
    );
    assert_eq!(
        entry_error(
            "[function.\"game.a\"]\nlocator = { signature = \"90\" }\nthread = \"any\"\nabi = { args = [\"void\"], ret = \"void\" }\n"
        ),
        "an argument cannot be `void`"
    );
    assert_eq!(
        entry_error("[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"void\"\n"),
        "`type` cannot be `void`"
    );
    assert_eq!(
        entry_error("[vtable.\"game.a\"]\nlocator = { signature = \"90\" }\nslots = 0\n"),
        "`slots` must be at least 1"
    );
    assert_eq!(
        entry_error(
            "[patch.\"game.a\"]\nlocator = { signature = \"90\" }\noriginal = \"74 05\"\nreplacement = \"EB\"\n"
        ),
        "`original` and `replacement` must have the same length"
    );
    assert_eq!(
        entry_error(
            "[patch.\"game.a\"]\nlocator = { signature = \"90\" }\noriginal = \"74 ??\"\nreplacement = \"EB 05\"\n"
        ),
        "`original` is hexadecimal bytes separated by spaces"
    );
}

#[test]
fn refuses_locators_that_do_not_fit() {
    assert_eq!(
        entry_error("[global.\"game.a\"]\nlocator = { at = 1 }\ntype = \"ptr\"\n"),
        "a locator needs `signature`, `vtable`, or `in`"
    );
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]\nlocator = { signature = \"90\", follow = \"rel8\" }\ntype = \"ptr\"\n"
        ),
        "`follow` is `rel32`, not `rel8`"
    );
    assert_eq!(
        entry_error("[global.\"game.a\"]\nlocator = { signature = \"9\" }\ntype = \"ptr\"\n"),
        "signature: token 0 (`9`) is neither two hexadecimal digits nor `??`"
    );
    assert_eq!(
        entry_error("[global.\"game.a\"]\nlocator = { vtable = \"game.v\" }\ntype = \"ptr\"\n"),
        "`slot` is required"
    );
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]\nlocator = { in = \"game.f\", signature = \"90\", operand = \"disp\" }\ntype = \"ptr\"\n"
        ),
        "an `in` locator locates only a field"
    );
    assert_eq!(
        entry_error(
            "[field.\"game.a\"]\nof = \"game\"\ntype = \"u32\"\nlocator = { signature = \"90\" }\n"
        ),
        "the locator of a field is an `in` locator"
    );
    assert_eq!(
        entry_error(
            "[field.\"game.a\"]\nof = \"game\"\ntype = \"u32\"\nlocator = { in = \"game.f\", signature = \"90\" }\n"
        ),
        "an `in` locator needs `operand = \"disp\"`"
    );
    assert_eq!(
        entry_error(
            "[field.\"game.a\"]\nof = \"game\"\ntype = \"u32\"\noffset = 1\nlocator = { in = \"game.f\", signature = \"90\", operand = \"disp\" }\n"
        ),
        "a field has either `locator` or `offset`, not both"
    );
    assert_eq!(
        entry_error("[field.\"game.a\"]\nof = \"game\"\ntype = \"u32\"\n"),
        "a field needs `locator` or `offset`"
    );
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\nchecks = [{ equals = \"game.b\", signature = \"90\" }]\n"
        ),
        "check 0: a check is a locator, `equals`, `vtable_slot` with `code_slots`, or `contains`"
    );
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]
locator = { signature = \"90\" }
type = \"ptr\"
checks = [{ contains = \"90\", signature = \"90\" }]
"
        ),
        "check 0: `contains` takes only `in`"
    );
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]
locator = { signature = \"90\" }
type = \"ptr\"
checks = [{ contains = \"90\" }]
"
        ),
        "a `contains` check of an entry that is not code gives `in`"
    );
    let not_function = "[global.\"game.g\"]
locator = { signature = \"90\" }
type = \"ptr\"
                        [global.\"game.a\"]
locator = { signature = \"90\" }
type = \"ptr\"
checks = [{ contains = \"90\", in = \"game.g\" }]
";
    assert!(
        matches!(map(&[("entries/game.toml", not_function)]), Err(MapError::BadReference { problem, .. }) if problem == "not a function")
    );
    assert_eq!(
        entry_error(
            "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\nchecks = [{ vtable_slot = 3, code_slots = 3 }]\n"
        ),
        "check 0: `code_slots` must be greater than `vtable_slot`"
    );
}

#[test]
fn refuses_bad_references() {
    let vtable = "[vtable.\"game.v\"]\nlocator = { signature = \"90\" }\nslots = 2\n";
    let undefined = format!(
        "{vtable}[global.\"game.a\"]\nlocator = {{ signature = \"90\" }}\ntype = \"ptr\"\ndepends = [\"game.missing\"]\n"
    );
    assert!(
        matches!(map(&[("entries/game.toml", &undefined)]), Err(MapError::BadReference { problem, .. }) if problem == "not defined")
    );

    let slot = format!(
        "{vtable}[function.\"game.f\"]\nlocator = {{ vtable = \"game.v\", slot = 2 }}\nabi = {{ args = [], ret = \"void\" }}\nthread = \"any\"\n"
    );
    assert!(
        matches!(map(&[("entries/game.toml", &slot)]), Err(MapError::BadReference { problem, .. }) if problem == "a vtable without that slot")
    );

    let not_vtable = "[global.\"game.g\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n\
                      [function.\"game.f\"]\nlocator = { vtable = \"game.g\", slot = 0 }\nabi = { args = [], ret = \"void\" }\nthread = \"any\"\n";
    assert!(
        matches!(map(&[("entries/game.toml", not_vtable)]), Err(MapError::BadReference { problem, .. }) if problem == "not a vtable")
    );

    let not_function = format!(
        "{vtable}[field.\"game.x\"]\nof = \"game\"\ntype = \"u32\"\nlocator = {{ in = \"game.v\", signature = \"90\", operand = \"disp\" }}\n"
    );
    assert!(
        matches!(map(&[("entries/game.toml", &not_function)]), Err(MapError::BadReference { problem, .. }) if problem == "not a function")
    );
}

#[test]
fn refuses_a_cycle() {
    let text = "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\ndepends = [\"game.b\"]\n\
                [global.\"game.b\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\nchecks = [{ equals = \"game.a\" }]\n";
    assert_eq!(
        map(&[("entries/game.toml", text)]).unwrap_err(),
        MapError::Cycle {
            names: vec![
                "game.a".to_owned(),
                "game.b".to_owned(),
                "game.a".to_owned()
            ]
        }
    );
}

#[test]
fn hashes_the_map_files_in_path_order() {
    let a = (
        "entries/a.toml",
        "[global.\"a.x\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n",
    );
    let b = (
        "entries/b.toml",
        "[global.\"b.x\"]\nlocator = { signature = \"91\" }\ntype = \"ptr\"\n",
    );
    let forward = map(&[a, b]).unwrap();
    let backward = map(&[b, a]).unwrap();
    assert_eq!(forward.entries_sha256(), backward.entries_sha256());
    assert_eq!(forward.entries_sha256().len(), 64);

    let changed = map(&[
        a,
        (
            "entries/b.toml",
            "[global.\"b.x\"]\nlocator = { signature = \"92\" }\ntype = \"ptr\"\n",
        ),
    ])
    .unwrap();
    assert_ne!(forward.entries_sha256(), changed.entries_sha256());
    assert_ne!(
        map(&[]).unwrap().entries_sha256(),
        Map::from_sources("format = 2\n\n", &[])
            .unwrap()
            .entries_sha256()
    );
}

#[test]
fn loads_a_map_directory() {
    let directory = tempfile::tempdir().unwrap();
    // A path with Cyrillic and a space, as under a Russian user profile.
    let root = directory.path().join("карта игры");
    std::fs::create_dir_all(root.join("entries")).unwrap();
    std::fs::write(root.join("map.toml"), MAP).unwrap();
    std::fs::write(
        root.join("entries/game.toml"),
        "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n",
    )
    .unwrap();
    let loaded = Map::load(&root).unwrap();
    assert!(loaded.get("game.a").is_some());
    assert_eq!(
        loaded.entries_sha256(),
        map(&[(
            "entries/game.toml",
            "[global.\"game.a\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n"
        )])
        .unwrap()
        .entries_sha256()
    );

    std::fs::write(root.join("entries/notes.md"), "").unwrap();
    assert!(matches!(
        Map::load(&root),
        Err(MapError::UnexpectedFile { .. })
    ));

    let missing = Map::load(&directory.path().join("missing")).unwrap_err();
    assert_eq!(missing.code(), "io");
    assert!(missing.path().is_some());
}
