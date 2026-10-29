//! Intercepts as the game's hook runs them: `before` handlers change the
//! arguments or skip the game's code, `after` handlers change the result,
//! several plugins' handlers run in the order their manifests ask for, a
//! value a handler gives must have the function's type, a handler that
//! fails leaves the call as it was, and a function that calls itself runs
//! the handlers again.
use angelica_fake_game::ui::WidgetRef;
use angelica_fake_game::{TestHost, build_component};
use angelica_pe::testing::{CODE, PeBuilder};
use runtime::{
    host::{Action, Event, Status},
    intercepts::{Arg, Returned},
    sandbox::Limits,
};
use std::sync::OnceLock;

const BASE: u64 = 0x1_4000_0000;
const FUNCTIONS: &str = include_str!("../intercepting/anatomia/functions/game.toml");
const TYPES: &str = include_str!("../intercepting/anatomia/types/game.toml");

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-intercepting"))
}

fn executable() -> Vec<u8> {
    let mut code = vec![0xCC; 0x100];
    // Each function's own bytes, where its signature finds it.
    code[..10].copy_from_slice(&[0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20]);
    code[0x40..0x46].copy_from_slice(&[0x40, 0x53, 0x48, 0x83, 0xEC, 0x30]);
    code[0x80..0x87].copy_from_slice(&[0x48, 0x83, 0xEC, 0x28, 0x4C, 0x8B, 0xC2]);
    code[0xC0..0xC8].copy_from_slice(&[0x48, 0x89, 0x6C, 0x24, 0x10, 0x56, 0x41, 0x54]);
    code[0x60..0x68].copy_from_slice(&[0x55, 0x48, 0x8B, 0xEC, 0x48, 0x83, 0xEC, 0x50]);
    code[0xA0..0xA8].copy_from_slice(&[0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x48, 0x83]);
    code[0xB0..0xB8].copy_from_slice(&[0x53, 0x56, 0x57, 0x48, 0x83, 0xEC, 0x20, 0x90]);
    code[0xE0..0xEB].copy_from_slice(&[
        0x48, 0x89, 0x74, 0x24, 0x18, 0x41, 0x56, 0x48, 0x83, 0xEC, 0x40,
    ]);
    PeBuilder::new()
        .section(".text", CODE, &code)
        .functions(&[
            (0x1000, 0x1030),
            (0x1040, 0x1060),
            (0x1060, 0x1078),
            (0x1080, 0x10A0),
            (0x10A0, 0x10B0),
            (0x10B0, 0x10C0),
            (0x10C0, 0x10D8),
            (0x10E0, 0x10F8),
        ])
        .build_mapped()
}

fn manifest(id: &str, order: &str) -> String {
    format!(
        r#"manifest = 1

[plugin]
id = "{id}"
name = "Intercepting"
version = "0.1.0"
api = "1.0"
description = "Intercepts the scripted game's functions."

[permissions]
"unreviewed" = true
{order}"#
    )
}

/// A scripted game with the plugins of `ids`, each with its `[intercepts]`
/// table, installed and approved in that order.
fn started(plugins: &[(&str, &str)]) -> TestHost {
    started_with(Limits::default(), plugins)
}

fn started_with(limits: Limits, plugins: &[(&str, &str)]) -> TestHost {
    let mut test = TestHost::new(limits);
    test.use_executable(BASE, executable());
    for (id, order) in plugins {
        let entries = format!("format = \"1.0\"\nnamespace = \"{id}\"\n");
        test.install_with_entries(
            &manifest(id, order),
            component(),
            &[
                ("anatomia.toml", &entries),
                ("functions/game.toml", FUNCTIONS),
                ("types/game.toml", TYPES),
            ],
        )
        .unwrap();
        test.host.queue(Action::Approve((*id).into()));
    }
    test.frames(1);
    for (id, _) in plugins {
        assert_eq!(test.host.plugin(id).unwrap().status(), Status::Running);
    }
    test
}

/// Makes `plugin` intercept `function` in `phase` with `handler`.
fn intercept(test: &TestHost, plugin: &str, phase: &str, function: &str, handler: u32) {
    test.event(
        plugin,
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: format!("{phase}:{function}:{handler}"),
        }),
    );
    let expected = format!("{phase} {function} {handler}: accepted");
    assert!(
        log(test, plugin).contains(&expected),
        "{:?}",
        log(test, plugin)
    );
}

