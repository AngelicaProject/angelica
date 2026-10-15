//! The inventory example over a scripted game: its query reads the bags
//! as the game map describes them, under the `inventory` domain the player
//! approved, and the window lists and searches the items by name. Also
//! `angelica look`'s side in the host.
use angelica_fake_game::{
    ColumnKind, Language, TestHost, build_component,
    game_files::{FakeGame, FakeRow, FakeSheet, FakeValue},
};
use runtime::{
    host::{Action, Event, Status},
    sandbox::{Limits, bindings::angelica::plugin::ui::Widget},
};
use std::sync::OnceLock;

const MANIFEST: &str = include_str!("../../../examples/inventory/angelica.toml");

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-inventory"))
}

/// The bags as the game map describes them, with the offsets of the game
/// build of 2026.10.06 as constants.
const INVENTORY_MAP: [(&str, &str); 6] = [
    (
        "entries/inventory.toml",
        r#"
[global."inventory.inventory_manager.instance"]
locator = { signature = "C3" }
type = "struct"

[field."inventory.inventory_manager.inventories"]
of = "inventory.inventory_manager"
type = "ptr"
offset = 0x1E08

[field."inventory.inventory_container.items"]
of = "inventory.inventory_container"
type = "ptr"
offset = 0x08

[field."inventory.inventory_container.kind"]
of = "inventory.inventory_container"
type = "u32"
offset = 0x10

[field."inventory.inventory_container.size"]
of = "inventory.inventory_container"
type = "u32"
offset = 0x14

[field."inventory.inventory_item.symbolic"]
of = "inventory.inventory_item"
type = "bool"
offset = 0x0E

[field."inventory.inventory_item.item_id"]
of = "inventory.inventory_item"
type = "u32"
offset = 0x10

[field."inventory.inventory_item.quantity"]
of = "inventory.inventory_item"
type = "u32"
offset = 0x14

[field."inventory.inventory_item.flags"]
of = "inventory.inventory_item"
type = "u8"
offset = 0x1C
"#,
    ),
    (
        "types/inventory.toml",
        r#"
[types."inventory.inventory_manager"]
doc = "Every container."
domain = "inventory"

[types."inventory.inventory_manager".fields.containers]
doc = "The containers."
at = "inventory.inventory_manager.inventories"
array = { of = "inventory.inventory_container", length = 84, stride = 32, partition = { field = "kind", domains = { armoury = ["armoury_head"] } } }

[types."inventory.inventory_container"]
doc = "One container."
domain = "inventory"

[types."inventory.inventory_container".fields.kind]
doc = "Which container this is."
enum = "inventory.container_kind"

[types."inventory.inventory_container".fields.size]
doc = "How many slots it has."

[types."inventory.inventory_container".fields.items]
doc = "The slots."
array = { of = "inventory.inventory_item", count = "size", most = 200, stride = 72, present = "item" }

[types."inventory.inventory_item"]
doc = "One slot."
domain = "inventory"

[types."inventory.inventory_item".fields.item]
doc = "The item."
at = "inventory.inventory_item.item_id"
sheet = "Item"

[types."inventory.inventory_item".fields.quantity]
doc = "How many."

[types."inventory.inventory_item".fields.flags]
doc = "Marks."
flags = "inventory.item_flags"

[types."inventory.inventory_item".fields.symbolic]
doc = "Links to another slot."
"#,
    ),
    (
        "enums/inventory.container_kind.toml",
        "doc = \"Containers.\"\nvalues = { bag_1 = 0, bag_2 = 1, bag_3 = 2, bag_4 = 3, crystals = 2001, armoury_head = 3201 }\n",
    ),
    (
        "enums/inventory.item_flags.toml",
        "doc = \"Marks.\"\nflags = true\nvalues = { high_quality = 1, company_crest = 2 }\n",
    ),
    (
        "domains.toml",
        r#"
[domains.inventory]
doc = "The items the player carries."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }

[domains.armoury]
doc = "The armoury chest."
class = "you"
read = { en = "Sees your armoury chest", ru = "Видит ваш оружейный сундук" }
"#,
    ),
    (
        "roots.toml",
        r#"
[roots.inventory]
doc = "The player's items."
global = "inventory.inventory_manager.instance"
type = "inventory.inventory_manager"
"#,
    ),
];

/// A game whose Item sheet names three items.
fn game() -> FakeGame {
    let names = [
        (4u32, "Wind Shard"),
        (5057, "Iron Ore"),
        (5111, "Cobalt Ingot"),
    ];
    let sheet = FakeSheet::new(vec![ColumnKind::String]).with_rows(
        Language::English,
        names
            .iter()
            .map(|(row, name)| {
                FakeRow::new(*row, vec![FakeValue::String(name.as_bytes().to_vec())])
            })
            .collect(),
    );
    FakeGame::new("2026.09.15.0000.0000").with_sheet("Item", sheet)
}

fn started() -> TestHost {
    let mut test = TestHost::new(Limits::default());
    game().write(&test.game_folder()).unwrap();
    test.use_map(&INVENTORY_MAP, &[]);
    test.name_columns("Item", &[("name", 0)]);
    test.install(MANIFEST, component()).unwrap();
    let plugin = test.host.plugin("inventory").unwrap();
    assert_eq!(
        plugin
            .requests()
            .into_iter()
            .flat_map(|(_, requests)| requests)
            .map(|request| runtime::language::Language::English.asked(&request.asked))
            .collect::<Vec<_>>(),
        ["Sees the items you carry"],
        "the approval shows the map's words"
    );
    test.host.queue(Action::Approve("inventory".into()));
    test.frames(1);
    assert_eq!(plugin.status(), Status::Running);
    test.event("inventory", Event::OpenRequested);
    test
}

