//! The conversion from game map v2 loses nothing: converting the converted
//! description back gives the same map, as the v2 reader sees it.

use std::collections::BTreeMap;
use std::path::Path;

use angelica_anatomia::{Ty, convert_map};
use angelica_map::Map;
use toml::{Table, Value};

fn fixture() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/map"))
}

fn table(text: &str) -> Table {
    text.parse().unwrap()
}

fn get<'a>(table: &'a Table, key: &str) -> Option<&'a Value> {
    table.get(key)
}

/// Puts `value` at `path` of nested tables, creating them.
fn insert(root: &mut Table, path: &[&str], value: Value) {
    let (last, parents) = path.split_last().unwrap();
    let mut table = root;
    for key in parents {
        table = table
            .entry((*key).to_owned())
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .unwrap();
    }
    table.insert((*last).to_owned(), value);
}

/// The v2 locator a `find` pipeline came from.
fn locator(steps: &Value, old: &dyn Fn(&str) -> String) -> Value {
    let steps: Vec<&Table> = steps
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step.as_table().unwrap())
        .collect();
    let mut out = Table::new();
    match steps.as_slice() {
        [function, scan, operand] if function.contains_key("in") => {
            out.insert(
                "in".into(),
                Value::String(old(function["in"].as_str().unwrap())),
            );
            out.insert("signature".into(), scan["scan"].clone());
            out.insert("operand".into(), operand["operand"].clone());
        }
        [slot] if slot.contains_key("vtable") => {
            out.insert(
                "vtable".into(),
                Value::String(old(slot["vtable"].as_str().unwrap())),
            );
            out.insert("slot".into(), slot["slot"].clone());
        }
        [scan] => {
            out.insert("signature".into(), scan["scan"].clone());
        }
        [scan, next] if next.contains_key("rel32") => {
            out.insert("signature".into(), scan["scan"].clone());
            out.insert("at".into(), next["rel32"].clone());
            out.insert("follow".into(), Value::String("rel32".into()));
        }
        [scan, next] if next.contains_key("offset") => {
            out.insert("signature".into(), scan["scan"].clone());
            out.insert("at".into(), next["offset"].clone());
        }
        other => panic!("no v2 locator for {other:?}"),
    }
    Value::Table(out)
}

fn v2_kind(ty: &Ty) -> String {
    match ty {
        Ty::Primitive(primitive) => primitive.name().to_owned(),
        Ty::Ptr(_) => "ptr".to_owned(),
        _ => "struct".to_owned(),
    }
}

/// The common keys of an entry: locator, checks, depends, origin.
fn location(entry: &Table, v2: &mut Table, old: &dyn Fn(&str) -> String) {
    if let Some(find) = get(entry, "find") {
        v2.insert("locator".into(), locator(find, old));
    }
    if let Some(at) = get(entry, "at") {
        v2.insert("offset".into(), at.clone());
    }
    if let Some(checks) = get(entry, "checks") {
        let checks = checks
            .as_array()
            .unwrap()
            .iter()
            .map(|check| {
                let check = check.as_table().unwrap();
                let mut out = Table::new();
                if let Some(find) = check.get("find") {
                    return locator(find, old);
                }
                for (key, value) in check {
                    let value = match (key.as_str(), value) {
                        ("in" | "equals", Value::String(name)) => Value::String(old(name)),
                        _ => value.clone(),
                    };
                    out.insert(key.clone(), value);
                }
                Value::Table(out)
            })
            .collect();
        v2.insert("checks".into(), Value::Array(checks));
    }
    if let Some(depends) = get(entry, "depends") {
        let names = depends
            .as_array()
            .unwrap()
            .iter()
            .map(|name| Value::String(old(name.as_str().unwrap())))
            .collect();
        v2.insert("depends".into(), Value::Array(names));
    }
    if let Some(origin) = entry.get("ext").and_then(|ext| ext.get("angelica.origin")) {
        v2.insert("origin".into(), origin.clone());
    }
}