fn log(test: &TestHost, plugin: &str) -> Vec<String> {
    test.host
        .plugin(plugin)
        .unwrap()
        .log
        .lines()
        .into_iter()
        .map(|line| line.text)
        .collect()
}

/// `game.add` as the game's code computes it.
fn add(test: &TestHost, plugin: &str, first: u64, second: u64) -> (u64, Vec<u64>) {
    let (returned, ran) = test.call_intercepted(
        plugin,
        "game.add",
        &[Arg::both(first), Arg::both(second)],
        |args| Returned {
            integer: args[0].int + args[1].int,
            float: 0,
        },
    );
    (returned.integer, ran.iter().map(|arg| arg.int).collect())
}

#[test]
fn before_changes_arguments_and_after_changes_the_result() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.add", 1);
    assert_eq!(
        add(&test, "intercepting", 5, 1),
        (11, vec![10, 1]),
        "the game's code gets the doubled argument"
    );
    intercept(&test, "intercepting", "after", "game.add", 5);
    assert_eq!(add(&test, "intercepting", 5, 1).0, 12);
    assert_eq!(test.services().intercepts.cost("intercepting").calls, 3);
}

#[test]
fn a_skip_returns_at_once_and_after_handlers_are_told() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.add", 3);
    intercept(&test, "intercepting", "after", "game.add", 0);
    let (returned, ran) = test.call_intercepted(
        "intercepting",
        "game.add",
        &[Arg::both(1), Arg::both(2)],
        |_| panic!("the game's code does not run"),
    );
    assert_eq!(returned.integer, 99);
    assert!(ran.is_empty());
    let lines = log(&test, "intercepting");
    assert!(
        lines.iter().any(|line| line.starts_with("Phase::After")
            && line.contains("returned=Value::Unsigned(99)")
            && line.contains("skipped=true")),
        "{lines:?}"
    );
}

#[test]
fn handlers_run_in_the_order_manifests_ask_for() {
    // `second` asks to come after `first`, though it intercepts first.
    let test = started(&[
        ("first", ""),
        ("second", "\n[intercepts]\nafter = [\"first\"]\n"),
    ]);
    intercept(&test, "second", "before", "game.add", 2);
    intercept(&test, "first", "before", "game.add", 1);
    intercept(&test, "second", "after", "game.add", 5);
    intercept(&test, "first", "after", "game.add", 4);
    // Before: first doubles 5, second adds one: 11. The game adds 0: 11.
    // After, from last to first: second adds one, first doubles: 24.
    assert_eq!(add(&test, "first", 5, 0), (24, vec![11, 0]));
}

#[test]
fn wishes_that_contradict_are_ignored_and_logged() {
    let test = started(&[
        ("first", ""),
        ("second", "\n[intercepts]\nafter = [\"first\"]\n"),
        (
            "third",
            "\n[intercepts]\nafter = [\"second\"]\nbefore = [\"first\"]\n",
        ),
    ]);
    intercept(&test, "first", "before", "game.add", 1);
    intercept(&test, "second", "before", "game.add", 2);
    intercept(&test, "third", "before", "game.add", 1);
    // first doubles, second adds one, third, last, doubles: (2x + 1) * 2.
    assert_eq!(add(&test, "first", 5, 0).0, 22);
    assert!(
        log(&test, "third")
            .iter()
            .any(|line| line.contains("contradicts itself")),
        "{:?}",
        log(&test, "third")
    );
}

#[test]
fn a_value_of_another_type_is_refused() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.add", 6);
    assert_eq!(add(&test, "intercepting", 5, 1), (6, vec![5, 1]));
    let lines = log(&test, "intercepting");
    assert!(
        lines.iter().any(|line| line
            == "attempts: ErrorKind::InvalidArgument, ErrorKind::InvalidArgument, ErrorKind::InvalidArgument, ErrorKind::WrongContext, ErrorKind::InvalidArgument, ErrorKind::InvalidArgument"),
        "{lines:?}"
    );
}

