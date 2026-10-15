//! Window elements as Lens objects: names from the map's window schemas,
//! values read from one bit, fields of a variant, text in a game string,
//! and values set through the game's functions and held against the game.
use super::set::{Changes, Setting, Writer, prepare as prepare_change};
use super::*;
use crate::error::Kind;
use angelica_map::{EntryKind, Map, Value as Resolved};
use std::cell::RefCell;

const ENTRIES: &str = r#"
[global."test.windows"]
locator = { signature = "C3" }
type = "ptr"

[field."test.list.windows"]
of = "test.list"
type = "ptr"
offset = 0x0

[field."test.list.count"]
of = "test.list"
type = "u16"
offset = 0x8

[field."test.window.name"]
of = "test.window"
type = "struct"
offset = 0x10

[field."test.window.elements"]
of = "test.window"
type = "ptr"
offset = 0x40

[field."test.window.element_count"]
of = "test.window"
type = "u16"
offset = 0x48

[field."test.node.id"]
of = "test.node"
type = "u32"
offset = 0x0

[field."test.node.kind"]
of = "test.node"
type = "u16"
offset = 0x4

[field."test.node.flags"]
of = "test.node"
type = "u32"
offset = 0x8

[field."test.text_node.text"]
of = "test.text_node"
type = "struct"
offset = 0x10

[field."test.string.bytes"]
of = "test.string"
type = "ptr"
offset = 0x0

