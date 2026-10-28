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
const FUNCTIONS: &str = r#"
[function."game.add"]
type = "fn(u32, u32) -> u32"
thread = "any"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]

[function."game.mixed"]
type = "fn(f32, f64, u64, u64, u64) -> f64"
thread = "any"
find = [{ scan = "40 53 48 83 EC 30" }]

[function."game.text"]
type = "fn(u32, ptr<char8>) -> ptr"
thread = "any"
find = [{ scan = "48 83 EC 28 4C 8B C2" }]

[function."game.store_row"]
type = "fn(u32, u32, ptr<char8>)"
thread = "any"
find = [{ scan = "48 89 6C 24 10 56 41 54" }]

[function."game.read_file"]
type = "fn(ptr<char8>) -> u32"
thread = "any"
find = [{ scan = "48 89 74 24 18 41 56 48 83 EC 40" }]
"#;

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
    code[0xE0..0xEB].copy_from_slice(&[
        0x48, 0x89, 0x74, 0x24, 0x18, 0x41, 0x56, 0x48, 0x83, 0xEC, 0x40,
    ]);
    PeBuilder::new()
        .section(".text", CODE, &code)
        .functions(&[
            (0x1000, 0x1030),
            (0x1040, 0x1060),
            (0x1080, 0x10A0),
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

/// Rows of a text table and paths of file replacements: as many as an
/// intercept instance reads from the snapshot within one handler's limit.
const ROWS: u32 = 20_000;
const PATHS: u32 = 5_000;
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
/// a path is looked up among the replaced ones. The numbers go to the
/// output; the bounds only catch a handler far off.
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
    let started = std::time::Instant::now();
    store_row(0, 0);
    let first = started.elapsed();
    let hit = average(|index| store_row(index % 100, index));
    assert_eq!(
        test.bytes_at(row_text, 15),
        format!("строка {:08}", CALLS - 1).as_bytes()[..15],
        "the last row's text replaced the game's"
    );
    let miss = average(|index| store_row(1000, index));

    let replaced = test.place(b"chara/equipment/e0042/texture/v01_c0101e0042_top_n.tex\0");
    let other = test.place(b"chara/equipment/e9999/texture/v01_c0101e9999_top_n.tex\0");
    let read_file = |path: u64| {
        test.call_intercepted(
            "intercepting",
            "game.read_file",
            &[Arg::both(path)],
            |_| Returned::default(),
        )
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
        "{ROWS} rows, {PATHS} paths: first call {first:?} (reads the tables); row replaced {hit:?}, row without text {miss:?}; path replaced {found:?}, path not replaced {not_found:?}"
    );
    for each in [hit, miss, found, not_found] {
        assert!(each < std::time::Duration::from_millis(1), "{each:?}");
    }
}
