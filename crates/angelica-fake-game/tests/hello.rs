//! The example plugin in a host with a scripted game: approval, windows and
//! events, storage, jobs, text tables, letter case, chat commands and
//! messages, permissions, and disabling.
use angelica_fake_game::{
    TestHost, build_component,
    game_files::{FakeGame, TextSheet},
    row_buffer, row_strings, store_row,
};
use angelica_text::table::{Column, Variant};
use runtime::{
    excel_text::SheetInfo,
    host::{Action, Event, Status},
    sandbox::{
        Limits,
        bindings::angelica::plugin::ui::{Edited, Widget, WidgetRef},
    },
};
use std::sync::OnceLock;

const MANIFEST: &str = include_str!("../../../examples/hello/angelica.toml");

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-hello"))
}

fn approved(manifest: &str) -> TestHost {
    approve(TestHost::new(Limits::default()), manifest)
}

fn approve(mut test: TestHost, manifest: &str) -> TestHost {
    test.install(manifest, component()).unwrap();
    test.host.queue(Action::Approve("hello".into()));
    test.frames(1);
    assert_eq!(test.host.plugin("hello").unwrap().status(), Status::Running);
    test.event("hello", Event::OpenRequested);
    test
}

/// A host over `game`, with the example approved after `setup`. The
/// example reads the game's command list when it starts, so the game's
/// files exist before.
fn approved_over(game: &FakeGame, manifest: &str, setup: impl FnOnce(&TestHost)) -> TestHost {
    let test = TestHost::new(Limits::default());
    game.write(&test.game_folder()).unwrap();
    setup(&test);
    approve(test, manifest)
}

/// A game whose command list has `/tell`.
fn game_with_commands() -> FakeGame {
    FakeGame::new("2026.09.15.0000.0000").with_text(
        "TextCommand",
        &TextSheet::new(2, &[0, 1]).row(1, &[(0, "/tell"), (1, "/t")]),
    )
}

fn with_commands(manifest: &str, setup: impl FnOnce(&TestHost)) -> TestHost {
    approved_over(&game_with_commands(), manifest, setup)
}

/// What the player ran printed in the chat log.
fn printed(test: &TestHost, line: &str) -> Vec<String> {
    test.game.printed.lock().unwrap().clear();
    assert!(test.command(line), "{line} was left to the game");
    test.game.printed_text()
}