/// Converts a description and its Lens catalog back to a v2 map.
#[allow(clippy::too_many_lines)]
fn to_v2(files: &BTreeMap<String, String>) -> Map {
    let parsed: BTreeMap<&str, Table> = files
        .iter()
        .filter(|(path, _)| {
            Path::new(path.as_str())
                .extension()
                .is_some_and(|extension| extension == "toml")
                && !path.starts_with("sheets/")
                && !path.starts_with("windows/")
        })
        .map(|(path, text)| (path.as_str(), table(text)))
        .collect();
    let of_kind = |folder: &str, kind: &str| -> Vec<(String, Table)> {
        parsed
            .iter()
            .filter(|(path, _)| path.starts_with(folder))
            .flat_map(|(_, file)| {
                file.get(kind)
                    .and_then(Value::as_table)
                    .into_iter()
                    .flatten()
                    .map(|(name, value)| (name.clone(), value.as_table().unwrap().clone()))
            })
            .collect()
    };
    let structs = of_kind("types/", "struct");
    let catalog = |file: &str, kind: &str| -> Table {
        parsed
            .get(format!("lens/{file}.toml").as_str())
            .and_then(|file| file.get(kind))
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default()
    };
    let offered = catalog("types", "types");
    let changes = catalog("changes", "changes");
    let meaning_of = |kind: &str| -> Table {
        let mut all = Table::new();
        for (path, file) in &parsed {
            if path.starts_with("meaning/")
                && let Some(items) = file.get(kind).and_then(Value::as_table)
            {
                all.extend(items.clone());
            }
        }
        all
    };
    let field_meaning = meaning_of("fields");
    let type_meaning = meaning_of("types");

    // The v2 name of each field: its alias, or its own.
    let mut old_names: BTreeMap<String, String> = BTreeMap::new();
    for (structure, definition) in &structs {
        for (field, value) in definition
            .get("fields")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            let full = format!("{structure}.{field}");
            if let Some(alias) = value
                .get("aliases")
                .and_then(Value::as_array)
                .and_then(|aliases| aliases.first())
            {
                let alias = alias.as_str().unwrap();
                let old = if alias.contains('.') {
                    alias.to_owned()
                } else {
                    format!("{structure}.{alias}")
                };
                old_names.insert(full, old);
            }
        }
    }
    let old = |name: &str| {
        old_names
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    };
    let area = |name: &str| name.split('.').next().unwrap().to_owned();

    let mut entries: BTreeMap<String, Table> = BTreeMap::new();
    let mut put = |name: &str, kind: &str, entry: Table| {
        insert(
            entries.entry(area(name)).or_default(),
            &[kind, name],
            Value::Table(entry),
        );
    };
    for (name, function) in of_kind("functions/", "function") {
        let Ty::Fn(ty) = Ty::parse(function["type"].as_str().unwrap()).unwrap() else {
            panic!()
        };
        let mut entry = Table::new();
        let mut abi = Table::new();
        abi.insert(
            "args".into(),
            Value::Array(
                ty.params
                    .iter()
                    .map(|param| Value::String(param.ty.to_string()))
                    .collect(),
            ),
        );
        abi.insert(
            "ret".into(),
            Value::String(ty.ret.map_or("void".to_owned(), |ret| ret.to_string())),
        );
        entry.insert("abi".into(), Value::Table(abi));
        entry.insert("thread".into(), function["thread"].clone());
        entry.extend(
            function
                .get("doc")
                .map(|doc| ("doc".to_owned(), doc.clone())),
        );
        location(&function, &mut entry, &old);
        put(&name, "function", entry);
    }
    for (name, found) in of_kind("functions/", "location") {
        let mut entry = Table::new();
        entry.insert("thread".into(), found["thread"].clone());
        entry.extend(found.get("doc").map(|doc| ("doc".to_owned(), doc.clone())));
        location(&found, &mut entry, &old);
        put(&name, "location", entry);
    }
    for (name, global) in of_kind("globals/", "global") {
        let mut entry = Table::new();
        let ty = Ty::parse(global["type"].as_str().unwrap()).unwrap();
        entry.insert("type".into(), Value::String(v2_kind(&ty)));
        entry.extend(global.get("doc").map(|doc| ("doc".to_owned(), doc.clone())));
        location(&global, &mut entry, &old);
        put(&name, "global", entry);
    }
    for (name, vtable) in of_kind("globals/", "vtable") {
        let mut entry = Table::new();
        entry.insert("slots".into(), vtable["code_slots"].clone());
        entry.extend(vtable.get("doc").map(|doc| ("doc".to_owned(), doc.clone())));
        location(&vtable, &mut entry, &old);
        put(&name, "vtable", entry);
    }

    let mut types: BTreeMap<String, Table> = BTreeMap::new();
    for (structure, definition) in &structs {
        let fields = definition
            .get("fields")
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();
        let lens_fields: Vec<String> = offered
            .get(structure)
            .and_then(|offered| offered.get("fields"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|field| field.as_str().unwrap().to_owned())
            .collect();
        let in_model = offered.contains_key(structure);
        for (field, value) in &fields {
            let value = value.as_table().unwrap();
            let full = format!("{structure}.{field}");
            let ty = Ty::parse(value["type"].as_str().unwrap()).unwrap();
            let offered = lens_fields.contains(field);
            let entry_doc = value
                .get("ext")
                .and_then(|ext| ext.get("angelica.notes"))
                .and_then(|notes| notes.get("entry_doc"));
            if !value.contains_key("bits") {
                let mut entry = Table::new();
                entry.insert("of".into(), Value::String(structure.clone()));
                entry.insert("type".into(), Value::String(v2_kind(&ty)));
                let doc = if offered { entry_doc } else { value.get("doc") };
                entry.extend(doc.map(|doc| ("doc".to_owned(), doc.clone())));
                location(value, &mut entry, &old);
                put(&old(&full), "field", entry);
            }
            if !offered {
                continue;
            }
            // The object model's field.
            let mut object = Table::new();
            object.extend(value.get("doc").map(|doc| ("doc".to_owned(), doc.clone())));
            let at = match value.get("bits") {
                Some(bits) => {
                    object.insert("bit".into(), bits["at"].clone());
                    old(&format!("{structure}.{}", bits["in"].as_str().unwrap()))
                }
                None => old(&full),
            };
            if at != full {
                object.insert("at".into(), Value::String(at));
            }
            for key in ["sheet", "enum", "flags", "element", "unit"] {
                object.extend(value.get(key).map(|found| (key.to_owned(), found.clone())));
            }
            let meaning = field_meaning
                .get(&full)
                .and_then(Value::as_table)
                .cloned()
                .unwrap_or_default();
            for key in ["domain", "forbidden", "whose", "as"] {
                object.extend(
                    meaning
                        .get(key)
                        .map(|found| (key.to_owned(), found.clone())),
                );
            }
            let array = |element: &Ty, length: u32| -> Value {
                let mut array = Table::new();
                let (of, pointers) = match element {
                    Ty::Ptr(Some(inner)) => (inner.to_string(), true),
                    other => (other.to_string(), false),
                };
                array.insert("of".into(), Value::String(of.clone()));
                match value.get("count") {
                    Some(count) => {
                        array.insert("count".into(), count.clone());
                        array.insert("most".into(), Value::Integer(i64::from(length)));
                    }
                    None => {
                        array.insert("length".into(), Value::Integer(i64::from(length)));
                    }
                }
                let stride = if pointers {
                    8
                } else {
                    structs.iter().find(|(name, _)| *name == of).unwrap().1["size"]
                        .as_integer()
                        .unwrap()
                };
                array.insert("stride".into(), Value::Integer(stride));
                if pointers {
                    array.insert("pointers".into(), Value::Boolean(true));
                }
                array.extend(
                    value
                        .get("present")
                        .map(|present| ("present".to_owned(), present.clone())),
                );
                array.extend(
                    meaning
                        .get("partition")
                        .map(|partition| ("partition".to_owned(), partition.clone())),
                );
                Value::Table(array)
            };
            match (&ty, value.get("text")) {
                (Ty::Array(_, length), Some(Value::Boolean(true))) => {
                    object.insert("text".into(), Value::Integer(i64::from(*length)));
                }
                (Ty::Ptr(_), Some(most)) => {
                    object.insert("text".into(), most.clone());
                }
                (Ty::Named(holder, _), Some(most)) => {
                    let holder_definition =
                        &structs.iter().find(|(name, _)| name == holder).unwrap().1;
                    let through = holder_definition["text"].as_str().unwrap();
                    object.insert(
                        "pointer".into(),
                        Value::String(old(&format!("{holder}.{through}"))),
                    );
                    object.insert("text".into(), most.clone());
                }
                (Ty::Array(element, length), None) => {
                    object.insert("array".into(), array(element, *length));
                }
                (Ty::Ptr(Some(inner)), None) => match inner.as_ref() {
                    Ty::Array(element, length) => {
                        object.insert("array".into(), array(element, *length));
                    }
                    target => {
                        object.insert("ref".into(), Value::String(target.to_string()));
                    }
                },
                (Ty::Named(target, _), None) => {
                    object.insert("embed".into(), Value::String(target.clone()));
                }
                _ => {}
            }
            match changes.get(&full).and_then(Value::as_table) {
                Some(set) if set.is_empty() => {
                    object.insert("set".into(), Value::Boolean(true));
                }
                Some(set) => {
                    object.insert("set".into(), Value::Table(set.clone()));
                }
                None => {}
            }
            insert(
                types.entry(area(structure)).or_default(),
                &["types", structure, "fields", field],
                Value::Table(object),
            );
        }
        if in_model {
            let target = types.entry(area(structure)).or_default();
            let path = ["types", structure.as_str()];
            insert(
                target,
                &[path[0], path[1], "doc"],
                definition["doc"].clone(),
            );
            if let Some(domain) = type_meaning
                .get(structure)
                .and_then(|meaning| meaning.get("domain"))
            {
                insert(target, &[path[0], path[1], "domain"], domain.clone());
            }
            if let Some(base) = definition
                .get("extends")
                .and_then(Value::as_array)
                .and_then(|bases| bases.first())
            {
                insert(target, &[path[0], path[1], "extends"], base["type"].clone());
            }
            if let Some(variant) = definition.get("variant") {
                insert(target, &[path[0], path[1], "variant"], variant.clone());
            }
        }
    }

    let mut sources: Vec<(String, String)> = Vec::new();
    for (area, file) in entries {
        sources.push((
            format!("entries/{area}.toml"),
            toml::to_string(&file).unwrap(),
        ));
    }
    for (area, file) in types {
        sources.push((
            format!("types/{area}.toml"),
            toml::to_string(&file).unwrap(),
        ));
    }
    for (name, definition) in of_kind("types/", "enum") {
        sources.push((
            format!("enums/{name}.toml"),
            toml::to_string(&definition).unwrap(),
        ));
    }
    let domains = meaning_of("domains");
    if !domains.is_empty() {
        let mut file = Table::new();
        file.insert("domains".into(), Value::Table(domains));
        sources.push(("domains.toml".into(), toml::to_string(&file).unwrap()));
    }
    for file in ["roots", "events", "actions"] {
        if let Some(text) = files.get(&format!("lens/{file}.toml")) {
            sources.push((format!("{file}.toml"), text.clone()));
        }
    }
    for (path, text) in files {
        if path.starts_with("sheets/") || path.starts_with("windows/") {
            sources.push((path.clone(), text.clone()));
        }
    }
    Map::from_sources("format = 2\n", &sources).unwrap_or_else(|error| panic!("{error}"))
}

