use super::*;
use crate::error::Kind;
use angelica_map::{EntryKind, Map, Value as Resolved};

/// Memory of one buffer at address 0x1000.
struct Buffer(Vec<u8>);

impl Memory for Buffer {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let Some(start) = address
            .checked_sub(0x1000)
            .and_then(|a| usize::try_from(a).ok())
        else {
            return false;
        };
        match self.0.get(start..start + buffer.len()) {
            Some(bytes) => {
                buffer.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

const ENTRIES: &str = r#"
[global."test.manager"]
locator = { signature = "C3" }
type = "struct"

[global."test.player"]
locator = { signature = "C3" }
type = "ptr"

[field."test.manager.containers"]
of = "test.manager"
type = "ptr"
offset = 0x08

[field."test.container.items"]
of = "test.container"
type = "ptr"
offset = 0x00

[field."test.container.kind"]
of = "test.container"
type = "u32"
offset = 0x08

[field."test.container.size"]
of = "test.container"
type = "i32"
offset = 0x0C

[field."test.item.item"]
of = "test.item"
type = "u32"
offset = 0x00

[field."test.item.quantity"]
of = "test.item"
type = "u32"
offset = 0x04

[field."test.item.high_quality"]
of = "test.item"
type = "bool"
offset = 0x08

[field."test.item.flags"]
of = "test.item"
type = "u8"
offset = 0x09

[field."test.item.secret"]
of = "test.item"
type = "u8"
offset = 0x0A

[field."test.character.name"]
of = "test.character"
type = "struct"
offset = 0x00

[field."test.character.target"]
of = "test.character"
type = "ptr"
offset = 0x10

[field."test.character.data"]
of = "test.character"
type = "struct"
offset = 0x20

[field."test.character.missing"]
of = "test.character"
type = "u8"
offset = 0x30

[field."test.data.level"]
of = "test.data"
type = "u8"
offset = 0x00

[field."test.data.health"]
of = "test.data"
type = "i32"
offset = 0x04

[field."test.data.speed"]
of = "test.data"
type = "f32"
offset = 0x08
"#;

const TYPES: &str = r#"
[types."test.manager"]
doc = "Every container."
domain = "inventory"

[types."test.manager".fields.containers]
doc = "The containers."
array = { of = "test.container", length = 4, stride = 24 }

[types."test.container"]
doc = "A container."
domain = "inventory"

[types."test.container".fields.kind]
doc = "Which."
enum = "test.kind"

[types."test.container".fields.size]
doc = "Slots."

[types."test.container".fields.items]
doc = "The slots."
array = { of = "test.item", count = "size", most = 8, stride = 16, present = "item" }

[types."test.item"]
doc = "A slot."
domain = "inventory"

[types."test.item".fields.item]
doc = "The item."
sheet = "Item"

[types."test.item".fields.quantity]
doc = "How many."

[types."test.item".fields.high_quality]
doc = "High quality."

[types."test.item".fields.flags]
doc = "Marks."
flags = "test.marks"

[types."test.item".fields.secret]
doc = "Never read."
forbidden = true

[types."test.character"]
doc = "A character."
domain = "character"

[types."test.character".fields.name]
doc = "The name."
text = 16

[types."test.character".fields.target]
doc = "Its target."
ref = "test.character"

[types."test.character".fields.data]
doc = "Its numbers."
embed = "test.data"

[types."test.character".fields.missing]
doc = "Absent in the build."

[types."test.data"]
doc = "Numbers."
domain = "character"

[types."test.data".fields.level]
doc = "Level."

[types."test.data".fields.health]
doc = "HP."

[types."test.data".fields.speed]
doc = "Speed."
unit = "yalms/s"
"#;

const DOMAINS: &str = r#"
[domains.inventory]
doc = "Items."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }

[domains.character]
doc = "The character."
class = "you"
read = { en = "Sees your character", ru = "Видит вашего персонажа" }
"#;

const ROOTS: &str = r#"
[roots.inventory]
doc = "Items."
global = "test.manager"
type = "test.manager"

[roots.player]
doc = "The player."
global = "test.player"
type = "test.character"
"#;

fn schema() -> Schema {
    schema_of(
        TYPES,
        DOMAINS,
        "doc = \"Kinds.\"\nvalues = { bag_1 = 0, bag_2 = 1, crystals = 2001 }\n",
    )
    .unwrap()
}

fn schema_of(
    types: &str,
    domains: &str,
    kinds: &str,
) -> std::result::Result<Schema, angelica_map::MapError> {
    let map = Map::from_sources(
        "format = 2",
        &[
            ("entries/test.toml".into(), ENTRIES.into()),
            ("types/test.toml".into(), types.into()),
            ("domains.toml".into(), domains.into()),
            ("roots.toml".into(), ROOTS.into()),
            ("enums/test.kind.toml".into(), kinds.into()),
            (
                "enums/test.marks.toml".into(),
                "doc = \"Marks.\"\nflags = true\nvalues = { new = 1, glamoured = 4 }\n".into(),
            ),
        ],
    )?;
    let entries = map.clone();
    Ok(Schema::new(&map, 0x800, move |name| {
        if name == "test.character.missing" {
            return Err("no match".into());
        }
        match &entries.get(name).unwrap().kind {
            EntryKind::Field { offset, .. } => Ok(Resolved::Offset(offset.unwrap())),
            // The manager is at 0x1000, the player's pointer at 0x1040.
            _ if name == "test.manager" => Ok(Resolved::Address(0x800)),
            _ => Ok(Resolved::Address(0x840)),
        }
    }))
}

fn put(bytes: &mut [u8], at: u64, value: &[u8]) {
    let at = usize::try_from(at - 0x1000).unwrap();
    bytes[at..at + value.len()].copy_from_slice(value);
}

/// The manager at 0x1000 with its containers at 0x1100: bag 1 with three
/// slots at 0x1200 (the second empty), bag 2 with one at 0x1300, and the
/// crystals without items. The player at 0x1800 targets the character at
/// 0x1900.
fn memory(player: bool) -> Buffer {
    let mut bytes = vec![0u8; 0x1000];
    put(&mut bytes, 0x1008, &0x1100u64.to_le_bytes());
    for (index, items, kind, size) in [
        (0u64, 0x1200u64, 0u32, 3i32),
        (1, 0x1300, 1, 1),
        (2, 0, 2001, 0),
        (3, 0, 7, -5),
    ] {
        let at = 0x1100 + index * 24;
        put(&mut bytes, at, &items.to_le_bytes());
        put(&mut bytes, at + 8, &kind.to_le_bytes());
        put(&mut bytes, at + 12, &size.to_le_bytes());
    }
    for (at, item, quantity, high_quality, flags) in [
        (0x1200u64, 5057u32, 12u32, 0u8, 5u8),
        (0x1210, 0, 0, 0, 0),
        (0x1220, 4, 999, 1, 0),
        (0x1300, 5057, 3, 1, 2),
    ] {
        put(&mut bytes, at, &item.to_le_bytes());
        put(&mut bytes, at + 4, &quantity.to_le_bytes());
        put(&mut bytes, at + 8, &[high_quality, flags]);
    }
    if player {
        put(&mut bytes, 0x1040, &0x1800u64.to_le_bytes());
    }
    put(&mut bytes, 0x1800, b"Alisae");
    put(&mut bytes, 0x1810, &0x1900u64.to_le_bytes());
    put(&mut bytes, 0x1820, &[90]);
    put(&mut bytes, 0x1824, &(-1i32).to_le_bytes());
    put(&mut bytes, 0x1828, &6.5f32.to_le_bytes());
    put(&mut bytes, 0x1900, b"Alphinaud");
    put(&mut bytes, 0x1920, &[70]);
    Buffer(bytes)
}

fn all(_: &str) -> bool {
    true
}

fn rows(text: &str) -> Vec<Row> {
    let prepared = prepare(&schema(), text, &all).unwrap();
    run(&memory(true), &prepared).unwrap()
}

fn error(text: &str, permitted: &dyn Fn(&str) -> bool) -> Error {
    prepare(&schema(), text, permitted).unwrap_err()
}

fn item(row: u32) -> Value {
    Value::Row {
        sheet: "Item".into(),
        row,
    }
}

#[test]
fn arrays_on_the_path_become_rows_with_their_positions() {
    let rows =
        rows("inventory.containers[kind in (bag_1, bag_2)].items { item, quantity, high_quality }");
    assert_eq!(
        rows,
        [
            Row {
                indexes: vec![0, 0],
                values: vec![
                    Some(item(5057)),
                    Some(Value::Unsigned(12)),
                    Some(Value::Boolean(false))
                ],
            },
            Row {
                indexes: vec![0, 2],
                values: vec![
                    Some(item(4)),
                    Some(Value::Unsigned(999)),
                    Some(Value::Boolean(true))
                ],
            },
            Row {
                indexes: vec![1, 0],
                values: vec![
                    Some(item(5057)),
                    Some(Value::Unsigned(3)),
                    Some(Value::Boolean(true))
                ],
            },
        ],
        "the empty slot is skipped by `present`, the negative size counts none"
    );
}

#[test]
fn filters_compare_numbers_names_and_text() {
    let containers = |filter: &str| -> Vec<Vec<u32>> {
        rows(&format!("inventory.containers[{filter}] {{ kind }}"))
            .into_iter()
            .map(|row| row.indexes)
            .collect()
    };
    assert_eq!(containers("kind = crystals"), [vec![2]]);
    assert_eq!(
        containers("kind != bag_1 and size >= 0"),
        [vec![1], vec![2]]
    );
    assert_eq!(containers("size < 0"), [vec![3]]);
    assert_eq!(containers("kind in (7, 2001)"), [vec![2], vec![3]]);
    assert_eq!(
        rows("inventory.containers[0].items[item = 5057 and high_quality = false] { quantity }")
            .len(),
        1
    );
    assert_eq!(rows("inventory.containers[1].items[1] { quantity }"), []);
    assert_eq!(
        rows("player.target[name = \"Alphinaud\"] { name }").len(),
        1
    );
    assert_eq!(rows("player.target[name != \"Alphinaud\"] { name }"), []);
    assert_eq!(rows("player.data[speed > 6.25] { level }").len(), 1);
}

#[test]
fn values_carry_their_meaning() {
    assert_eq!(
        rows("inventory.containers[2] { kind }")[0].values,
        [Some(Value::Named("crystals".into()))]
    );
    assert_eq!(
        rows("inventory.containers[3] { kind }")[0].values,
        [Some(Value::Unsigned(7))],
        "a value without a name stays a number"
    );
    assert_eq!(
        rows("inventory.containers[0].items[0] { flags }")[0].values,
        [Some(Value::Flags {
            names: vec!["new".into(), "glamoured".into()],
            bits: 5
        })]
    );
    assert_eq!(
        rows("player { name, level: data.level, data.health, data.speed, target.name, target.target.name }")[0]
            .values,
        [
            Some(Value::Text(b"Alisae".to_vec())),
            Some(Value::Unsigned(90)),
            Some(Value::Signed(-1)),
            Some(Value::Float(6.5)),
            Some(Value::Text(b"Alphinaud".to_vec())),
            None,
        ],
        "the target's target pointer is zero"
    );
    assert_eq!(
        rows("player.data.level"),
        [Row {
            indexes: Vec::new(),
            values: vec![Some(Value::Unsigned(90))]
        }]
    );
}

#[test]
fn a_root_whose_pointer_is_zero_answers_nothing() {
    let prepared = prepare(&schema(), "player { name }", &all).unwrap();
    assert_eq!(run(&memory(false), &prepared).unwrap(), []);
}

#[test]
fn columns_name_their_fields() {
    let prepared = prepare(&schema(), "player { lv: data.level, name }", &all).unwrap();
    assert_eq!(prepared.columns[0].name, "lv");
    assert_eq!(prepared.columns[0].field, "test.data.level");
    assert_eq!(prepared.columns[0].doc, "Level.");
    assert_eq!(prepared.columns[1].stored, Stored::Text(16));
    assert_eq!(
        prepared.domains.into_iter().collect::<Vec<_>>(),
        ["character"]
    );
}

#[test]
fn every_domain_on_the_way_needs_permission() {
    let only_character = |domain: &str| domain == "character";
    assert_eq!(
        error("inventory.containers { kind }", &only_character).kind,
        Kind::PermissionDenied
    );
    assert!(prepare(&schema(), "player.target { name }", &only_character).is_ok());
    let error = error("player { name }", &|_: &str| false);
    assert_eq!(error.kind, Kind::PermissionDenied);
    assert!(error.message.contains("[lens] read"), "{}", error.message);
}

#[test]
fn mistakes_are_named_before_anything_is_read() {
    for (text, kind, message) in [
        (
            "party { name }",
            Kind::NotFound,
            "the game map has no root party; its roots: inventory, player",
        ),
        (
            "player.mana",
            Kind::NotFound,
            "test.character has no field mana; its fields: data, missing, name, target",
        ),
        (
            "player.missing",
            Kind::Unavailable,
            "test.character.missing is not available in this game build: map entry test.character.missing is absent: no match",
        ),
        (
            "inventory.containers[0].items[0] { secret }",
            Kind::PermissionDenied,
            "test.item.secret is never read by a plugin",
        ),
        (
            "player",
            Kind::InvalidArgument,
            "the path ends at test.character: name the values to answer with in braces, such as { a, b }; its fields: data, missing, name, target",
        ),
        (
            "player.name.x",
            Kind::InvalidArgument,
            "test.character.name is a value: a path goes on only through references, objects, and arrays",
        ),
        (
            "player[0]",
            Kind::InvalidArgument,
            "a filter follows a field of the root, not the root",
        ),
        (
            "player.target[0] { name }",
            Kind::InvalidArgument,
            "test.character.target is one object, not an array: it has no index",
        ),
        (
            "inventory.containers[4] { kind }",
            Kind::InvalidArgument,
            "test.manager.containers has at most 4 elements: index 4 is past them",
        ),
        (
            "inventory.containers[kind = bag_9] { kind }",
            Kind::InvalidArgument,
            "test.container.kind has no value bag_9; its values: bag_1, bag_2, crystals",
        ),
        (
            "inventory.containers[kind > bag_1] { kind }",
            Kind::InvalidArgument,
            "test.container.kind has no order: compare it with =, !=, or in",
        ),
        (
            "inventory.containers[size = 1.5] { kind }",
            Kind::InvalidArgument,
            "test.container.size is a whole number",
        ),
        (
            "player[name = 3] { name }",
            Kind::InvalidArgument,
            "a filter follows a field of the root, not the root",
        ),
        (
            "player.target[name = 3] { name }",
            Kind::InvalidArgument,
            "test.character.name is text: compare it with \"text\"",
        ),
        (
            "player.target[name < \"a\"] { name }",
            Kind::InvalidArgument,
            "test.character.name is text: compare it with = or !=",
        ),
        (
            "player { data }",
            Kind::InvalidArgument,
            "test.character.data is an object; name its values, such as data.<field>",
        ),
        (
            "inventory { containers }",
            Kind::InvalidArgument,
            "test.manager.containers is an array: put it on the path, before the braces, to get a row per element",
        ),
        (
            "player.data.level[0]",
            Kind::InvalidArgument,
            "test.data.level is a value: it has no filter",
        ),
    ] {
        let found = error(text, &all);
        assert_eq!(
            (found.kind, found.message.as_str()),
            (kind, message),
            "{text}"
        );
    }
}

#[test]
fn a_run_reads_each_page_once_and_stops_at_its_limits() {
    struct Counting(Buffer, std::cell::Cell<usize>);
    impl Memory for Counting {
        fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
            self.1.set(self.1.get() + 1);
            self.0.read(address, buffer)
        }
    }
    let memory = Counting(memory(true), std::cell::Cell::new(0));
    let prepared = prepare(
        &schema(),
        "inventory.containers.items { item, quantity }",
        &all,
    )
    .unwrap();
    assert_eq!(run(&memory, &prepared).unwrap().len(), 3);
    assert_eq!(memory.1.get(), 1, "everything is on the page at 0x1000");
}

#[test]
fn unreadable_memory_gives_no_value_instead_of_a_fault() {
    let prepared = prepare(&schema(), "player { name, target.name }", &all).unwrap();
    let mut memory = memory(true);
    // The target points past the buffer.
    put(&mut memory.0, 0x1810, &0x9_0000u64.to_le_bytes());
    assert_eq!(
        run(&memory, &prepared).unwrap()[0].values,
        [Some(Value::Text(b"Alisae".to_vec())), None]
    );
}

#[test]
fn held_queries_are_planned_again_for_a_new_schema() {
    let lens = Lens::new(schema());
    let mut queries = Queries::default();
    let (handle, prepared) = queries.prepare(&lens, "player { name }", &all).unwrap();
    assert_eq!(prepared.columns.len(), 1);
    assert!(queries.plan(&lens, handle, &all).is_ok());
    lens.replace(Schema::default());
    assert_eq!(
        queries.plan(&lens, handle, &all).unwrap_err().kind,
        Kind::NotFound,
        "the new map has no such root"
    );
    queries.release(handle);
    assert_eq!(
        queries.plan(&lens, handle, &all).unwrap_err().message,
        format!("no prepared query {handle}")
    );
    let lens = Lens::new(schema());
    let mut queries = Queries::default();
    for _ in 0..MOST_QUERIES {
        queries.prepare(&lens, "player { name }", &all).unwrap();
    }
    assert_eq!(
        queries
            .prepare(&lens, "player { name }", &all)
            .unwrap_err()
            .kind,
        Kind::LimitExceeded
    );
}

/// The containers belong to their kind's domain: kind 7 is the armoury's.
fn partitioned() -> Schema {
    schema_of(
        &TYPES.replace(
            "array = { of = \"test.container\", length = 4, stride = 24 }",
            "array = { of = \"test.container\", length = 4, stride = 24, partition = { field = \"kind\", domains = { armoury = [\"armoury_head\"] } } }",
        ),
        &format!(
            "{DOMAINS}\n[domains.armoury]\ndoc = \"Gear.\"\nclass = \"you\"\nread = {{ en = \"Sees your armoury chest\", ru = \"Видит ваш оружейный сундук\" }}\n"
        ),
        "doc = \"Kinds.\"\nvalues = { bag_1 = 0, bag_2 = 1, crystals = 2001, armoury_head = 7 }\n",
    )
    .unwrap()
}

#[test]
fn a_partitioned_array_reads_only_the_elements_of_permitted_domains() {
    let schema = partitioned();
    let containers = |text: &str, permitted: &dyn Fn(&str) -> bool| -> Result<Vec<Vec<u32>>> {
        let prepared = prepare(&schema, text, permitted)?;
        Ok(run(&memory(true), &prepared)?
            .into_iter()
            .map(|row| row.indexes)
            .collect())
    };
    let inventory = |domain: &str| domain == "inventory";
    let armoury = |domain: &str| domain == "armoury";
    assert_eq!(
        containers("inventory.containers { kind }", &inventory).unwrap(),
        [vec![0], vec![1], vec![2]],
        "without a filter, the permitted domains' elements"
    );
    assert_eq!(
        containers("inventory.containers { kind, size }", &armoury).unwrap(),
        [vec![3]],
        "the container's own fields come with the armoury's elements"
    );
    assert_eq!(
        prepare(&schema, "inventory.containers { size }", &armoury)
            .unwrap()
            .domains
            .into_iter()
            .collect::<Vec<_>>(),
        ["armoury"]
    );
    assert_eq!(
        containers(
            "inventory.containers[kind in (bag_1, crystals)] { kind }",
            &all
        )
        .unwrap(),
        [vec![0], vec![2]]
    );
    for (text, permitted, kind) in [
        (
            "inventory.containers[kind = bag_1] { kind }",
            &armoury as &dyn Fn(&str) -> bool,
            Kind::PermissionDenied,
        ),
        (
            "inventory.containers[kind = 7] { kind }",
            &inventory,
            Kind::PermissionDenied,
        ),
        (
            "inventory.containers[kind = 9] { kind }",
            &all,
            Kind::InvalidArgument,
        ),
        (
            "inventory.containers { kind }",
            &|domain: &str| domain == "character",
            Kind::PermissionDenied,
        ),
    ] {
        assert_eq!(
            containers(text, permitted).unwrap_err().kind,
            kind,
            "{text}"
        );
    }
    // Elements whose kind has no name are never read.
    put_kind_never_read(&schema);

    let wrong = TYPES.replace(
        "array = { of = \"test.container\", length = 4, stride = 24 }",
        "array = { of = \"test.container\", length = 4, stride = 24, partition = { field = \"kind\", domains = { armoury = [\"armoury_legs\"] } } }",
    );
    assert!(schema_of(&wrong, DOMAINS, "doc = \"K.\"\nvalues = { bag_1 = 0 }\n").is_err());
}

fn put_kind_never_read(schema: &Schema) {
    let mut memory = memory(true);
    // The third container's kind becomes 9, which has no name.
    put(&mut memory.0, 0x1100 + 2 * 24 + 8, &9u32.to_le_bytes());
    let prepared = prepare(schema, "inventory.containers { kind }", &all).unwrap();
    let indexes: Vec<Vec<u32>> = run(&memory, &prepared)
        .unwrap()
        .into_iter()
        .map(|row| row.indexes)
        .collect();
    assert_eq!(indexes, [vec![0], vec![1], vec![3]]);
}
