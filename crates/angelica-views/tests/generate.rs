//! The views a description gives: a view per structure with its own and
//! its bases' fields, typed by what each field holds; a function per
//! global; the values of each enum; nothing for what no view reads.
use angelica_anatomia::Anatomia;

const MANIFEST: &str = "format = \"1.0\"\nnamespace = \"test\"\n";

const TYPES: &str = r#"
[enum."game.kind"]
type = "u16"
values = { small = 1, large = 0xFFFF }

[struct."game.base"]
doc = "A base."
size = 0x10

[struct."game.base".fields.id]
doc = "Its id."
type = "u32"
at = 0x8

[struct."game.thing"]
doc = "A thing."
size = 0x40
extends = [{ type = "game.base" }]

[struct."game.thing".fields.kind]
type = "game.kind"
at = 0x10

[struct."game.thing".fields.name]
type = "char8[16]"
text = true
at = 0x18

[struct."game.thing".fields.next]
type = "ptr<game.thing>"
at = 0x28

[struct."game.thing".fields.parts]
type = "ptr<game.base[8]>"
at = 0x30

[struct."game.thing".fields.callback]
type = "fn(u32)"
at = 0x38
"#;

const GLOBALS: &str = r#"
[global."game.first_thing"]
type = "ptr<game.thing>"
find = [{ scan = "C3" }]

[global."game.counter"]
type = "u32"
find = [{ scan = "C3" }]
"#;

fn anatomia() -> Anatomia {
    Anatomia::from_sources(
        MANIFEST,
        &[
            ("types/game.toml".into(), TYPES.into()),
            ("globals/game.toml".into(), GLOBALS.into()),
        ],
    )
    .unwrap()
}

#[test]
fn a_structure_gives_a_view_with_its_own_and_its_bases_fields() {
    let code = angelica_views::generate(&anatomia(), "::angelica_sdk", false);
    for expected in [
        "pub mod game {",
        "pub struct Thing(Handle);",
        "const STRUCTURE: &'static str = \"game.thing\";",
        "pub fn id(&self) -> Result<u32, Error> {",
        "Field::new(\"game.base.id\")",
        "pub fn kind(&self) -> Result<u16, Error> {",
        "pub fn name(&self) -> Result<Vec<u8>, Error> {",
        "pub fn set_name(&self, value: &[u8]) -> Result<(), Error> {",
        "pub fn next(&self) -> Result<Option<super::game::Thing>, Error> {",
        "pub fn set_next(&self, value: Option<&super::game::Thing>) -> Result<(), Error> {",
        "pub fn parts(&self) -> Result<Option<Array<super::game::Base>>, Error> {",
        "pub const SMALL: u16 = 1;",
        "pub const LARGE: u16 = 65535;",
        "pub fn first_thing() -> Result<Option<super::game::Thing>, Error> {",
        "pub fn counter() -> Result<u32, Error> {",
    ] {
        assert!(code.contains(expected), "no `{expected}` in:\n{code}");
    }
    assert!(
        !code.contains("fn callback"),
        "a function pointer is not read"
    );
}

#[test]
fn the_central_views_leave_out_globals_that_hold_no_object() {
    let code = angelica_views::generate(&anatomia(), "crate", true);
    assert!(code.contains("pub fn first_thing()"));
    assert!(!code.contains("pub fn counter()"));
    assert!(code.contains("use crate::memory::"));
}