fn assert_round_trip(directory: &Path) {
    let original = Map::load(directory).unwrap();
    let converted = convert_map(directory).unwrap();
    let back = to_v2(&converted.files);
    let entries = |map: &Map| map.entries().cloned().collect::<Vec<_>>();
    assert_eq!(entries(&back), entries(&original));
    assert_eq!(back.objects(), original.objects());
    assert_eq!(
        back.sheets().collect::<Vec<_>>(),
        original.sheets().collect::<Vec<_>>()
    );
    assert_eq!(
        back.windows().collect::<Vec<_>>(),
        original.windows().collect::<Vec<_>>()
    );
}

#[test]
fn the_fixture_converts_back_to_the_same_map() {
    assert_round_trip(fixture());
}

#[test]
fn the_converted_fixture_uses_everything() {
    let converted = convert_map(fixture()).unwrap();
    let sources: Vec<(String, String)> = converted
        .files
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
    let anatomia =
        angelica_anatomia::Anatomia::from_sources(&converted.files["anatomia.toml"], &sources)
            .unwrap();
    assert!(anatomia.broken().is_empty(), "{:?}", anatomia.broken());
    assert_eq!(
        converted.renamed,
        BTreeMap::from([
            (
                "game.helper.free".to_owned(),
                "game.helper_vtable.free".to_owned()
            ),
            (
                "game.thing.raw_flags".to_owned(),
                "game.thing.flags".to_owned()
            ),
            (
                "game.thing.raw_value".to_owned(),
                "game.thing.value".to_owned()
            ),
            (
                "game.thing.slots_raw".to_owned(),
                "game.thing.slots".to_owned()
            ),
        ])
    );
    // The old names still find the fields.
    assert_eq!(
        anatomia.canonical("game.helper.free"),
        "game.helper_vtable.free"
    );
    assert_eq!(
        anatomia.canonical("game.thing.raw_flags"),
        "game.thing.flags"
    );
    let text = &converted.files["types/game.toml"];
    assert!(
        text.contains("type = \"ptr<ptr<game.thing>[16]>\"\nat = 0x100\ncount = \"count\""),
        "{text}"
    );
    assert!(text.contains("type = \"game.slot[8]\""), "{text}");
    assert!(text.contains("bits = { at = 2, in = \"flags\" }"), "{text}");
    assert!(
        text.contains("[struct.\"game.text\"]\ndoc = \"Text.\"\ntext = \"bytes\""),
        "{text}"
    );
}