/// The window's lines: text, and each entry as `title: value`.
fn lines(test: &TestHost) -> Vec<String> {
    test.host.plugin("inventory").unwrap().windows()[0]
        .widgets
        .iter()
        .filter_map(|widget| match widget {
            Widget::Text(line) => Some(line.text.clone()),
            Widget::Entry(entry) => Some(format!("{}: {}", entry.title, entry.value)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_window_lists_the_bags_by_name_and_searches_them() {
    let test = started();
    assert_eq!(lines(&test), ["Nothing in your bags"]);

    test.game.set(
        "inventory.containers[kind = bag_1].items[0]",
        &[("item", 5057.into()), ("quantity", 12.into())],
    );
    test.game.set(
        "inventory.containers[kind = bag_1].items[3]",
        &[("item", 5057.into()), ("quantity", 30.into())],
    );
    test.game.set(
        "inventory.containers[kind = bag_2].items[0]",
        &[
            ("item", 5111.into()),
            ("quantity", 2.into()),
            ("flags", 1.into()),
        ],
    );
    test.game.set(
        "inventory.containers[kind = crystals].items[0]",
        &[("item", 4.into()), ("quantity", 999.into())],
    );
    test.game.set(
        "inventory.containers[kind = bag_2].items[1]",
        &[
            ("item", 5057.into()),
            ("quantity", 5.into()),
            ("symbolic", true.into()),
        ],
    );
    test.game.set(
        "inventory.containers[kind = armoury_head].items[0]",
        &[("item", 5111.into()), ("quantity", 1.into())],
    );
    test.frames(1);
    assert_eq!(
        lines(&test),
        ["Cobalt Ingot (HQ): 2", "Iron Ore: 42"],
        "the crystals and the armoury chest are not bags; a linked slot is not counted"
    );

    test.edit("inventory", "main", "search", "iron");
    assert_eq!(lines(&test), ["Iron Ore: 42"]);
    test.edit("inventory", "main", "search", "mythril");
    assert_eq!(lines(&test), ["Nothing by that name"]);
}

#[test]
fn without_the_domain_in_the_map_the_plugin_says_why() {
    let mut test = TestHost::new(Limits::default());
    test.install(MANIFEST, component()).unwrap();
    assert_eq!(
        test.host.plugin("inventory").unwrap().ungranted(),
        ["inventory"]
    );
    test.host.queue(Action::Approve("inventory".into()));
    test.frames(1);
    test.event("inventory", Event::OpenRequested);
    assert_eq!(
        lines(&test),
        ["Cannot read the bags: the game map has no root inventory"]
    );
}

#[test]
fn look_answers_a_query_from_the_command_line_between_frames() {
    use runtime::look::{REQUEST, RESPONSE, Request, Response};
    let test = started();
    test.game.set(
        "inventory.containers[kind = bag_1].items[2]",
        &[("item", 5057.into()), ("quantity", 7.into())],
    );
    let folder = test.folder.path().join("look");
    std::fs::create_dir_all(&folder).unwrap();
    test.host.serve_look(folder.clone());
    let ask = |query: &str| {
        let request = Request {
            format: 1,
            id: "1f".into(),
            query: query.into(),
            mode: runtime::look::Mode::Rows,
            seconds: None,
            bytes: None,
            entries: Vec::new(),
            follow: Vec::new(),
            offset: None,
            size: None,
        };
        std::fs::write(folder.join(REQUEST), serde_json::to_vec(&request).unwrap()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));
        test.frames(1);
        assert!(!folder.join(REQUEST).exists(), "taken");
        serde_json::from_slice::<Response>(&std::fs::read(folder.join(RESPONSE)).unwrap()).unwrap()
    };
    let response = ask("inventory.containers.items { item, quantity, flags }");
    assert_eq!(response.id, "1f");
    assert_eq!(
        response.columns.unwrap()[0].field,
        "inventory.inventory_item.item"
    );
    let rows = response.rows.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].indexes, [0, 2]);
    assert_eq!(
        rows[0].values,
        [
            serde_json::json!({"sheet": "Item", "row": 5057}),
            serde_json::json!(7),
            serde_json::json!({"names": [], "bits": 0}),
        ]
    );
    let response = ask("inventory.containers { nothing }");
    assert_eq!(response.error.unwrap().kind, "not-found");
}

#[test]
fn a_release_build_answers_look_only_in_dev_mode() {
    use runtime::look::{REQUEST, RESPONSE, Request};
    let test = started();
    let installation = tempfile::tempdir().unwrap();
    let dev = std::sync::Arc::new(runtime::dev::DevMode::new(installation.path()));
    test.host.set_dev(dev.clone());
    let folder = test.folder.path().join("look");
    std::fs::create_dir_all(&folder).unwrap();
    test.host.serve_look_in_dev_mode(folder.clone());
    let request = Request {
        format: 1,
        id: "2a".into(),
        query: "inventory.containers { kind }".into(),
        mode: runtime::look::Mode::Rows,
        seconds: None,
        bytes: None,
        entries: Vec::new(),
        follow: Vec::new(),
        offset: None,
        size: None,
    };
    std::fs::write(folder.join(REQUEST), serde_json::to_vec(&request).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(120));
    test.frames(1);
    assert!(
        folder.join(REQUEST).exists(),
        "outside dev mode the request stays unread"
    );
    assert!(!folder.join(RESPONSE).exists());

    dev.set(true).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(120));
    test.frames(1);
    assert!(!folder.join(REQUEST).exists(), "taken in dev mode");
    assert!(folder.join(RESPONSE).exists());
}