/// The example's manifest without these permissions.
fn without_permissions(names: &[&str]) -> String {
    MANIFEST
        .lines()
        .filter(|line| {
            !names
                .iter()
                .any(|name| line.starts_with(&format!("\"{name}\"")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The example's manifest without `http.get` and its hosts.
fn without_http() -> String {
    without_permissions(&["http.get", "http.websocket"])
        .lines()
        .filter(|line| *line != "[http]" && !line.starts_with("hosts ="))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// Clicks a job's button and waits for the job's message.
fn run_job(test: &TestHost, id: &str) -> String {
    click(test, id);
    for _ in 0..3000 {
        if message(test).starts_with("job") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        test.frames(1);
    }
    message(test)
}

fn click(test: &TestHost, id: &str) {
    test.event(
        "hello",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: id.into(),
        }),
    );
}

fn message(test: &TestHost) -> String {
    let windows = test.host.plugin("hello").unwrap().windows();
    match windows[0].widgets.last() {
        Some(Widget::Text(last)) => last.text.clone(),
        other => panic!("unexpected last widget {other:?}"),
    }
}

fn item() -> SheetInfo {
    SheetInfo {
        name: "Item".into(),
        variant: Variant::Rows,
        language: 2,
        columns: vec![Column {
            index: 0,
            offset: 0,
        }],
    }
}

#[test]
fn a_plugin_runs_only_after_approval() {
    let mut test = TestHost::new(Limits::default());
    test.install(MANIFEST, component()).unwrap();
    test.host.start_all();
    test.frames(1);
    let plugin = test.host.plugin("hello").unwrap();
    assert_eq!(plugin.status(), Status::AwaitingApproval);
    assert!(plugin.windows().is_empty());

    test.host.queue(Action::Approve("hello".into()));
    test.frames(1);
    assert_eq!(plugin.status(), Status::Running);
    assert!(test.folder.path().join("grants/hello.json").exists());
    assert!(plugin.log.lines()[0].text.contains("2026.09.15.0000.0000"));
}

#[test]
fn windows_events_and_storage() {
    let test = approved(MANIFEST);
    let plugin = test.host.plugin("hello").unwrap();
    assert_eq!(plugin.windows()[0].title, "Hello");

    test.event(
        "hello",
        Event::Edited(Edited {
            window: "main".into(),
            id: "name".into(),
            value: "Alphinaud".into(),
        }),
    );
    click(&test, "click");
    click(&test, "click");
    let widgets = &plugin.windows()[0].widgets;
    assert!(matches!(&widgets[0], Widget::Heading(heading) if heading == "Hello, Alphinaud"));
    assert_eq!(
        std::fs::read(test.folder.path().join("storage/hello/clicks.json")).unwrap(),
        b"2"
    );

    test.host.queue(Action::OpenFolder("hello".into()));
    test.frames(1);
    assert_eq!(
        test.opened.lock().unwrap().as_slice(),
        [test.folder.path().join("storage/hello")]
    );

    test.event("hello", Event::Closed("main".into()));
    assert!(plugin.windows().is_empty());
}

#[test]
fn jobs_run_in_their_own_instance() {
    let test = approved(MANIFEST);
    click(&test, "count");
    for _ in 0..500 {
        if message(&test).starts_with("job") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        test.frames(1);
    }
    assert_eq!(message(&test), "job finished: 9 bytes");
    assert_eq!(
        std::fs::read(test.folder.path().join("storage/hello/jobs.log")).unwrap(),
        b"hello job"
    );

    click(&test, "fail");
    for _ in 0..500 {
        if message(&test).starts_with("job failed") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        test.frames(1);
    }
    assert_eq!(message(&test), "job failed: this job always fails");
}

#[test]
fn text_tables_change_rows_the_game_builds_until_the_plugin_stops() {
    let test = with_character(MANIFEST, &[]);
    let excel = &test.services().excel;
    let columns = item().columns;
    let potion = || row_buffer(4, &columns, &[b"Potion"]);
    assert_eq!(
        store_row(excel, &item(), 1, 0, potion(), 4),
        potion(),
        "no table yet"
    );

    click(&test, "table");
    assert_eq!(message(&test), "table active: 1 sheets, 1 cells");
    let changed = store_row(excel, &item(), 1, 0, potion(), 4);
    assert_eq!(row_strings(&changed, 4), ["Зелье".as_bytes()]);
    let other = row_buffer(4, &columns, &[b"Hi-Potion"]);
    assert_eq!(
        store_row(excel, &item(), 1, 0, other.clone(), 4),
        other,
        "guard mismatch"
    );

    click(&test, "status");
    assert_eq!(message(&test), "table: 1 cells, 1 stale");

    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert_eq!(
        store_row(excel, &item(), 1, 0, potion(), 4),
        potion(),
        "removed with the plugin"
    );
}

#[test]
fn letters_apply_until_the_plugin_stops() {
    let test = with_character(MANIFEST, &[]);
    click(&test, "letters");
    assert_eq!(message(&test), "letters added");
    let mut name = "паладин".as_bytes().to_vec();
    test.services()
        .case
        .letters()
        .to_upper(&mut name, true, false, |_| false);
    assert_eq!(name, "Паладин".as_bytes());
    let mut shout = "ПАЛАДИН".as_bytes().to_vec();
    test.services().case.letters().to_lower(&mut shout);
    assert_eq!(shout, "паладин".as_bytes());

    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert!(test.services().case.letters().is_empty());
}

#[test]
fn operations_need_their_permission_and_the_game_entries() {
    let without = MANIFEST.replace(
        "change = [\"camera\", \"chat\", \"text\", \"windows\"]",
        "change = [\"camera\", \"chat\", \"windows\"]",
    );
    let test = with_character(&without, &[]);
    click(&test, "table");
    assert_eq!(
        message(&test),
        "table refused: deciding at sheet_text changes domain text, which the plugin may not change: list it in the manifest's [lens] change"
    );
    click(&test, "letters");
    assert_eq!(
        message(&test),
        "letters refused: deciding at letter_upper changes domain text, which the plugin may not change: list it in the manifest's [lens] change"
    );

    // Without the map, the events are unknown.
    let test = approved(MANIFEST);
    click(&test, "table");
    assert_eq!(
        message(&test),
        "table refused: the game map has no event sheet_text"
    );

    let test = with_character(MANIFEST, &[]);
    test.game
        .excel_available
        .store(false, std::sync::atomic::Ordering::Release);
    click(&test, "table");
    assert_eq!(
        message(&test),
        "table refused: map entry excel.hash_table_page_row_resolver.store_row is absent"
    );
}

#[test]
fn the_players_choice_and_grants_persist_across_game_starts() {
    let test = approved(MANIFEST);
    click(&test, "click");
    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    let folder = test.folder;

    let mut next = TestHost::in_folder(folder, Limits::default());
    next.install(MANIFEST, component()).unwrap();
    next.host.start_all();
    assert_eq!(
        next.host.plugin("hello").unwrap().status(),
        Status::Disabled
    );
    next.host.queue(Action::Enable("hello".into()));
    next.frames(1);
    assert_eq!(next.host.plugin("hello").unwrap().status(), Status::Running);
    let log = next.host.plugin("hello").unwrap().log.lines();
    assert!(log[0].text.ends_with("with the game: false"));

    let folder = next.folder;
    let mut next = TestHost::in_folder(folder, Limits::default());
    next.install(MANIFEST, component()).unwrap();
    next.host.start_all();
    let plugin = next.host.plugin("hello").unwrap();
    assert_eq!(plugin.status(), Status::Running);
    assert!(plugin.log.lines()[0].text.ends_with("with the game: true"));
    next.event("hello", Event::OpenRequested);
    assert!(
        matches!(&plugin.windows()[0].widgets[6], Widget::Text(clicks) if clicks.text == "1 clicks")
    );
}

#[test]
fn plugins_read_the_games_files_and_sheets() {
    use angelica_fake_game::game_files::{FakeRow, FakeSheet, FakeValue};
    use angelica_sqpack::excel::{ColumnKind, Language};
    let test = approved(MANIFEST);
    click(&test, "read");
    assert!(
        message(&test).starts_with("cannot read Item:"),
        "{}",
        message(&test)
    );

    let sheet = FakeSheet::new(vec![ColumnKind::String]).with_rows(
        Language::English,
        vec![FakeRow::new(1, vec![FakeValue::String(b"Potion".to_vec())])],
    );
    let game = game_with_commands()
        .with_sheet("Item", sheet)
        .with_file("common/font/AXIS_12.fdt", vec![7; 100]);
    let test = approved_over(&game, MANIFEST, |_| {});
    click(&test, "read");
    assert_eq!(
        message(&test),
        "cannot read Item: the game map names no columns of sheet Item"
    );
    test.name_columns("Item", &[("name", 0)]);
    click(&test, "read");
    assert_eq!(
        message(&test),
        "Item 1 is Potion; the font file has 100 bytes"
    );
}

#[test]
fn plugins_replace_game_files_the_host_can_check() {
    let mut texture = vec![0u8; 80 + 32];
    texture[4] = 0x40;
    texture[5] = 0x14;
    let game = game_with_commands().with_file("common/font/font1.tex", texture.clone());
    let test = approved_over(&game, MANIFEST, |_| {});
    click(&test, "texture");
    assert_eq!(message(&test), "texture replaced");
    let served = test
        .services()
        .files
        .lookup(b"common/font/font1.tex")
        .unwrap();
    let cache = String::from_utf16(&served[..served.len() - 1]).unwrap();
    let mut expected = texture;
    *expected.last_mut().unwrap() ^= 0xFF;
    assert_eq!(std::fs::read(cache).unwrap(), expected);

    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert!(
        test.services()
            .files
            .lookup(b"common/font/font1.tex")
            .is_none()
    );

    let game = game_with_commands().with_file("common/font/font1.tex", vec![0u8; 80 + 32]);
    let test = approved_over(&game, &without_permissions(&["files.replace"]), |_| {});
    click(&test, "texture");
    assert_eq!(
        message(&test),
        "texture refused: the plugin does not hold files.replace"
    );
}

#[test]
fn commands_run_and_print_in_the_chat_log() {
    let test = with_commands(MANIFEST, |_| {});
    assert_eq!(
        printed(&test, "/hello greet Alphinaud 2"),
        ["[Hello] Hello, Alphinaud!
Hello, Alphinaud!"]
    );
    assert_eq!(
        printed(&test, "/HELLO g \"Y'shtola Rhul\""),
        ["[Hello] Hello, Y'shtola Rhul!"]
    );
    assert_eq!(
        printed(&test, "/hello print Good  it works "),
        ["[Hello] it works"]
    );
    assert_eq!(
        printed(&test, "/hello colors"),
        ["[Hello] normal accent muted good warning bad italic"]
    );

    printed(&test, "/hello caps on");
    test.event(
        "hello",
        Event::Edited(Edited {
            window: "main".into(),
            id: "name".into(),
            value: "Alisaie".into(),
        }),
    );
    let windows = test.host.plugin("hello").unwrap().windows();
    assert!(
        matches!(&windows[0].widgets[0], Widget::Heading(heading) if heading == "Hello, ALISAIE")
    );

    test.game.printed.lock().unwrap().clear();
    assert!(!test.command("/tell Alphinaud hi"), "the game's command");
    assert!(!test.command("hello"), "not a command");
    assert!(test.game.printed_text().is_empty());
}

#[test]
fn the_host_answers_help_mistakes_and_errors() {
    let test = with_commands(MANIFEST, |_| {});
    let help = printed(&test, "/hello help");
    assert!(
        help[0].starts_with("[Hello] /hello — Greets you"),
        "{help:?}"
    );
    assert!(help[0].contains(
        "
/hello greet <name> [times] — Greets someone"
    ));
    assert!(help[0].contains(
        "
/hello print <normal|accent|muted|good|warning|bad> <words...>"
    ));
    let help = printed(&test, "/hello help greet");
    assert!(help[0].contains("(also g)"), "{help:?}");
    assert!(help[0].contains(
        "
  times — How often (whole number, 1 to 5)"
    ));

    assert_eq!(
        printed(&test, "/hello greet"),
        ["[Hello] Missing <name>.
Usage: /hello greet <name> [times] · /hello help"]
    );
    let mistake = printed(&test, "/hello greet Alphinaud 6");
    assert!(
        mistake[0].starts_with("[Hello] <times> is from 1 to 5."),
        "{mistake:?}"
    );
    let mistake = printed(&test, "/hello print loud hi");
    assert!(
        mistake[0].starts_with("[Hello] <tone> is one of normal, accent"),
        "{mistake:?}"
    );

    assert_eq!(
        printed(&test, "/hello fail"),
        ["[Hello] this command always fails"]
    );
}

#[test]
fn commands_and_messages_need_their_permission() {
    let test = with_commands(&without_permissions(&["chat.commands"]), |_| {});
    assert_eq!(
        message(&test),
        "commands refused: the plugin does not hold chat.commands"
    );
    assert!(!test.command("/hello"));

    let test = with_commands(&without_permissions(&["chat.print"]), |_| {});
    assert!(printed(&test, "/hello greet Alphinaud").is_empty());
    assert_eq!(
        message(&test),
        "cannot print: the plugin does not hold chat.print"
    );
    let help = printed(&test, "/hello help");
    assert!(
        help[0].starts_with("[Hello] /hello"),
        "the host's answers need no permission"
    );
}

#[test]
fn commands_and_messages_need_the_game() {
    let test = approved(MANIFEST);
    assert!(
        message(&test).starts_with("commands refused: the game's command list cannot be read"),
        "{}",
        message(&test)
    );

    let test = with_commands(MANIFEST, |test| {
        test.game
            .commands_available
            .store(false, std::sync::atomic::Ordering::Release);
    });
    assert_eq!(
        message(&test),
        "commands refused: map entry shell.command_module.execute_command_inner is absent"
    );

    let test = with_commands(MANIFEST, |test| {
        test.game
            .chat_available
            .store(false, std::sync::atomic::Ordering::Release);
    });
    assert!(printed(&test, "/hello").is_empty());
    assert_eq!(
        message(&test),
        "cannot print: the chat log does not exist yet"
    );
}

#[test]
fn commands_are_removed_with_the_plugin() {
    let test = with_commands(MANIFEST, |_| {});
    assert_eq!(
        printed(&test, "/hello"),
        ["[Hello] Hello! Try /hello help."]
    );
    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert!(!test.command("/hello"));
}

#[test]
fn requests_need_their_permission() {
    let test = approved(&without_http());
    assert_eq!(
        run_job(&test, "fetch"),
        "job failed: the plugin does not hold http.get"
    );
}

/// Needs the internet.
#[test]
#[ignore = "needs the internet"]
fn jobs_fetch_from_the_listed_hosts() {
    let test = approved(MANIFEST);
    let message = run_job(&test, "fetch");
    assert!(message.starts_with("job finished: 200 with "), "{message}");
    let saved = std::fs::read(test.folder.path().join("storage/hello/example.html")).unwrap();
    assert!(
        message.ends_with(&format!("saved {} bytes with 200", saved.len())),
        "{message}"
    );
    assert!(String::from_utf8_lossy(&saved).contains("Example Domain"));
}

/// The player's character, zone, and statuses, shaped like the game
/// map's, with constant offsets.
const CHARACTER_MAP: [(&str, &str); 12] = [
    (
        "entries/excel.toml",
        r#"
[function."excel.hash_table_page_row_resolver.store_row"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "ptr" }
thread = "any"
"#,
    ),
    (
        "entries/text.toml",
        r#"
[function."text.utf8_string.to_upper"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8", "u8", "u8", "ptr"], ret = "ptr" }
thread = "any"

[function."text.utf8_string.to_lower"]
locator = { signature = "C3" }
abi = { args = ["ptr"], ret = "ptr" }
thread = "any"
"#,
    ),
    (
        "entries/test.toml",
        r#"
[global."test.local_player"]
locator = { signature = "C3" }
type = "ptr"

[global."test.game_main"]
locator = { signature = "C3" }
type = "struct"

[field."test.game_object.name"]
of = "test.game_object"
type = "struct"
offset = 0x30

[field."test.game_object.position_x"]
of = "test.game_object"
type = "f32"
offset = 0xB0

[field."test.game_object.position_y"]
of = "test.game_object"
type = "f32"
offset = 0xB4

[field."test.game_object.position_z"]
of = "test.game_object"
type = "f32"
offset = 0xB8

[field."test.character.data"]
of = "test.character"
type = "struct"
offset = 0x1A0

[field."test.character_data.class_job"]
of = "test.character_data"
type = "u8"
offset = 0x2A

[field."test.character_data.level"]
of = "test.character_data"
type = "u8"
offset = 0x2B

[field."test.character_data.health"]
of = "test.character_data"
type = "u32"
offset = 0x0C

[field."test.character_data.max_health"]
of = "test.character_data"
type = "u32"
offset = 0x10

[field."test.character_data.mana"]
of = "test.character_data"
type = "u32"
offset = 0x14

[field."test.character_data.max_mana"]
of = "test.character_data"
type = "u32"
offset = 0x18

[field."test.battle_chara.status_manager"]
of = "test.battle_chara"
type = "struct"
offset = 0x23B0

[field."test.status_manager.statuses"]
of = "test.status_manager"
type = "struct"
offset = 0x8

[field."test.status.status_id"]
of = "test.status"
type = "u16"
offset = 0x0

[field."test.status.remaining_time"]
of = "test.status"
type = "f32"
offset = 0x4

[field."test.game_main.current_territory_type"]
of = "test.game_main"
type = "u32"
offset = 0x4108

[function."test.use_action"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u32", "u32", "u64"], ret = "bool" }
thread = "main"

[global."test.action_manager"]
locator = { signature = "C3" }
type = "struct"

[function."test.print_message"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u16", "ptr", "ptr", "i32", "u8"], ret = "u32" }
thread = "main"

[field."test.utf8_string.string_ptr"]
of = "test.utf8_string"
type = "ptr"
offset = 0x0

[global."test.stage"]
locator = { signature = "C3" }
type = "ptr"

[field."test.stage.unit_manager"]
of = "test.stage"
type = "ptr"
offset = 0x20

[field."test.unit_manager.units"]
of = "test.unit_manager"
type = "struct"
offset = 0x10

[field."test.unit_manager.unit_count"]
of = "test.unit_manager"
type = "u16"
offset = 0x8

[field."test.window.name"]
of = "test.window"
type = "struct"
offset = 0x8

[field."test.window.values"]
of = "test.window"
type = "ptr"
offset = 0x178

[field."test.window.value_count"]
of = "test.window"
type = "u16"
offset = 0x1E2

[field."test.atk_value.kind"]
of = "test.atk_value"
type = "u8"
offset = 0x0

[field."test.atk_value.text"]
of = "test.atk_value"
type = "ptr"
offset = 0x8

[function."test.fire_callback_int"]
locator = { signature = "C3" }
abi = { args = ["ptr", "i32"], ret = "bool" }
thread = "main"

[field."test.window.layout"]
of = "test.window"
type = "ptr"
offset = 0x28

[field."test.widget.nodes"]
of = "test.widget"
type = "ptr"
offset = 0x10

[field."test.widget.node_count"]
of = "test.widget"
type = "u16"
offset = 0x18

[field."test.node.id"]
of = "test.node"
type = "u32"
offset = 0x14

[field."test.node.kind"]
of = "test.node"
type = "u16"
offset = 0x90

[field."test.node.flags"]
of = "test.node"
type = "u32"
offset = 0x9C

[function."test.set_visible"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8"], ret = "void" }
thread = "main"

[field."test.text_node.text"]
of = "test.text_node"
type = "struct"
offset = 0xD0

[function."test.set_text"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "void" }
thread = "main"

[global."test.camera_manager"]
locator = { signature = "C3" }
type = "struct"

[field."test.camera_manager.camera"]
of = "test.camera_manager"
type = "ptr"
offset = 0x0

[field."test.camera.max_distance"]
of = "test.camera"
type = "f32"
offset = 0x12C

[field."test.camera.distance"]
of = "test.camera"
type = "f32"
offset = 0x124
"#,
    ),
    (
        "types/test.toml",
        r#"
[types."test.game_object"]
doc = "An object."
domain = "character"

[types."test.game_object".fields.name]
doc = "The name."
text = 64

[types."test.game_object".fields.position_x]
doc = "East-west."
domain = "location"

[types."test.game_object".fields.position_y]
doc = "Height."
domain = "location"

[types."test.game_object".fields.position_z]
doc = "North-south."
domain = "location"

[types."test.character"]
doc = "A character."
domain = "character"
extends = "test.game_object"

[types."test.character".fields.data]
doc = "Its numbers."
embed = "test.character_data"

[types."test.character_data"]
doc = "Numbers."
domain = "character"

[types."test.character_data".fields.class_job]
doc = "Class."
sheet = "ClassJob"

[types."test.character_data".fields.level]
doc = "Level."

[types."test.character_data".fields.health]
doc = "HP."

[types."test.character_data".fields.max_health]
doc = "Maximum HP."

[types."test.character_data".fields.mana]
doc = "MP."

[types."test.character_data".fields.max_mana]
doc = "Maximum MP."

[types."test.battle_chara"]
doc = "A battle character."
domain = "character"
extends = "test.character"

[types."test.battle_chara".fields.status_manager]
doc = "Its statuses."
embed = "test.status_manager"
domain = "statuses"

[types."test.status_manager"]
doc = "Statuses."
domain = "statuses"

[types."test.status_manager".fields.statuses]
doc = "The slots."
array = { of = "test.status", length = 60, stride = 16, present = "status_id" }

[types."test.status"]
doc = "A status."
domain = "statuses"

[types."test.status".fields.status_id]
doc = "The status."
sheet = "Status"

[types."test.status".fields.remaining_time]
doc = "Seconds."
unit = "s"

[types."test.game_main"]
doc = "The game."
domain = "location"

[types."test.stage"]
doc = "The stage."
domain = "windows"

[types."test.stage".fields.unit_manager]
doc = "The windows' manager."
ref = "test.unit_manager"

[types."test.unit_manager"]
doc = "The windows' manager."
domain = "windows"

[types."test.unit_manager".fields.unit_count]
doc = "How many."

[types."test.unit_manager".fields.windows]
doc = "The windows."
at = "test.unit_manager.units"
array = { of = "test.window", count = "unit_count", most = 256, stride = 8, pointers = true }

[types."test.window"]
doc = "A window."
domain = "windows"

[types."test.window".fields.name]
doc = "Its name."
text = 32

[types."test.window".fields.value_count]
doc = "How many values."

[types."test.window".fields.values]
doc = "Its values."
array = { of = "test.atk_value", count = "value_count", most = 64, stride = 16 }

[types."test.window".fields.layout]
doc = "Its elements as its layout builds them."
ref = "test.widget"

[types."test.widget"]
doc = "A window's elements."
domain = "windows"

[types."test.widget".fields.element_count]
doc = "How many elements."
at = "test.widget.node_count"

[types."test.widget".fields.elements]
doc = "The elements."
at = "test.widget.nodes"
array = { of = "test.node", count = "element_count", most = 64, stride = 8, pointers = true }

[types."test.node"]
doc = "An element of a window."
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
pointer = "test.utf8_string.string_ptr"
text = 512
set = { call = "test.set_text", hold = true }

[types."test.atk_value"]
doc = "A value."
domain = "windows"

[types."test.atk_value".fields.kind]
doc = "Its kind."

[types."test.atk_value".fields.text]
doc = "Its text."
text = 256

[types."test.utf8_string"]
doc = "A string."
domain = "chat"

[types."test.utf8_string".fields.text]
doc = "Its text."
at = "test.utf8_string.string_ptr"
text = 4096

[types."test.camera_manager"]
doc = "The cameras."
domain = "camera"

[types."test.camera_manager".fields.camera]
doc = "The camera."
ref = "test.camera"

[types."test.camera"]
doc = "A camera."
domain = "camera"

[types."test.camera".fields.max_distance]
doc = "The zoom limit."
unit = "yalms"
set = { range = [1.5, 100.0] }

[types."test.camera".fields.distance]
doc = "The zoom."
unit = "yalms"

[types."test.game_main".fields.current_territory_type]
doc = "The zone."
sheet = "TerritoryType"
"#,
    ),
    (
        "domains.toml",
        r#"
[domains.character]
doc = "The character."
class = "you"
read = { en = "Sees your character", ru = "Видит вашего персонажа" }

[domains.location]
doc = "Where."
class = "you"
read = { en = "Sees where you are", ru = "Видит, где вы" }

[domains.statuses]
doc = "Statuses."
class = "you"
read = { en = "Sees your statuses", ru = "Видит ваши эффекты" }

[domains.actions]
doc = "Actions."
class = "you"
read = { en = "Sees the actions you use", ru = "Видит действия, которые вы используете" }
act = { en = "Uses actions for you", ru = "Использует действия за вас" }

[domains.windows]
doc = "The game's windows."
class = "others"
read = { en = "Sees the game's windows", ru = "Видит окна игры" }
act = { en = "Answers the game's dialogs for you", ru = "Отвечает на диалоги игры за вас" }
change = { en = "Hides parts of the game's windows", ru = "Скрывает части окон игры" }

[domains.chat]
doc = "The chat log."
class = "others"
read = { en = "Sees your chat log", ru = "Видит ваш чат" }
change = { en = "Hides messages in your chat log", ru = "Скрывает сообщения в вашем чате" }

[domains.text]
doc = "The game's text."
class = "display"
read = { en = "Sees the game's text", ru = "Видит тексты игры" }
change = { en = "Changes the game's text", ru = "Меняет тексты игры" }

[domains.camera]
doc = "The camera."
class = "display"
read = { en = "Sees your camera", ru = "Видит вашу камеру" }
change = { en = "Changes your camera's zoom limits", ru = "Меняет пределы приближения камеры" }
"#,
    ),
    (
        "actions.toml",
        r#"
[actions.use_action]
doc = "Uses an action as a hotbar slot does."
call = "test.use_action"
domain = "actions"
rate = 3
ok = true
when = [{ path = "player", says = "you are not in the world" }]

[actions.answer_yes_no]
doc = "Answers the yes or no dialog."
call = "test.fire_callback_int"
domain = "windows"
when = [{ path = "windows.unit_manager.windows[name = \"SelectYesno\"]", says = "no yes or no dialog is open" }]

[actions.answer_yes_no.args.window]
arg = 0
object = "windows.unit_manager.windows[name = \"SelectYesno\"]"

[actions.answer_yes_no.args.answer]
arg = 1
doc = "Yes or no."
enum = "test.yes_no"

[actions.use_action.args.manager]
arg = 0
global = "test.action_manager"

[actions.use_action.args.kind]
arg = 1
doc = "The kind."
enum = "test.action_kind"

[actions.use_action.args.action]
arg = 2
doc = "The id."

[actions.use_action.args.target]
arg = 3
doc = "The target."
default = 0xE0000000
"#,
    ),
    (
        "events.toml",
        r#"
[events.action_used]
doc = "The player asked to use an action."
at = "test.use_action"
domain = "actions"

[events.action_used.args.kind]
doc = "The kind."
arg = 1
enum = "test.action_kind"

[events.action_used.args.action]
doc = "The id."
arg = 2

[events.action_used.args.target]
doc = "The target."
arg = 3

[events.chat_message_added]
doc = "The game adds a message to the chat log."
at = "test.print_message"
domain = "chat"

[events.chat_message_added.args.kind]
doc = "The log kind."
arg = 1
mask = 0x7F

[events.chat_message_added.args.sender]
doc = "The sender."
arg = 2
ref = "test.utf8_string"

[events.chat_message_added.args.message]
doc = "The message."
arg = 3
ref = "test.utf8_string"

[events.chat_message_added.outcomes.hide]
doc = "The message is not added."
returns = 0

[events.sheet_text]
doc = "A text cell of a sheet row the game builds."
at = "excel.hash_table_page_row_resolver.store_row"
derived = "sheet_cell"
domain = "text"
rules = "text-table-v1"

[events.sheet_text.args.sheet]
doc = "The sheet."
from = "sheet"
text = 256

[events.sheet_text.args.row]
doc = "The row."
from = "row"

[events.sheet_text.args.column]
doc = "The column."
from = "column"

[events.sheet_text.args.text]
doc = "The game's text."
from = "text"
text = 4096

[events.sheet_text.outcomes.replace]
doc = "The cell shows the rule's text."
text = 65535

[events.letter_upper]
doc = "A letter the conversion to capitals meets."
at = "text.utf8_string.to_upper"
derived = "case_letter"
domain = "text"

[events.letter_upper.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_upper.outcomes.becomes]
doc = "The conversion gives the rule's letter."
text = 4
same_length = "letter"

[events.letter_lower]
doc = "A letter the conversion to small letters meets."
at = "text.utf8_string.to_lower"
derived = "case_letter"
domain = "text"

[events.letter_lower.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_lower.outcomes.becomes]
doc = "The conversion gives the rule's letter."
text = 4
same_length = "letter"
"#,
    ),
    (
        "enums/test.node_kind.toml",
        "doc = \"Kinds of element.\"\nvalues = { res = 1, text = 3 }\n",
    ),
    (
        "windows/_Exp.toml",
        r#"
window = "_Exp"
doc = "The experience bar."
layout = "ui/uld/Exp.uld"

[elements.window]
kind = "other"
doc = "The whole bar."

[elements.text]
kind = "text"
doc = "The experience text."

[[layouts]]
fingerprint = "50e3bcde560501b1"
elements = { window = 1, text = 4 }
"#,
    ),
    (
        "enums/test.yes_no.toml",
        "doc = \"Answers.\"\nvalues = { yes = 0, no = 1 }\n",
    ),
    (
        "enums/test.action_kind.toml",
        "doc = \"Kinds.\"\nvalues = { action = 1, item = 2, general_action = 5, mount = 13 }\n",
    ),
    (
        "roots.toml",
        r#"
[roots.player]
doc = "The player."
global = "test.local_player"
type = "test.battle_chara"

[roots.game]
doc = "The game."
global = "test.game_main"
type = "test.game_main"

[roots.windows]
doc = "The windows."
global = "test.stage"
type = "test.stage"

[roots.camera]
doc = "The cameras."
global = "test.camera_manager"
type = "test.camera_manager"
"#,
    ),
];

/// The example approved over the character map, without `absent` entries.
fn with_character(manifest: &str, absent: &[&str]) -> TestHost {
    let test = TestHost::new(Limits::default());
    test.use_map(&CHARACTER_MAP, absent);
    approve(test, manifest)
}

/// The example's manifest without its `[lens]` table.
fn without_lens() -> String {
    MANIFEST
        .lines()
        .filter(|line| {
            *line != "[lens]"
                && !line.starts_with("change = ")
                && !line.starts_with("read = ")
                && !line.starts_with("act = ")
                && !line.starts_with("change = ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_message_is_delivered_and_answered_in_the_next_frames() {
    let test = approved(MANIFEST);
    click(&test, "ping");
    assert_eq!(message(&test), "ping sent");
    test.frames(1);
    test.frames(1);
    assert_eq!(message(&test), "hello answered pong");

    let test = approved(&MANIFEST.replace("to = [\"hello\"]", "to = [\"other\"]"));
    click(&test, "ping");
    assert_eq!(
        message(&test),
        "cannot send: hello is not a plugin the manifest lists in [messages] to"
    );
}

#[test]
fn a_greeting_is_copied_for_the_players_click() {
    let test = approved(MANIFEST);
    click(&test, "copy");
    assert_eq!(message(&test), "copied");
    assert_eq!(
        test.host.services().clipboard.take().as_deref(),
        Some("Hello from Angelica")
    );

    let test = approved(&MANIFEST.replace(
        "\"clipboard.write\" = true
",
        "",
    ));
    click(&test, "copy");
    assert_eq!(
        message(&test),
        "cannot copy: the plugin does not hold clipboard.write"
    );
    assert_eq!(test.host.services().clipboard.take(), None);
}

#[test]
fn a_notification_is_shown_under_the_plugins_name() {
    let test = approved(MANIFEST);
    click(&test, "notify");
    assert_eq!(message(&test), "notified");
    let shown = test
        .host
        .services()
        .notifications
        .current(std::time::Instant::now());
    assert_eq!(shown.len(), 1);
    assert_eq!(
        (shown[0].plugin.as_str(), shown[0].text.as_str()),
        ("hello", "Hello from Angelica")
    );
    click(&test, "notify");
    assert_eq!(
        message(&test),
        "cannot notify: a plugin notifies at most once in 2 seconds"
    );
    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert!(
        test.host
            .services()
            .notifications
            .current(std::time::Instant::now())
            .is_empty(),
        "a stopped plugin's notifications leave"
    );

    let test = approved(&MANIFEST.replace(
        "\"notifications\" = true
",
        "",
    ));
    click(&test, "notify");
    assert_eq!(
        message(&test),
        "cannot notify: the plugin does not hold notifications"
    );
}

#[test]
fn a_checked_sound_is_played() {
    let test = approved(MANIFEST);
    click(&test, "beep");
    assert_eq!(message(&test), "beeped");
    {
        let played = test.game.played.lock().unwrap();
        assert_eq!(played.len(), 1);
        let wav = &played[0];
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(
            wav.len(),
            44 + 4410 * 2,
            "16-bit samples as the host wrote them"
        );
    }
    click(&test, "beep");
    assert_eq!(
        message(&test),
        "cannot beep: a plugin plays at most one sound a second"
    );

    let test = approved(&MANIFEST.replace("\"sound.play\" = true\n", ""));
    click(&test, "beep");
    assert_eq!(
        message(&test),
        "cannot beep: the plugin does not hold sound.play"
    );
    assert!(test.game.played.lock().unwrap().is_empty());
}

#[test]
fn the_cache_is_kept_apart_and_removed_with_the_plugin() {
    let test = approved(MANIFEST);
    click(&test, "cache");
    assert_eq!(message(&test), "cached");
    let cached = test.folder.path().join("cache/plugins/hello/hello.txt");
    assert!(cached.is_file(), "{}", cached.display());
    assert!(!test.folder.path().join("storage/hello/cache").exists());
    test.host.queue(Action::Remove {
        id: "hello".into(),
        data: false,
    });
    test.frames(1);
    assert!(!cached.exists(), "the cache goes with the plugin");
    assert!(test.folder.path().join("storage/hello").exists());

    let test = approved(
        &MANIFEST
            .replace(
                "\"storage.cache\" = true
",
                "",
            )
            .replace(
                "[storage]
cache = 1
",
                "",
            ),
    );
    click(&test, "cache");
    assert_eq!(
        message(&test),
        "cannot cache: cache/ is the plugin's cache, which needs storage.cache"
    );
}

/// A WebSocket server on this computer that answers one message with the
/// same text and then reports how the connection ended.
fn echo_once() -> (std::net::SocketAddr, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let first = socket.read().unwrap();
        socket.send(first).unwrap();
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Close(frame)) => {
                    return format!("closed {:?}", frame.map(|frame| u16::from(frame.code)));
                }
                Ok(_) => {}
                Err(error) => return format!("ended: {error}"),
            }
        }
    });
    (address, server)
}

/// Runs frames until the example's message is `expected`, for at most five
/// seconds; the message it shows then.
fn frames_until(test: &TestHost, expected: &str) -> String {
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(5) && message(test) != expected {
        test.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    message(test)
}

#[test]
fn a_socket_echoes_and_closes_through_the_scripted_server() {
    let (address, server) = echo_once();
    let test = approved(MANIFEST);
    *test.game.socket_server.lock().unwrap() = Some(address);
    click(&test, "socket");
    assert_eq!(message(&test), "connecting");
    assert_eq!(
        frames_until(&test, "socket said hello"),
        "socket said hello"
    );
    assert_eq!(server.join().unwrap(), "closed Some(1000)");

    // A stopped plugin's connections close: this server keeps the
    // connection until the host closes it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    *test.game.socket_server.lock().unwrap() = Some(listener.local_addr().unwrap());
    let (greeted, heard) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Text(_)) => greeted.send(()).unwrap(),
                Ok(tungstenite::Message::Close(frame)) => {
                    return frame.map(|frame| u16::from(frame.code));
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    });
    click(&test, "socket");
    let started = std::time::Instant::now();
    while heard.try_recv().is_err() {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        test.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert_eq!(server.join().unwrap(), Some(1001), "going away");

    let test = approved(&MANIFEST.replace(
        "\"http.websocket\" = true
",
        "",
    ));
    click(&test, "socket");
    assert_eq!(
        message(&test),
        "cannot connect: the plugin does not hold http.websocket"
    );
}

#[test]
fn a_mark_is_drawn_around_the_character() {
    use angelica_fake_game::Set;
    let test = with_character(MANIFEST, &[]);
    click(&test, "mark");
    assert_eq!(message(&test), "not in the world");
    test.game.set(
        "player",
        &[
            ("position_x", Set::Float(1.0)),
            ("position_y", Set::Float(0.0)),
            ("position_z", Set::Float(10.0)),
        ],
    );
    click(&test, "mark");
    assert_eq!(message(&test), "marked 1.0, 10.0");
    // A camera at the origin looking down +z.
    let ahead = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
    ];
    let marks = test.host.services().draw.marks(&ahead);
    assert!(marks.len() > 40, "the circle's segments and the label");
    assert!(marks.iter().any(|mark| matches!(
        mark,
        runtime::draw::Mark::Text { text, .. } if text == "here"
    )));
    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert!(
        test.host.services().draw.idle(),
        "a stopped plugin draws nothing"
    );
}

#[test]
fn a_dialog_is_read_and_answered() {
    use angelica_fake_game::Set;
    let test = with_character(MANIFEST, &[]);
    click(&test, "yes");
    assert_eq!(
        message(&test),
        "cannot say yes: action answer_yes_no: no yes or no dialog is open"
    );
    test.game.set(
        r#"windows.unit_manager.windows[name = "SelectYesno"].values[0]"#,
        &[
            ("kind", Set::Integer(8)),
            ("text", Set::Text("Leave the duty?".into())),
        ],
    );
    click(&test, "yes");
    assert_eq!(message(&test), "said yes to Leave the duty?");
    let acted = test.game.acted.lock().unwrap().clone();
    assert_eq!(acted.len(), 1);
    assert_eq!(acted[0].0, "test.fire_callback_int");
    assert_eq!(acted[0].1[1], 0, "yes");
    assert_ne!(acted[0].1[0], 0, "the dialog's address");
}

#[test]
fn a_rule_hides_messages_without_calling_the_plugin() {
    use angelica_fake_game::Fire;
    let kind = |kind: u32| [("kind", Fire::from(kind))];
    let test = approved(MANIFEST);
    click(&test, "quiet");
    assert_eq!(
        message(&test),
        "cannot hide: the game map has no event chat_message_added"
    );

    let test = with_character(MANIFEST, &[]);
    assert_eq!(test.decide("chat_message_added", &kind(57)), None);
    click(&test, "quiet");
    assert_eq!(message(&test), "system messages hidden");
    assert_eq!(*test.game.decided.lock().unwrap(), ["test.print_message"]);
    assert_eq!(test.decide("chat_message_added", &kind(57)), Some(0));
    assert_eq!(
        test.decide("chat_message_added", &kind(0x39 | 0x0880)),
        Some(0),
        "the mask keeps the log kind's bits"
    );
    assert_eq!(test.decide("chat_message_added", &kind(10)), None);

    click(&test, "quiet");
    assert_eq!(message(&test), "system messages shown");
    assert_eq!(
        test.decide("chat_message_added", &kind(57)),
        None,
        "dropping the rule ended it"
    );

    click(&test, "quiet");
    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert_eq!(
        test.decide("chat_message_added", &kind(57)),
        None,
        "a stopped plugin's rules end"
    );

    let test = with_character(&MANIFEST.replace("\"camera\", \"chat\"", "\"camera\""), &[]);
    click(&test, "quiet");
    assert_eq!(
        message(&test),
        "cannot hide: deciding at chat_message_added changes domain chat, which the plugin may not change: list it in the manifest's [lens] change"
    );
}

#[test]
fn the_zoom_limit_is_changed_and_put_back() {
    use angelica_fake_game::Set;
    use runtime::game::Game as _;
    let test = approved(MANIFEST);
    click(&test, "zoom");
    assert_eq!(
        message(&test),
        "cannot zoom: the game map has no root camera"
    );

    let test = with_character(MANIFEST, &[]);
    let limit = || {
        let (schema, _) = test.host.services().lens.schema();
        let read =
            runtime::lens::prepare(&schema, "camera.camera.max_distance", &|_| true).unwrap();
        test.game.query(&read).unwrap()[0].values[0].clone()
    };
    click(&test, "zoom");
    assert_eq!(
        message(&test),
        "zoom limit 40 on 0 cameras",
        "no camera yet"
    );
    test.game
        .set("camera.camera", &[("max_distance", Set::Float(20.0))]);
    click(&test, "zoom");
    assert_eq!(message(&test), "zoom limit 40 on 1 cameras");
    assert_eq!(limit(), Some(runtime::lens::Value::Float(40.0)));

    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert_eq!(
        limit(),
        Some(runtime::lens::Value::Float(20.0)),
        "the original is back when the plugin stops"
    );

    let test = with_character(
        &MANIFEST.replace(
            "change = [\"camera\", \"chat\", \"text\", \"windows\"]\n",
            "",
        ),
        &[],
    );
    test.game
        .set("camera.camera", &[("max_distance", Set::Float(20.0))]);
    click(&test, "zoom");
    assert_eq!(
        message(&test),
        "cannot zoom: test.camera.max_distance is in domain camera, which the plugin may not change: list it in the manifest's [lens] change"
    );
}

#[test]
fn sprint_is_taken_through_the_map_under_its_domain() {
    let test = approved(MANIFEST);
    click(&test, "sprint");
    assert_eq!(
        message(&test),
        "cannot sprint: the game map has no action use_action"
    );

    let test = with_character(MANIFEST, &[]);
    let plugin = test.host.plugin("hello").unwrap();
    assert!(
        plugin
            .asked()
            .iter()
            .any(|asked| asked.name() == "actions.act")
    );
    click(&test, "sprint");
    assert_eq!(
        message(&test),
        "cannot sprint: action use_action: you are not in the world"
    );
    assert!(test.game.acted.lock().unwrap().is_empty());

    test.game.set("player", &[("data.level", 90.into())]);
    click(&test, "sprint");
    assert_eq!(message(&test), "sprinting");
    let acted = test.game.acted.lock().unwrap().clone();
    assert_eq!(acted.len(), 1);
    assert_eq!(acted[0].0, "test.use_action");
    assert_eq!(acted[0].1[1..], [5, 4, 0xE000_0000]);
    assert_ne!(acted[0].1[0], 0, "the action manager's address");

    *test.game.act_returns.lock().unwrap() = 0;
    click(&test, "sprint");
    assert_eq!(
        message(&test),
        "cannot sprint: action use_action: the game refused it now"
    );
    click(&test, "sprint");
    click(&test, "sprint");
    assert_eq!(
        message(&test),
        "cannot sprint: action use_action is at its rate of 3 a second"
    );

    test.host.services().lens.acting.set_stopped(true);
    click(&test, "sprint");
    assert_eq!(
        message(&test),
        "cannot sprint: the player stopped plugins from acting"
    );

    let test = with_character(
        &MANIFEST.replace("act = [\"actions\", \"windows\"]", "act = [\"windows\"]"),
        &[],
    );
    test.game.set("player", &[("data.level", 90.into())]);
    click(&test, "sprint");
    assert_eq!(
        message(&test),
        "cannot sprint: action use_action acts in domain actions: list it in the manifest's [lens] act"
    );
}

#[test]
fn the_character_is_read_through_lens_under_its_domains() {
    let test = approved(MANIFEST);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "cannot read the character: the game map has no root player"
    );

    let test = with_character(MANIFEST, &[]);
    click(&test, "where");
    assert_eq!(message(&test), "no character yet");

    test.game.set(
        "player",
        &[
            ("name", "Alisaie".into()),
            ("data.class_job", 35.into()),
            ("data.level", 100.into()),
            ("data.health", 900.into()),
            ("data.max_health", 1000.into()),
            ("position_x", 1.25.into()),
        ],
    );
    test.game
        .set("game", &[("current_territory_type", 132.into())]);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "Alisaie is class 35 at level 100, HP 900/1000, MP 0/0, at (1.2, 0.0, 0.0) in zone 132"
    );
    test.game.clear("player");
    click(&test, "where");
    assert_eq!(message(&test), "no character yet", "logged out");

    let test = with_character(MANIFEST, &["test.character_data.level"]);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "cannot read the character: test.character_data.level is not available in this game build: map entry test.character_data.level is absent: absent in the scripted build"
    );

    let test = with_character(&without_lens(), &[]);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "cannot read the character: the query reads character, which the plugin may not read: its manifest lists the domains it reads in [lens] read"
    );
}

#[test]
fn arrays_on_the_path_give_one_row_per_element() {
    let test = with_character(MANIFEST, &[]);
    click(&test, "statuses");
    assert_eq!(message(&test), "no statuses");

    test.game.set(
        "player.status_manager.statuses[0]",
        &[("status_id", 48.into()), ("remaining_time", 1799.6.into())],
    );
    test.game.set(
        "player.status_manager.statuses[3]",
        &[("status_id", 1199.into())],
    );
    click(&test, "statuses");
    assert_eq!(message(&test), "statuses: 48 (1800s), 1199");
}

#[test]
fn watched_actions_arrive_in_the_next_frame() {
    use angelica_fake_game::{Fire, Set};
    let test = approved(MANIFEST);
    click(&test, "used");
    assert_eq!(
        message(&test),
        "cannot watch the actions: the game map has no event action_used"
    );

    let test = with_character(MANIFEST, &[]);
    click(&test, "used");
    assert_eq!(message(&test), "no action used yet");
    assert_eq!(
        *test.game.watched.lock().unwrap(),
        ["test.print_message", "test.use_action"],
        "the chat log and the actions"
    );
    let used = |kind: &str, action: u32| {
        test.fire(
            "action_used",
            &[
                ("kind", Fire::Value(Set::Name(kind.into()))),
                ("action", action.into()),
                ("target", Fire::Value(Set::Integer(0x1040_0001))),
            ],
        );
    };
    used("action", 7);
    used("mount", 1);
    test.frames(1);
    click(&test, "used");
    assert_eq!(
        message(&test),
        "used action 7 on 0x10400001",
        "the watch's filter leaves the mount out"
    );
    used("item", 4551);
    test.frames(1);
    click(&test, "used");
    assert_eq!(message(&test), "used item 4551 on 0x10400001");

    let test = TestHost::new(Limits::default());
    test.use_map(&CHARACTER_MAP, &[]);
    test.game
        .watch_available
        .store(false, std::sync::atomic::Ordering::Release);
    let test = approve(test, MANIFEST);
    click(&test, "used");
    assert_eq!(
        message(&test),
        "cannot watch the actions: event action_used cannot be watched: map entry test.use_action cannot be hooked in the scripted build"
    );
}

#[test]
fn approval_names_the_domains_in_the_maps_words() {
    let mut test = TestHost::new(Limits::default());
    test.use_map(&CHARACTER_MAP, &[]);
    test.install(MANIFEST, component()).unwrap();
    let plugin = test.host.plugin("hello").unwrap();
    let words: Vec<String> = plugin
        .asked()
        .iter()
        .map(|asked| runtime::language::Language::English.asked(asked))
        .collect();
    for wording in [
        "Sees your character",
        "Sees where you are",
        "Sees your statuses",
        "Sees the actions you use",
        "Uses actions for you",
    ] {
        assert!(words.iter().any(|word| word == wording), "{words:?}");
    }
    assert!(plugin.ungranted().is_empty());

    // Without the map, the domains cannot be granted.
    let mut bare = TestHost::new(Limits::default());
    bare.install(MANIFEST, component()).unwrap();
    assert_eq!(
        bare.host.plugin("hello").unwrap().ungranted(),
        [
            "actions",
            "character",
            "chat",
            "location",
            "statuses",
            "windows",
            "actions.act",
            "windows.act",
            "camera.change",
            "chat.change",
            "text.change",
            "windows.change"
        ]
    );
}

#[test]
fn chat_messages_are_watched_under_the_chat_domain() {
    use angelica_fake_game::Fire;
    let test = with_character(MANIFEST, &[]);
    click(&test, "heard");
    assert_eq!(message(&test), "nothing heard yet");
    let line = |kind: i64, sender: &str, text: &str| {
        [
            ("kind", Fire::from(kind)),
            ("sender", Fire::String(sender.into())),
            ("message", Fire::String(text.into())),
        ]
    };
    test.fire("chat_message_added", &line(10, "Alphinaud", "Well met!"));
    test.fire("chat_message_added", &line(13, "Alisaie", "Hmph."));
    test.frames(1);
    click(&test, "heard");
    assert_eq!(message(&test), "heard Alisaie (13): Hmph.");

    // Without the domain the watch is refused.
    let test = with_character(
        &MANIFEST.replace("\"chat\", \"location\"", "\"location\""),
        &[],
    );
    let logged = test.host.plugin("hello").unwrap().log.lines();
    assert!(
        logged
            .iter()
            .any(|line| line.text.starts_with("cannot watch the chat")),
        "{logged:?}"
    );
    test.fire("chat_message_added", &line(10, "Alphinaud", "Well met!"));
    test.frames(1);
    click(&test, "heard");
    assert_eq!(message(&test), "nothing heard yet");
}

#[test]
fn a_new_package_replaces_the_running_plugin_without_a_restart() {
    let test = approved(MANIFEST);
    click(&test, "click");
    let old = test.host.plugin("hello").unwrap();

    test.reload(
        &MANIFEST.replace("version = \"0.1.0\"", "version = \"0.1.1\""),
        component(),
    )
    .unwrap();
    let new = test.host.plugin("hello").unwrap();
    assert_eq!(old.status(), Status::Disabled, "the old version stopped");
    assert_eq!(new.status(), Status::Running);
    assert_eq!(new.manifest.plugin.version, "0.1.1");
    assert!(
        test.host
            .diagnostics()
            .iter()
            .any(|line| line == "hello: loaded version 0.1.1")
    );
    test.event("hello", Event::OpenRequested);
    assert!(
        matches!(&new.windows()[0].widgets[6], Widget::Text(clicks) if clicks.text == "1 clicks"),
        "storage is kept"
    );
}

#[test]
fn a_package_that_declares_more_waits_for_approval() {
    let test = approved(&without_permissions(&["sound.play"]));
    test.reload(MANIFEST, component()).unwrap();
    assert_eq!(
        test.host.plugin("hello").unwrap().status(),
        Status::AwaitingApproval
    );
}

#[test]
fn a_package_the_host_refuses_leaves_the_old_one_running() {
    let test = approved(MANIFEST);
    std::fs::write(test.folder.path().join("grants/hello.json"), b"{").unwrap();
    test.reload(MANIFEST, component()).unwrap();
    assert_eq!(test.host.plugin("hello").unwrap().status(), Status::Running);
    assert!(
        test.host
            .diagnostics()
            .iter()
            .any(|line| line.starts_with("reload: hello: grants:")),
        "{:?}",
        test.host.diagnostics()
    );
}

#[test]
fn the_test_kit_drives_windows_jobs_and_restarts() {
    let test = approved(MANIFEST);
    test.edit("hello", "main", "name", "Alisaie");
    assert_eq!(test.texts("hello")[0], "Hello, Alisaie");
    test.toggle("hello", "main", "shout", true);
    assert_eq!(test.texts("hello")[0], "Hello, ALISAIE");
    test.click("hello", "main", "click");
    test.click("hello", "main", "count");
    assert!(test.wait_until(|test| message(test) == "job finished: 9 bytes"));

    // Clicks are saved in storage and the grant persists.
    let test = test.restart().unwrap();
    assert_eq!(test.host.plugin("hello").unwrap().status(), Status::Running);
    test.event("hello", Event::OpenRequested);
    assert!(test.texts("hello").contains(&"1 clicks".to_owned()));
}

#[test]
fn numbers_tabs_tables_tooltips_and_icons() {
    let test = approved(MANIFEST);
    test.set_number("hello", "main", "volume", 0.75);
    assert!(test.texts("hello").contains(&"volume 0.75".to_owned()));
    let widgets = test.host.plugin("hello").unwrap().windows()[0]
        .widgets
        .clone();
    let slider = widgets
        .iter()
        .position(|widget| matches!(widget, Widget::Number(number) if number.slider && number.value.to_bits() == 0.75f64.to_bits()))
        .unwrap();
    assert!(matches!(&widgets[slider + 1], Widget::Tooltip(tip) if tip == "How loud hello is"));

    test.choose("hello", "main", "tabs", 1);
    test.click("hello", "main", "click");
    let widgets = test.host.plugin("hello").unwrap().windows()[0]
        .widgets
        .clone();
    let table = widgets
        .iter()
        .position(|widget| matches!(widget, Widget::BeginTable(table) if table.columns == ["", "Item", "Count"]))
        .unwrap();
    assert!(
        matches!(&widgets[table + 1], Widget::Icon(icon) if icon.icon == 65002 && icon.size == 20)
    );
    assert!(matches!(&widgets[table + 2], Widget::Tooltip(tip) if tip == "Gil"));
    assert!(matches!(&widgets[table + 3], Widget::Text(shown) if shown.text == "Potion"));
    assert!(matches!(&widgets[table + 5], Widget::Text(shown) if shown.text == "1"));
    assert!(matches!(&widgets[table + 6], Widget::EndTable));
}

#[test]
fn removing_a_plugin_deletes_its_approval_and_its_data_only_when_asked() {
    for data in [false, true] {
        let test = approved(MANIFEST);
        if !data {
            // Removed while off; the other one is removed while it runs.
            test.host.queue(Action::Disable("hello".into()));
            test.frames(1);
        }
        let root = test.game_folder().parent().unwrap().to_path_buf();
        let package = root.join("plugins").join("hello");
        std::fs::create_dir_all(&package).unwrap();
        let storage = test
            .host
            .plugin("hello")
            .unwrap()
            .storage
            .root()
            .to_path_buf();
        std::fs::create_dir_all(&storage).unwrap();
        std::fs::write(storage.join("notes.txt"), b"kept").unwrap();
        assert!(root.join("grants").join("hello.json").is_file());

        test.host.queue(Action::Remove {
            id: "hello".into(),
            data,
        });
        test.frames(1);

        assert!(test.host.plugin("hello").is_none());
        assert!(!root.join("grants").join("hello.json").exists());
        assert!(!root.join("state").join("hello.json").exists());
        assert!(!package.exists());
        assert_eq!(storage.join("notes.txt").is_file(), !data, "data: {data}");
    }
}

/// The experience bar's elements, as the example changes them.
const BAR: &str = "windows.unit_manager.windows[name = \"_Exp\"].layout";

/// A host over the character map whose game has the experience bar's
/// layout file, which the map's window schema knows.
fn over_experience_bar(manifest: &str) -> TestHost {
    let game = FakeGame::new("2026.09.15.0000.0000")
        .with_file("ui/uld/Exp.uld", b"uldh0100 experience".to_vec());
    approved_over(&game, manifest, |test| test.use_map(&CHARACTER_MAP, &[]))
}

/// The game builds the experience bar: the whole bar, and its text.
fn build_experience_bar(test: &TestHost, shown: &str) {
    use angelica_fake_game::Set;
    test.game.set(
        &format!("{BAR}.elements[0]"),
        &[
            ("name", 1.into()),
            ("kind", Set::Name("res".into())),
            ("visible", true.into()),
        ],
    );
    test.game.set(
        &format!("{BAR}.elements[1]"),
        &[
            ("name", 4.into()),
            ("kind", Set::Name("text".into())),
            ("visible", true.into()),
            ("text", shown.into()),
        ],
    );
}

/// A value of the experience bar's element `element` as the game holds it;
/// `None` when the element does not have it.
fn bar_value(test: &TestHost, element: &str, field: &str) -> Option<runtime::lens::Value> {
    use runtime::game::Game as _;
    let (schema, _) = test.host.services().lens.schema();
    let read = runtime::lens::prepare(
        &schema,
        &format!("{BAR}.elements[name = {element}].{field}"),
        &|_| true,
    )
    .unwrap();
    test.game
        .query(&read)
        .unwrap()
        .first()
        .and_then(|row| row.values[0].clone())
}

fn bar_shown(test: &TestHost) -> bool {
    bar_value(test, "window", "visible") == Some(runtime::lens::Value::Boolean(true))
}

fn bar_text(test: &TestHost) -> String {
    match bar_value(test, "text", "text") {
        Some(runtime::lens::Value::Text(bytes)) => String::from_utf8(bytes).unwrap(),
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn elements_of_the_games_windows_stay_hidden_until_the_plugin_stops() {
    let test = over_experience_bar(MANIFEST);
    // Before the game builds the bar, the change waits for it.
    click(&test, "hide");
    assert_eq!(message(&test), "experience bar hidden");
    build_experience_bar(&test, "EXP 10/20");
    test.frames(1);
    assert!(!bar_shown(&test));

    // The game shows the bar again; the next frame hides it.
    test.game
        .set(&format!("{BAR}.elements[0]"), &[("visible", true.into())]);
    test.frames(1);
    assert!(!bar_shown(&test));

    click(&test, "hide");
    assert_eq!(message(&test), "experience bar shown");
    assert!(bar_shown(&test));

    click(&test, "hide");
    assert!(!bar_shown(&test));
    test.host.queue(Action::Disable("hello".into()));
    test.frames(2);
    assert!(bar_shown(&test));
}

#[test]
fn text_in_the_games_windows_is_held_against_the_game_until_the_plugin_stops() {
    use angelica_fake_game::Set;
    let test = over_experience_bar(MANIFEST);
    build_experience_bar(&test, "EXP 10/20");
    click(&test, "label");
    assert_eq!(message(&test), "clicks in the experience text");
    assert_eq!(bar_text(&test), "0 clicks");

    click(&test, "click");
    assert_eq!(bar_text(&test), "1 clicks");

    // The game writes its own text again; the next frame puts the
    // plugin's back, and the game's becomes the original.
    test.game.set(
        &format!("{BAR}.elements[1]"),
        &[("text", Set::Text("EXP 11/20".into()))],
    );
    test.frames(1);
    assert_eq!(bar_text(&test), "1 clicks");

    // Only the text element has text: the bar itself is never written.
    assert_eq!(bar_value(&test, "window", "text"), None);

    test.host.queue(Action::Disable("hello".into()));
    test.frames(2);
    assert_eq!(bar_text(&test), "EXP 11/20");
}

#[test]
fn changes_to_the_games_windows_need_the_domain_and_the_names() {
    let without = MANIFEST.replace(
        "change = [\"camera\", \"chat\", \"text\", \"windows\"]",
        "change = [\"camera\", \"chat\", \"text\"]",
    );
    let test = over_experience_bar(&without);
    build_experience_bar(&test, "EXP 10/20");
    click(&test, "hide");
    assert_eq!(
        message(&test),
        "hiding refused: test.node.visible is in domain windows, which the plugin may not change: list it in the manifest's [lens] change"
    );
    click(&test, "label");
    assert!(
        message(&test).starts_with("text refused: test.text_node.text is in domain windows"),
        "{}",
        message(&test)
    );
    assert!(bar_shown(&test));

    // A game build whose layout file of the bar the map does not know.
    let test = with_character(MANIFEST, &[]);
    click(&test, "hide");
    assert!(
        message(&test).starts_with(
            "hiding refused: the elements of window _Exp are not known in this game build"
        ),
        "{}",
        message(&test)
    );
}

#[test]
fn look_watches_dumps_and_counts_for_work_on_the_map() {
    use angelica_fake_game::{Fire, Set};
    use runtime::look::{Mode, REQUEST, RESPONSE, Request, Response};
    let test = with_character(MANIFEST, &[]);
    test.game.set("player", &[("data.level", 90.into())]);
    let folder = test.folder.path().join("look");
    std::fs::create_dir_all(&folder).unwrap();
    test.host.serve_look(folder.clone());
    let ask = |request: &Request| {
        std::fs::write(folder.join(REQUEST), serde_json::to_vec(request).unwrap()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));
        test.frames(1);
    };
    let answer = || {
        serde_json::from_slice::<Response>(&std::fs::read(folder.join(RESPONSE)).unwrap()).unwrap()
    };
    let request = |mode: Mode, query: &str| Request {
        format: 1,
        id: format!("{mode:?}"),
        mode,
        query: query.into(),
        seconds: Some(1),
        bytes: Some(0x30),
        entries: vec!["test.use_action".into()],
    };

    ask(&request(Mode::Dump, "player.data"));
    let dumped = answer().objects.unwrap();
    assert_eq!(dumped.len(), 1);
    let bytes = dumped[0].bytes.as_ref().unwrap();
    assert_eq!(&bytes[0x2B * 2..0x2C * 2], "5a", "level 90 at +0x2B");

    ask(&request(Mode::Watch, "action_used { kind, action }"));
    assert!(
        !folder.join(RESPONSE).exists() || answer().id != "Watch",
        "a watch lasts"
    );
    test.fire(
        "action_used",
        &[
            ("kind", Fire::Value(Set::Name("action".into()))),
            ("action", 7.into()),
        ],
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    test.frames(2);
    let watched = answer();
    assert_eq!(watched.id, "Watch");
    assert_eq!(
        watched.firings.unwrap(),
        [vec![runtime::look::Row {
            indexes: Vec::new(),
            values: vec![serde_json::json!("action"), serde_json::json!(7)],
        }]]
    );

    ask(&request(Mode::Trace, ""));
    for _ in 0..3 {
        test.fire("action_used", &[("action", 1.into())]);
    }
    std::thread::sleep(std::time::Duration::from_millis(1100));
    test.frames(2);
    let traced = answer();
    assert_eq!(traced.id, "Trace");
    assert_eq!(traced.calls.unwrap()["test.use_action"], 3);
}
