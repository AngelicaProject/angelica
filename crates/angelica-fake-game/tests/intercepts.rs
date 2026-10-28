//! Intercepts against a hostile plugin: a handler that never returns is
//! stopped by the watchdog and disables the plugin's handlers, and a
//! handler cannot reach the host's other services.
use angelica_fake_game::ui::WidgetRef;
use angelica_fake_game::{TestHost, build_component};
use angelica_pe::testing::{CODE, PeBuilder};
use runtime::{
    host::{Action, Event, Status},
    intercepts::{Arg, Returned},
    sandbox::Limits,
};
use std::time::{Duration, Instant};

const HOSTILE: &str = include_str!("../hostile/angelica.toml");
const BASE: u64 = 0x1_4000_0000;
const ENTRIES: [(&str, &str); 2] = [
    (
        "anatomia.toml",
        "format = \"1.0\"\nnamespace = \"hostile\"\n",
    ),
    (
        "functions/game.toml",
        r#"
[function."game.spin"]
type = "fn(ptr)"
thread = "main"
find = [{ scan = "48 89 5C 24 08 57 48 83 EC 20" }]

[function."game.escape"]
type = "fn(u32) -> u32"
thread = "any"
find = [{ scan = "40 53 48 83 EC 30" }]
"#,
    ),
];

fn executable() -> Vec<u8> {
    let mut code = vec![0xCC; 0x100];
    code[..10].copy_from_slice(&[0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20]);
    code[0x40..0x46].copy_from_slice(&[0x40, 0x53, 0x48, 0x83, 0xEC, 0x30]);
    PeBuilder::new()
        .section(".text", CODE, &code)
        .functions(&[(0x1000, 0x1030), (0x1040, 0x1060)])
        .build_mapped()
}

fn started() -> TestHost {
    let mut test = TestHost::new(Limits::default());
    test.use_executable(BASE, executable());
    test.install_with_entries(HOSTILE, &build_component("angelica-hostile"), &ENTRIES)
        .unwrap();
    test.host.queue(Action::Approve("hostile".into()));
    test.frames(1);
    assert_eq!(
        test.host.plugin("hostile").unwrap().status(),
        Status::Running
    );
    test.event(
        "hostile",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: "intercept".into(),
        }),
    );
    test
}

#[test]
fn a_handler_that_never_returns_is_stopped_and_disabled() {
    let test = started();
    let started = Instant::now();
    let (returned, _) = test.call_intercepted("hostile", "game.spin", &[Arg::both(1)], |_| {
        Returned::default()
    });
    assert_eq!(returned, Returned::default(), "the game's call goes on");
    let spent = started.elapsed();
    assert!(
        spent < Duration::from_millis(500),
        "the watchdog stops it at 50 ms: {spent:?}"
    );
    let log = test.host.plugin("hostile").unwrap().log.lines();
    assert!(
        log.iter()
            .any(|line| line.text.contains("the handler of game.spin is disabled")),
        "{log:?}"
    );

    // Its other handlers are disabled with it: the call returns at once.
    let started = Instant::now();
    test.call_intercepted("hostile", "game.escape", &[Arg::both(1)], |_| {
        Returned::default()
    });
    assert!(started.elapsed() < Duration::from_millis(5));
    assert_eq!(test.services().intercepts.cost("hostile").calls, 1);
    assert_eq!(
        test.host.plugin("hostile").unwrap().status(),
        Status::Running,
        "the main instance goes on"
    );
}

#[test]
fn a_handler_reaches_nothing_but_its_call() {
    let test = started();
    let (returned, _) =
        test.call_intercepted("hostile", "game.escape", &[Arg::both(7)], |_| Returned {
            integer: 7,
            float: 0,
        });
    assert_eq!(returned.integer, 7, "the game's result stays");
    let log = test.host.plugin("hostile").unwrap().log.lines();
    assert!(
        log.iter()
            .any(|line| line.text.contains("the handler of game.escape is disabled")),
        "storage is not linked in an intercept instance: {log:?}"
    );
    assert!(!test.folder.path().join("storage/hostile/escaped").exists());
}