#[test]
fn a_failing_handler_leaves_the_call_as_it_was() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.add", 7);
    intercept(&test, "intercepting", "after", "game.add", 4);
    assert_eq!(
        add(&test, "intercepting", 5, 1),
        (6, vec![5, 1]),
        "the argument it changed before failing is dropped, and its other handlers are disabled"
    );
    assert!(
        log(&test, "intercepting")
            .iter()
            .any(|line| line.contains("the handler of game.add is disabled")),
    );
}

#[test]
fn floats_and_stack_arguments_reach_their_places() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.mixed", 8);
    let args = [
        Arg {
            int: 0,
            float: u64::from(1.5f32.to_bits()),
        },
        Arg {
            int: 0,
            float: 0.25f64.to_bits(),
        },
        Arg::both(3),
        Arg::both(4),
        Arg::both(5),
    ];
    let (_, ran) =
        test.call_intercepted("intercepting", "game.mixed", &args, |_| Returned::default());
    assert_eq!(ran[0].float, u64::from(3.0f32.to_bits()));
    assert_eq!(ran[1].float, 0.5f64.to_bits());
    assert_eq!(
        [ran[2].int, ran[3].int, ran[4].int, ran[4].float],
        [3, 4, 6, 6]
    );
}

#[test]
fn a_text_argument_reads_and_its_handle_passes_on() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.text", 9);
    let hello = test.place(b"hello\0");
    let (_, ran) = test.call_intercepted(
        "intercepting",
        "game.text",
        &[Arg::both(1), Arg::both(hello)],
        |_| Returned::default(),
    );
    assert_eq!(ran[1].int, 0, "the handler passed a zero pointer last");
    let lines = log(&test, "intercepting");
    assert!(
        lines.iter().any(|line| line == "text: Ok(\"hello\")"),
        "{lines:?}"
    );
}

#[test]
fn a_function_that_calls_itself_runs_the_handlers_again() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "after", "game.add", 5);
    // The game's code calls the function again once, from inside the call.
    let (returned, _) = test.call_intercepted(
        "intercepting",
        "game.add",
        &[Arg::both(1), Arg::both(1)],
        |args| {
            let inner = add(&test, "intercepting", args[0].int, args[1].int).0;
            Returned {
                integer: inner * 10,
                float: 0,
            }
        },
    );
    // Inner: 1 + 1, plus one: 3. Outer: 30, plus one: 31.
    assert_eq!(returned.integer, 31);
    assert_eq!(test.services().intercepts.cost("intercepting").calls, 2);
}

#[test]
fn a_handler_is_once_per_function_and_phase() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.add", 0);
    intercept(&test, "intercepting", "after", "game.add", 0);
    test.event(
        "intercepting",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: "before:game.add:1".into(),
        }),
    );
    assert!(
        log(&test, "intercepting")
            .iter()
            .any(|line| line == "before game.add 1: ErrorKind::InvalidArgument"),
    );
}

/// Rows of a translation's text table and paths of file replacements, as
/// large as a whole translation's.
const ROWS: u32 = 300_000;
const PATHS: u32 = 50_000;
/// Calls of each measurement.
const CALLS: u32 = 20_000;

/// The average time of `CALLS` calls of `call`.
fn average(mut call: impl FnMut(u32)) -> std::time::Duration {
    let started = std::time::Instant::now();
    for index in 0..CALLS {
        call(index);
    }
    started.elapsed() / CALLS
}