#[test]
fn a_patch_has_no_place() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("map.toml"), "format = 2\n").unwrap();
    std::fs::create_dir(directory.path().join("entries")).unwrap();
    std::fs::write(
        directory.path().join("entries/game.toml"),
        "[patch.\"game.thing.nop\"]\nlocator = { signature = \"90 90\" }\noriginal = \"90 90\"\nreplacement = \"CC CC\"\n",
    )
    .unwrap();
    let error = convert_map(directory.path()).unwrap_err();
    assert!(error.contains("`patch`"), "{error}");
}

/// The published game map, when `ANGELICA_GAMEMAP` names its directory.
#[test]
#[ignore = "needs ANGELICA_GAMEMAP, a checkout of angelica-gamemap"]
fn the_game_map_converts_back_to_the_same_map() {
    let directory = std::env::var("ANGELICA_GAMEMAP").expect("ANGELICA_GAMEMAP");
    assert_round_trip(Path::new(&directory));
}

/// Every file under `directory`, by its path relative to it.
fn files_in(directory: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut folders = vec![directory.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for item in std::fs::read_dir(&folder).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let relative = path.strip_prefix(directory).unwrap();
                let relative = relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                files.push((relative, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    files
}

#[test]
fn a_map_given_as_files_converts_as_its_folder_does() {
    let mut files = files_in(fixture());
    let map_toml = files
        .iter()
        .position(|(path, _)| path == "map.toml")
        .map(|at| files.remove(at).1)
        .unwrap();
    let given = angelica_anatomia::convert_sources(&map_toml, &files).unwrap();
    assert_eq!(given, convert_map(fixture()).unwrap());
    assert!(given.anatomia().unwrap().broken().is_empty());
}
