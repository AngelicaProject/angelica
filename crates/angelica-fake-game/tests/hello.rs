//! The example plugin in a host with a scripted game: approval, windows and
//! events, storage, jobs, text tables, letter case, permissions, and
//! disabling.
use angelica_fake_game::{TestHost, build_component, row_buffer, row_strings, store_row};
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
    let mut test = TestHost::new(Limits::default());
    test.install(manifest, component()).unwrap();
    test.host.queue(Action::Approve("hello".into()));
    test.frames(1);
    assert_eq!(test.host.plugin("hello").unwrap().status(), Status::Running);
    test.event("hello", Event::OpenRequested);
    test
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
        std::fs::read(test.folder.path().join("storage/hello/clicks.txt")).unwrap(),
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
    let without = MANIFEST
        .replace("\"excel.text\" = true\n", "")
        .replace("\"text.case\" = true\n", "");
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