[function."test.set_visible"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8"], ret = "void" }
thread = "main"

[function."test.set_text"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "void" }
thread = "main"
"#;

const TYPES: &str = r#"
[types."test.list"]
doc = "The windows."
domain = "windows"

[types."test.list".fields.count]
doc = "How many."

[types."test.list".fields.windows]
doc = "The windows."
array = { of = "test.window", count = "count", most = 8, stride = 8, pointers = true }

[types."test.window"]
doc = "A window."
domain = "windows"

[types."test.window".fields.name]
doc = "Its name."
text = 32

[types."test.window".fields.element_count]
doc = "How many elements."

[types."test.window".fields.elements]
doc = "Its elements."
array = { of = "test.node", count = "element_count", most = 8, stride = 8, pointers = true }

[types."test.node"]
doc = "An element."
domain = "windows"

[types."test.node".fields.name]
doc = "Its name."
at = "test.node.id"
element = "test.window.name"

[types."test.node".fields.kind]
doc = "What it is."
enum = "test.node_kind"

[types."test.node".fields.visible]
doc = "Whether it is shown."
at = "test.node.flags"
bit = 20
set = { call = "test.set_visible", hold = true }

[types."test.text_node"]
doc = "A text element."
domain = "windows"
extends = "test.node"
variant = { field = "kind", values = ["text"] }

[types."test.text_node".fields.text]
doc = "Its text."
pointer = "test.string.bytes"
text = 64
set = { call = "test.set_text", hold = true }

[types."test.string"]
doc = "A game string."
domain = "windows"

[types."test.string".fields.bytes]
doc = "Its text."
text = 64
"#;

const OTHERS: [(&str, &str); 4] = [
    (
        "domains.toml",
        r#"
[domains.windows]
doc = "Windows."
class = "others"
read = { en = "Sees windows", ru = "Видит окна" }
change = { en = "Changes windows", ru = "Меняет окна" }
"#,
    ),
    (
        "roots.toml",
        r#"
[roots.windows]
doc = "The windows."
global = "test.windows"
type = "test.list"
"#,
    ),
    (
        "enums/test.node_kind.toml",
        "doc = \"Kinds.\"\nvalues = { res = 1, text = 3 }\n",
    ),
    (
        "windows/_Exp.toml",
        r#"
window = "_Exp"
doc = "The experience bar."
layout = "ui/uld/Exp.uld"

[elements.window]
kind = "other"
doc = "The bar."

[elements.text]
kind = "text"
doc = "Its text."

[[layouts]]
fingerprint = "FINGERPRINT"
elements = { window = 1, text = 4 }
"#,
    ),
];

const LAYOUT: &[u8] = b"uldh0100 experience";
const SET_VISIBLE: u64 = 0x1F00;
const SET_TEXT: u64 = 0x1F10;
const VISIBLE: u32 = 1 << 20;

/// The schema, with the bar's layout file in the game as `layout` holds it.
fn schema(layout: &'static [u8]) -> Schema {
    let fingerprint = angelica_map::window_layout_fingerprint(LAYOUT);
    let mut files: Vec<(String, String)> = vec![
        ("entries/test.toml".into(), ENTRIES.into()),
        ("types/test.toml".into(), TYPES.into()),
    ];
    files.extend(OTHERS.iter().map(|(path, text)| {
        (
            (*path).to_owned(),
            text.replace("FINGERPRINT", &fingerprint),
        )
    }));
    let map = Map::from_sources("format = 2", &files).unwrap();
    let entries = map.clone();
    let mut schema = Schema::new(&map, 0x1000, move |name| {
        Ok(match &entries.get(name).unwrap().kind {
            EntryKind::Field { offset, .. } => Resolved::Offset(offset.unwrap()),
            _ if name == "test.set_visible" => Resolved::Address(0xF00),
            _ if name == "test.set_text" => Resolved::Address(0xF10),
            _ => Resolved::Address(0),
        })
    });
    schema.resolve_windows(&map, &|path| {
        if path == "ui/uld/Exp.uld" {
            Ok(layout.to_vec())
        } else {
            Err("no such file".into())
        }
    });
    schema
}

/// The game's memory from 0x1000, written through its setting functions.
struct Game {
    bytes: RefCell<Vec<u8>>,
    /// Where the next text the game writes goes.
    free: RefCell<u64>,
}

impl Memory for Game {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let Some(start) = address
            .checked_sub(0x1000)
            .and_then(|a| usize::try_from(a).ok())
        else {
            return false;
        };
        match self.bytes.borrow().get(start..start + buffer.len()) {
            Some(bytes) => {
                buffer.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

impl Game {
    fn put(&self, at: u64, value: &[u8]) {
        let at = usize::try_from(at - 0x1000).unwrap();
        self.bytes.borrow_mut()[at..at + value.len()].copy_from_slice(value);
    }

    fn word(&self, at: u64) -> u64 {
        let mut bytes = [0u8; 8];
        self.read(at, &mut bytes);
        u64::from_le_bytes(bytes)
    }

    /// The game writes a text element's text, in a new place.
    fn write_text(&self, node: u64, text: &str) {
        let at = *self.free.borrow();
        *self.free.borrow_mut() += 0x40;
        let mut terminated = text.as_bytes().to_vec();
        terminated.push(0);
        self.put(at, &terminated);
        self.put(node + 0x10, &at.to_le_bytes());
    }

    fn show(&self, node: u64, shown: bool) {
        let flags = u32::try_from(self.word(node + 8) & 0xFFFF_FFFF).unwrap();
        let flags = if shown {
            flags | VISIBLE
        } else {
            flags & !VISIBLE
        };
        self.put(node + 8, &flags.to_le_bytes());
    }

    fn shown(&self, node: u64) -> bool {
        self.word(node + 8) & u64::from(VISIBLE) != 0
    }

    fn text(&self, node: u64) -> String {
        let at = self.word(node + 0x10);
        let bytes = self.bytes.borrow();
        let start = usize::try_from(at - 0x1000).unwrap();
        let end = bytes[start..].iter().position(|&b| b == 0).unwrap();
        String::from_utf8(bytes[start..start + end].to_vec()).unwrap()
    }
}

impl Writer for Game {
    fn memory(&self) -> &dyn Memory {
        self
    }

    fn write(&self, _: u64, _: &[u8]) -> bool {
        panic!("window values are set through the game's functions")
    }

    fn call_setter(&self, function: u64, object: u64, value: &Setting) -> bool {
        match (function, value) {
            (SET_VISIBLE, Setting::Number(bits)) => self.show(object, *bits & 0xFF != 0),
            (SET_TEXT, Setting::Text(text)) => {
                self.write_text(object, std::str::from_utf8(text).unwrap());
            }
            _ => panic!("{function:#x} does not take {value:?}"),
        }
        true
    }
}

const BAR: u64 = 0x1500;
const TEXT: u64 = 0x1600;

/// The window list at 0x1100 with the bar's window at 0x1300, whose
/// elements are the whole bar (id 1) and its text (id 4).
fn game() -> Game {
    let game = Game {
        bytes: RefCell::new(vec![0u8; 0x1000]),
        free: RefCell::new(0x1800),
    };
    game.put(0x1000, &0x1100u64.to_le_bytes());
    game.put(0x1100, &0x1200u64.to_le_bytes());
    game.put(0x1108, &1u16.to_le_bytes());
    game.put(0x1200, &0x1300u64.to_le_bytes());
    game.put(0x1310, b"_Exp");
    game.put(0x1340, &0x1400u64.to_le_bytes());
    game.put(0x1348, &2u16.to_le_bytes());
    game.put(0x1400, &BAR.to_le_bytes());
    game.put(0x1408, &TEXT.to_le_bytes());
    for (node, id, kind) in [(BAR, 1u32, 1u16), (TEXT, 4, 3)] {
        game.put(node, &id.to_le_bytes());
        game.put(node + 4, &kind.to_le_bytes());
        game.show(node, true);
    }
    game.write_text(TEXT, "EXP 1/2");
    game
}

const ELEMENTS: &str = "windows.windows[name = \"_Exp\"].elements";

fn all(_: &str) -> bool {
    true
}

fn rows(text: &str) -> Vec<Row> {
    let prepared = prepare(&schema(LAYOUT), text, &all).unwrap();
    run(&game(), &prepared).unwrap()
}

#[test]
fn elements_are_named_by_the_window_the_path_names() {
    let found = rows(&format!("{ELEMENTS} {{ name, visible }}"));
    assert_eq!(
        found
            .iter()
            .map(|row| row.values.clone())
            .collect::<Vec<_>>(),
        [
            vec![
                Some(Value::Named("window".into())),
                Some(Value::Boolean(true))
            ],
            vec![
                Some(Value::Named("text".into())),
                Some(Value::Boolean(true))
            ],
        ]
    );
    let text = rows(&format!("{ELEMENTS}[name = text] {{ text }}"));
    assert_eq!(text[0].values, [Some(Value::Text(b"EXP 1/2".to_vec()))]);

    let error =
        |text: &str, layout: &'static [u8]| prepare(&schema(layout), text, &all).unwrap_err();
    let unnamed = error("windows.windows.elements[name = text] { visible }", LAYOUT);
    assert_eq!(unnamed.kind, Kind::InvalidArgument);
    assert!(
        unnamed
            .message
            .contains("name the window earlier on the path")
    );
    assert_eq!(
        error(&format!("{ELEMENTS}[name = gauge] {{ visible }}"), LAYOUT).kind,
        Kind::NotFound
    );
    assert_eq!(
        error(
            "windows.windows[name = \"_Money\"].elements[name = text] { visible }",
            LAYOUT
        )
        .kind,
        Kind::NotFound
    );
    let unknown = error(
        &format!("{ELEMENTS}[name = text] {{ visible }}"),
        b"another build",
    );
    assert_eq!(unknown.kind, Kind::Unavailable);
    assert!(unknown.message.contains("does not know layout"));
}

#[test]
fn a_variants_field_is_read_only_in_objects_of_the_variant() {
    let found = rows(&format!("{ELEMENTS} {{ name, text }}"));
    assert_eq!(found[0].values[1], None, "the bar is not a text element");
    assert_eq!(found[1].values[1], Some(Value::Text(b"EXP 1/2".to_vec())));
    assert!(rows(&format!("{ELEMENTS}[name = window].text")).is_empty());
}

#[test]
fn held_values_are_set_through_the_game_and_follow_it() {
    let schema = schema(LAYOUT);
    let game = game();
    let changes = Changes::default();
    let ready = |path: &str, value: Value| {
        prepare_change(&schema, &format!("{ELEMENTS}{path}"), &value, &all, &all)
    };
    let text = ready("[name = text].text", Value::Text(b"mine".to_vec())).unwrap();
    assert_eq!(changes.set("p", &text, &game).unwrap(), 1);
    assert_eq!(game.text(TEXT), "mine");
    assert!(changes.holding());

    // The game writes its own text again; the frame puts the plugin's back.
    game.write_text(TEXT, "EXP 2/2");
    changes.frame(&game);
    assert_eq!(game.text(TEXT), "mine");
    changes.restore("p", &text.text, &game);
    assert_eq!(game.text(TEXT), "EXP 2/2", "the game's latest text is back");

    let hidden = ready("[name = window].visible", Value::Boolean(false)).unwrap();
    changes.set("p", &hidden, &game).unwrap();
    assert!(!game.shown(BAR));
    game.show(BAR, true);
    changes.frame(&game);
    assert!(!game.shown(BAR));
    // Another plugin may not take the value.
    assert_eq!(
        changes.set("q", &hidden, &game).unwrap_err().kind,
        Kind::Unavailable
    );
    changes.remove_plugin("p", &game);
    assert!(game.shown(BAR));
    assert!(!changes.holding());

    // The bar's own element has no text: the change reaches nothing.
    let none = ready("[name = window].text", Value::Text(b"x".to_vec())).unwrap();
    assert_eq!(changes.set("p", &none, &game).unwrap(), 0);
}

#[test]
fn text_is_checked_before_the_game_gets_it() {
    let schema = schema(LAYOUT);
    let path = format!("{ELEMENTS}[name = text].text");
    let error = |value: Value| prepare_change(&schema, &path, &value, &all, &all).unwrap_err();
    assert_eq!(
        error(Value::Text(b"a\x02b".to_vec())).kind,
        Kind::InvalidArgument,
        "a macro"
    );
    assert_eq!(error(Value::Text(vec![0xFF])).kind, Kind::InvalidArgument);
    assert_eq!(error(Value::Text(vec![b'x'; 64])).kind, Kind::LimitExceeded);
    assert_eq!(error(Value::Unsigned(3)).kind, Kind::InvalidArgument);
    let windowless = |domain: &str| domain != "windows";
    assert_eq!(
        prepare_change(
            &schema,
            &path,
            &Value::Text(b"x".to_vec()),
            &all,
            &windowless
        )
        .unwrap_err()
        .kind,
        Kind::PermissionDenied
    );
}