/// What a handler costs on the game's hottest paths: building a row, where
/// a translation replaces its text from a table, and reading a file, where
/// a path is looked up among the replaced ones. The intercept instance
/// reads the tables once, in the background. The numbers go to the output;
/// the bounds only catch a handler far off.
#[test]
fn handlers_on_hot_paths_cost_microseconds() {
    // Building the tables takes the main instance longer than a frame.
    let limits = Limits {
        memory_bytes: 512 * 1024 * 1024,
        fuel: u64::MAX / 4,
        call_time: std::time::Duration::from_secs(30),
        frame_wall: std::time::Duration::from_secs(30),
        ..Limits::default()
    };
    let test = started_with(limits, &[("intercepting", "")]);
    test.event(
        "intercepting",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: format!("tables:{ROWS}:{PATHS}"),
        }),
    );
    let published = std::time::Instant::now();
    while !test.services().intercepts.prepared("intercepting") {
        assert!(published.elapsed() < std::time::Duration::from_secs(60));
        std::thread::yield_now();
    }
    let prepared = published.elapsed();
    assert!(
        log(&test, "intercepting").contains(&"tables: accepted".to_owned()),
        "{:?} {:?}",
        log(&test, "intercepting"),
        test.host.plugin("intercepting").unwrap().status()
    );
    intercept(&test, "intercepting", "before", "game.store_row", 10);
    intercept(&test, "intercepting", "before", "game.read_file", 11);

    let row_text = test.place("строка 00000000\0".as_bytes());
    let store_row = |sheet: u32, row: u32| {
        test.call_intercepted(
            "intercepting",
            "game.store_row",
            &[
                Arg::both(u64::from(sheet)),
                Arg::both(u64::from(row)),
                Arg::both(row_text),
            ],
            |_| Returned::default(),
        );
    };
    let hit = average(|index| store_row(index % 100, index));
    assert_eq!(
        test.bytes_at(row_text, 15),
        format!("строка {:08}", CALLS - 1).as_bytes()[..15],
        "the last row's text replaced the game's"
    );
    let miss = average(|index| store_row(1000, index));

    let replaced = test.place(b"chara/equipment/e0042/texture/v01_c0101e0042_top_n.tex\0");
    let other = test.place(b"chara/weapon/w9999/obj/body/b0001/texture/v01_w9999b0001_n.tex\0");
    let read_file = |path: u64| {
        test.call_intercepted("intercepting", "game.read_file", &[Arg::both(path)], |_| {
            Returned::default()
        })
        .0
        .integer
    };
    assert_eq!(read_file(replaced), 1, "{:?}", log(&test, "intercepting"));
    assert_eq!(read_file(other), 0);
    let found = average(|_| {
        read_file(replaced);
    });
    let not_found = average(|_| {
        read_file(other);
    });
    println!(
        "{ROWS} rows, {PATHS} paths: prepared in the background in {prepared:?}; row replaced {hit:?}, row without text {miss:?}; path replaced {found:?}, path not replaced {not_found:?}"
    );
    for each in [hit, miss, found, not_found] {
        assert!(each < std::time::Duration::from_millis(1), "{each:?}");
    }
}

/// Publishes a snapshot by clicking `id` and waits until it was prepared.
fn publish(test: &TestHost, id: &str) {
    test.event(
        "intercepting",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: id.into(),
        }),
    );
    let published = std::time::Instant::now();
    while !test.services().intercepts.prepared("intercepting") {
        assert!(published.elapsed() < std::time::Duration::from_secs(60));
        std::thread::yield_now();
    }
}

#[test]
fn a_snapshot_that_fails_to_prepare_leaves_handlers_with_the_one_they_had() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.read_file", 11);
    let path = test.place(b"chara/equipment/e0001/texture/v01_c0101e0001_top_n.tex\0");
    let read = || {
        test.call_intercepted("intercepting", "game.read_file", &[Arg::both(path)], |_| {
            Returned::default()
        })
        .0
        .integer
    };
    assert_eq!(read(), 0, "no snapshot yet");
    publish(&test, "tables:0:10");
    assert_eq!(read(), 1, "the prepared instance has the paths");
    publish(&test, "broken");
    assert_eq!(read(), 1, "handlers keep the instance they had");
    let lines = log(&test, "intercepting");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("handlers keep the snapshot they had: the snapshot is not prepared: the snapshot is broken")),
        "{lines:?}"
    );
}

#[test]
fn a_handler_reads_and_writes_the_plugins_own_object_through_its_handle() {
    let test = started(&[("intercepting", "")]);
    intercept(&test, "intercepting", "before", "game.open_box", 12);
    let mut object = vec![0u8; 0x20];
    object[0x8..0xC].copy_from_slice(&41u32.to_le_bytes());
    object[0x10..0x14].copy_from_slice(b"box\0");
    let at = test.place(&object);
    test.call_intercepted("intercepting", "game.open_box", &[Arg::both(at)], |_| {
        Returned::default()
    });
    let lines = log(&test, "intercepting");
    assert!(
        lines.iter().any(|line| line
            == "box: count 41 label BOX hold ErrorKind::WrongContext central ErrorKind::NotFound"),
        "a handler holds nothing, and without a game map no central field exists: {lines:?}"
    );
    let written = test.bytes_at(at, 0x20);
    assert_eq!(written[0x8..0xC], 42u32.to_le_bytes());
    assert_eq!(&written[0x10..0x14], b"BOX\0");
}

