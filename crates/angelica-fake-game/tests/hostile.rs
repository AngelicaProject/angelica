//! A plugin that tries to break the host's boundaries, next to the example
//! plugin: each attack faults the hostile plugin or is refused, and the
//! frame and the example go on.
use angelica_fake_game::{TestHost, build_component};
use runtime::{
    host::{Action, Event, Status},
    sandbox::{
        Limits,
        bindings::angelica::plugin::ui::{Widget, WidgetRef},
    },
};
use std::{sync::OnceLock, time::Duration};

const HOSTILE: &str = include_str!("../hostile/angelica.toml");
const HELLO: &str = include_str!("../../../examples/hello/angelica.toml");

fn component(package: &'static str) -> &'static [u8] {
    static HOSTILE: OnceLock<Vec<u8>> = OnceLock::new();
    static HELLO: OnceLock<Vec<u8>> = OnceLock::new();
    let cell = if package == "angelica-hostile" {
        &HOSTILE
    } else {
        &HELLO
    };
    cell.get_or_init(|| build_component(package))
}

/// Both plugins approved and running.
fn started(limits: Limits) -> TestHost {
    let mut test = TestHost::new(limits);
    test.install(HOSTILE, component("angelica-hostile"))
        .unwrap();
    test.install(HELLO, component("angelica-hello")).unwrap();
    test.host.queue(Action::Approve("hostile".into()));
    test.host.queue(Action::Approve("hello".into()));
    test.frames(1);
    for id in ["hostile", "hello"] {
        assert_eq!(
            test.host.plugin(id).unwrap().status(),
            Status::Running,
            "{id}"
        );
    }
    test
}

fn clicked(id: &str) -> Event {
    Event::Clicked(WidgetRef {
        window: "main".into(),
        id: id.into(),
    })
}

/// Clicks a button of the hostile plugin; the frame runs.
fn click(test: &TestHost, id: &str) {
    test.event("hostile", clicked(id));
}

/// What the hostile plugin's last refused operation reported.
fn report(test: &TestHost) -> String {
    match &test.host.plugin("hostile").unwrap().windows()[0].widgets[0] {
        Widget::Text(label) => label.text.clone(),
        other => panic!("{other:?}"),
    }
}

/// Why the host faulted the hostile plugin; the example keeps running and
/// the next frame runs.
fn faulted(test: &TestHost) -> String {
    let status = test.host.plugin("hostile").unwrap().status();
    let Status::Faulted(reason) = status else {
        panic!("not faulted: {status:?}");
    };
    test.frames(1);
    assert_eq!(test.host.plugin("hello").unwrap().status(), Status::Running);
    reason
}

#[test]
fn a_guest_loop_runs_out_of_time() {
    let test = started(Limits::default());
    click(&test, "spin");
    test.frames(1);
    assert_eq!(faulted(&test), "tick trapped: the call ran out of time");
}

#[test]
fn host_calls_cannot_hold_the_main_thread() {
    let test = started(Limits {
        frame_wall: Duration::from_millis(300),
        ..Limits::default()
    });
    click(&test, "host-loop");
    let started = std::time::Instant::now();
    test.frames(1);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        faulted(&test),
        "tick trapped: the call spent too long in host operations"
    );
}

#[test]
fn the_calls_of_a_frame_share_one_budget() {
    // Only fuel limits these calls.
    let test = started(Limits {
        call_time: Duration::from_secs(60),
        ..Limits::default()
    });
    click(&test, "burn");
    assert_eq!(
        test.host.plugin("hostile").unwrap().status(),
        Status::Running
    );
    for _ in 0..6 {
        test.host.queue_event("hostile", clicked("burn"));
    }
    test.frames(1);
    assert_eq!(faulted(&test), "ui-event trapped: the call ran out of fuel");
}

#[test]
fn memory_stack_and_panics_fault_only_the_plugin() {
    for attack in ["memory", "stack", "panic"] {
        let test = started(Limits::default());
        click(&test, attack);
        test.frames(1);
        let reason = faulted(&test);
        assert!(reason.starts_with("tick trapped: "), "{attack}: {reason}");
    }
}

#[test]
fn invalid_windows_fault_the_plugin() {
    let test = started(Limits::default());
    click(&test, "huge-ui");
    assert!(faulted(&test).starts_with("invalid windows: "));
}

#[test]
fn finished_jobs_count_until_they_are_polled() {
    let test = started(Limits::default());
    click(&test, "jobs");
    // The jobs finish; their outputs wait to be polled.
    std::thread::sleep(Duration::from_millis(500));
    click(&test, "fifth");
    assert!(
        report(&test).starts_with("ErrorKind::LimitExceeded: "),
        "{}",
        report(&test)
    );
}

#[test]
fn cancelled_jobs_stop_counting_once_they_stop() {
    let test = started(Limits::default());
    for _ in 0..8 {
        click(&test, "start-cancel");
        assert_eq!(report(&test), "ok");
        // The cancelled job's thread stops.
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn storage_paths_stay_in_the_folder() {
    let test = started(Limits::default());
    click(&test, "paths");
    let report = report(&test);
    assert_eq!(report.matches("InvalidArgument: ").count(), 6, "{report}");
}

#[test]
fn files_the_host_reads_are_bounded() {
    let test = started(Limits::default());
    let file = std::fs::File::create(test.folder.path().join("storage/hostile/big.table")).unwrap();
    file.set_len(runtime::excel_text::MAX_TABLE_SIZE + 1)
        .unwrap();
    drop(file);
    click(&test, "table");
    assert!(
        report(&test).starts_with("ErrorKind::LimitExceeded: "),
        "{}",
        report(&test)
    );
    click(&test, "replace");
    assert!(
        report(&test).starts_with("ErrorKind::LimitExceeded: "),
        "{}",
        report(&test)
    );
}

#[test]
fn operations_stay_in_their_instance() {
    let test = started(Limits::default());
    click(&test, "http");
    assert!(
        report(&test).starts_with("ErrorKind::WrongContext: "),
        "{}",
        report(&test)
    );
    click(&test, "job-print");
    std::thread::sleep(Duration::from_millis(500));
    click(&test, "poll");
    assert!(
        report(&test).starts_with("ErrorKind::WrongContext: "),
        "{}",
        report(&test)
    );
}

#[test]
fn undeclared_permissions_are_refused() {
    let test = started(Limits::default());
    test.define_state_table("self", "self", &["level"], &[]);
    click(&test, "permissions");
    assert_eq!(
        report(&test).matches("PermissionDenied: ").count(),
        2,
        "{}",
        report(&test)
    );
}
