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
    without_permissions(&["http.get"])
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
    let test = approved(MANIFEST);
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
    assert_eq!(
        message(&test),
        "table: 1 rows changed, 1 cells, 1 mismatches"
    );

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
    let test = approved(MANIFEST);
    click(&test, "letters");
    assert_eq!(message(&test), "letters added");
    let mut name = "паладин".as_bytes().to_vec();
    test.services()
        .case
        .letters()
        .to_upper(&mut name, true, false, |_| false);
    assert_eq!(name, "Паладин".as_bytes());

    test.host.queue(Action::Disable("hello".into()));
    test.frames(1);
    assert!(test.services().case.letters().is_empty());
}

#[test]
fn operations_need_their_permission_and_the_game_entries() {
    let without = without_permissions(&["excel.text", "text.case"]);
    let test = approved(&without);
    click(&test, "table");
    assert_eq!(
        message(&test),
        "table refused: the plugin does not hold excel.text"
    );
    click(&test, "letters");
    assert_eq!(
        message(&test),
        "letters refused: the plugin does not hold text.case"
    );

    let test = approved(MANIFEST);
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

#[test]
fn the_characters_state_is_read_under_its_permission() {
    use runtime::state::StateValue;
    let test = approved(MANIFEST);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "cannot read the character: the game map has no state table self"
    );

    let fields: [&str; 10] = [
        "name",
        "class_job",
        "level",
        "health",
        "max_health",
        "mana",
        "max_mana",
        "position_x",
        "position_y",
        "position_z",
    ];
    test.define_state_table("self", "self", &fields, &[]);
    test.define_state_table("self.zone", "self", &["territory_type"], &[]);
    click(&test, "where");
    assert_eq!(message(&test), "no character yet");

    let mut state = test.game.state.lock().unwrap();
    state.insert(
        "self".into(),
        vec![
            [
                ("name".to_owned(), StateValue::Text(b"Alisaie".to_vec())),
                ("class_job".to_owned(), StateValue::Unsigned(35)),
                ("level".to_owned(), StateValue::Unsigned(100)),
                ("health".to_owned(), StateValue::Unsigned(900)),
                ("max_health".to_owned(), StateValue::Unsigned(1000)),
                ("position_x".to_owned(), StateValue::Float(1.25)),
            ]
            .into(),
        ],
    );
    state.insert(
        "self.zone".into(),
        vec![[("territory_type".to_owned(), StateValue::Unsigned(132))].into()],
    );
    drop(state);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "Alisaie is class 35 at level 100, HP 900/1000, MP 0/0, at (1.2, 0.0, 0.0) in zone 132"
    );

    test.define_state_table("self", "self", &fields, &["level"]);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "cannot read the character: state table self, field level: map entry test.level is absent"
    );

    let test = approved(&without_permissions(&["state.self"]));
    test.define_state_table("self", "self", &fields, &[]);
    click(&test, "where");
    assert_eq!(
        message(&test),
        "cannot read the character: the plugin does not hold state.self"
    );
}

#[test]
fn list_tables_give_one_row_per_element() {
    use runtime::state::StateValue;
    let test = approved(MANIFEST);
    test.define_state_table(
        "self.statuses",
        "self",
        &["status_id", "remaining_time"],
        &[],
    );
    click(&test, "statuses");
    assert_eq!(message(&test), "no statuses");

    let row = |id: u64, time: f64| {
        [
            ("status_id".to_owned(), StateValue::Unsigned(id)),
            ("remaining_time".to_owned(), StateValue::Float(time)),
        ]
        .into()
    };
    test.game.state.lock().unwrap().insert(
        "self.statuses".into(),
        vec![row(48, 1799.6), row(1199, 0.0)],
    );
    click(&test, "statuses");
    assert_eq!(message(&test), "statuses: 48 (1800s), 1199");
}

#[test]
fn chat_messages_arrive_in_the_next_frame_under_their_permission() {
    let test = approved(MANIFEST);
    click(&test, "heard");
    assert_eq!(message(&test), "nothing heard yet");
    test.chat_message(10, "Alphinaud", "Well met!");
    test.chat_message(13, "Alisaie", "Hmph.");
    click(&test, "heard");
    assert_eq!(message(&test), "heard Alisaie (13): Hmph.");

    let without = without_permissions(&["chat.read"]);
    let test = with_commands(&without, |_| {});
    assert_eq!(
        message(&test),
        "events refused: the plugin does not hold chat.read"
    );
    test.chat_message(10, "Alphinaud", "Well met!");
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
    let test = approved(&without_permissions(&["state.self"]));
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