/// Where the plugin's functions are in the synthetic executable.
const ADD: u64 = BASE + 0x1000;
const EACH: u64 = BASE + 0x10A0;
const TWICE: u64 = BASE + 0x10B0;

/// Clicks `id` in the plugin's window and gives the line it logged for it.
fn click(test: &TestHost, plugin: &str, id: &str) -> String {
    test.event(
        plugin,
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: id.into(),
        }),
    );
    let prefix = format!("{}:", id.split(':').next().unwrap_or(id));
    log(test, plugin)
        .into_iter()
        .rfind(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("nothing logged for {id}"))
}

/// The game's own functions behind the plugin's entries: `game.add` adds,
/// `game.each` calls its callback with 0 to `count` and sums, and
/// `game.twice` doubles.
fn define_functions(test: &TestHost) {
    let returned = |integer: u64| Returned { integer, float: 0 };
    test.game
        .define_function(ADD, move |_, args| returned(args[0] + args[1]));
    test.game.define_function(EACH, move |game, args| {
        let sum = (0..args[1])
            .map(|index| game.call_thunk(args[0], &[index]).integer)
            .sum();
        returned(sum)
    });
    test.game
        .define_function(TWICE, move |_, args| returned(args[0] * 2));
}

const SERVER: &str = "\"server\" = true\n";

#[test]
fn a_plugin_calls_a_function_of_its_own_entries() {
    let test = started(&[("intercepting", SERVER)]);
    define_functions(&test);
    assert_eq!(click(&test, "intercepting", "add:2:3"), "add: 5");
}

#[test]
fn without_the_server_permission_a_plugin_calls_nothing() {
    let test = started(&[("intercepting", "")]);
    define_functions(&test);
    assert_eq!(
        click(&test, "intercepting", "add:2:3"),
        "add: ErrorKind::PermissionDenied"
    );
    assert_eq!(
        click(&test, "intercepting", "thunk:0"),
        "thunk: ErrorKind::PermissionDenied"
    );
}

#[test]
fn the_game_calls_a_plugins_thunk_back() {
    let test = started(&[("intercepting", SERVER)]);
    define_functions(&test);
    assert_eq!(click(&test, "intercepting", "thunk:0"), "thunk: accepted");
    assert_eq!(
        click(&test, "intercepting", "each:4"),
        "each: 60",
        "0 + 10 + 20 + 30, each from the thunk's handler"
    );
}

#[test]
fn a_failing_thunk_calls_its_fallback() {
    let test = started(&[("intercepting", SERVER)]);
    define_functions(&test);
    assert_eq!(
        click(&test, "intercepting", "thunk:1:game.add"),
        "thunk: ErrorKind::InvalidArgument",
        "a fallback of another type"
    );
    assert_eq!(
        click(&test, "intercepting", "thunk:1:game.twice"),
        "thunk: accepted"
    );
    assert_eq!(
        click(&test, "intercepting", "each:4"),
        "each: 12",
        "0 + 2 + 4 + 6, from the fallback"
    );
    let lines = log(&test, "intercepting");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("the handler of thunk 1 is disabled")),
        "{lines:?}"
    );
}

#[test]
fn a_thunk_of_a_stopped_plugin_returns_zero() {
    let test = started(&[("intercepting", SERVER)]);
    define_functions(&test);
    assert_eq!(click(&test, "intercepting", "thunk:0"), "thunk: accepted");
    let thunk = runtime::intercepts::Intercepts::thunk_of(&test.host.services().intercepts, 0)
        .unwrap()
        .1;
    assert_eq!(test.game.call_thunk(thunk, &[5]).integer, 50);
    test.host.queue(Action::Disable("intercepting".into()));
    test.frames(1);
    assert_eq!(test.game.call_thunk(thunk, &[5]).integer, 0);
}

#[test]
fn a_thunk_that_gives_a_value_of_another_type_returns_zero() {
    let test = started(&[("intercepting", SERVER)]);
    define_functions(&test);
    assert_eq!(click(&test, "intercepting", "thunk:2"), "thunk: accepted");
    assert_eq!(click(&test, "intercepting", "each:3"), "each: 0");
}
