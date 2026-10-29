//! Lens as a library in the plugin, over the scripted game: a query reads
//! the player through handles, a watch reads the chat log's messages in a
//! handler at the event's function and hands the rows to the main
//! instance, a rule hides a message by skipping the game's function, and
//! the core refuses a watch of a domain the player did not grant and a
//! rule without the domain to change.
use angelica_fake_game::ui::WidgetRef;
use angelica_fake_game::{TestHost, build_component, maps::EXAMPLE};
use runtime::{
    host::{Action, Event, Status},
    intercepts::{Arg, Returned},
    sandbox::Limits,
};
use std::sync::OnceLock;

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-lens-library"))
}

fn manifest(read: &str, change: &str) -> String {
    format!(
        r#"manifest = 1

[plugin]
id = "lens-library"
name = "Lens library"
version = "0.1.0"
api = "1.0"
description = "Reads, watches, and decides through the Lens library."

[lens]
read = [{read}]
change = [{change}]
"#
    )
}

fn started(read: &str, change: &str) -> TestHost {
    let mut test = TestHost::new(Limits::default());
    test.use_map(&EXAMPLE, &[]);
    test.install(&manifest(read, change), component()).unwrap();
    test.host.queue(Action::Approve("lens-library".into()));
    test.frames(1);
    assert_eq!(
        test.host.plugin("lens-library").unwrap().status(),
        Status::Running
    );
    test
}

fn click(test: &TestHost, id: &str) -> String {
    test.event(
        "lens-library",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: id.into(),
        }),
    );
    let prefix = format!("{}:", id.split(':').next().unwrap_or(id));
    test.host
        .plugin("lens-library")
        .unwrap()
        .log
        .lines()
        .into_iter()
        .map(|line| line.text)
        .rfind(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("the plugin logged nothing for {id}"))
}

/// Waits until the intercept instance planned what the main instance
/// published.
fn prepared(test: &TestHost) {
    let published = std::time::Instant::now();
    while !test.services().intercepts.prepared("lens-library") {
        assert!(published.elapsed() < std::time::Duration::from_secs(60));
        std::thread::yield_now();
    }
}

/// A game string at a new place of the scripted memory: its first word
/// points to `words`.
fn game_string(test: &TestHost, words: &str) -> u64 {
    let mut bytes = words.as_bytes().to_vec();
    bytes.push(0);
    let at = test.place(&bytes);
    let mut object = at.to_le_bytes().to_vec();
    object.resize(0x68, 0);
    test.place(&object)
}

/// The game prints a message of `kind` in the chat log; whether the game's
/// own code ran, and what the function returned.
fn print(test: &TestHost, kind: u64, words: &str) -> (bool, u64) {
    let message = game_string(test, words);
    let mut called = false;
    let (returned, _) = test.call_intercepted(
        "lens-library",
        "test.print_message",
        &[
            Arg::both(0),
            Arg::both(kind),
            Arg::both(0),
            Arg::both(message),
            Arg::both(0),
            Arg::both(0),
        ],
        |_| {
            called = true;
            Returned {
                integer: 1,
                float: 0,
            }
        },
    );
    (called, returned.integer)
}

#[test]
fn a_query_reads_the_player_through_handles() {
    let test = started("\"character\", \"chat\"", "\"chat\"");
    test.game.set(
        "player",
        &[("name", "Alisaie".into()), ("data.level", 90.into())],
    );
    assert_eq!(
        click(&test, "query:player { name, data.level }"),
        "query: Alisaie,90"
    );
}

#[test]
fn a_watch_reads_each_message_in_a_handler_and_hands_the_rows_over() {
    let test = started("\"character\", \"chat\"", "\"chat\"");
    assert_eq!(
        click(&test, "watch:chat_message_added { kind, message.text }"),
        "watch: accepted"
    );
    prepared(&test);
    assert_eq!(print(&test, 57, "hello"), (true, 1));
    assert_eq!(print(&test, 10, "again"), (true, 1));
    assert_eq!(click(&test, "take"), "take: 57,hello; 10,again");
    assert_eq!(click(&test, "take"), "take: ", "taken once");
}

#[test]
fn a_rule_hides_a_message_by_skipping_the_game_function() {
    let test = started("\"character\", \"chat\"", "\"chat\"");
    assert_eq!(
        click(&test, "rule:hide:chat_message_added[kind = 57]"),
        "rule: accepted"
    );
    prepared(&test);
    assert_eq!(
        print(&test, 57, "hidden"),
        (false, 0),
        "the outcome's value"
    );
    assert_eq!(print(&test, 10, "shown"), (true, 1));
    assert_eq!(click(&test, "decided"), "decided: 1");
}

#[test]
fn the_core_refuses_what_the_player_did_not_grant() {
    let test = started("\"character\"", "");
    assert_eq!(
        click(&test, "watch:chat_message_added { kind, message.text }"),
        "watch: PermissionDenied"
    );
    let test = started("\"character\", \"chat\"", "");
    assert_eq!(
        click(&test, "rule:hide:chat_message_added[kind = 57]"),
        "rule: accepted",
        "the intercept is the plugin's to make"
    );
    prepared(&test);
    assert_eq!(
        print(&test, 57, "still shown"),
        (true, 1),
        "without the domain to change, the rule cannot skip the call"
    );
    assert_eq!(click(&test, "decided"), "decided: 0");
}

#[test]
fn a_query_reads_only_the_bags_of_the_domains_the_player_granted() {
    let mut test = TestHost::new(Limits::default());
    test.use_map(&angelica_fake_game::maps::PLAYER_AND_BAGS, &[]);
    test.install(&manifest("\"inventory\"", ""), component())
        .unwrap();
    test.host.queue(Action::Approve("lens-library".into()));
    test.frames(1);
    test.game.set(
        "inventory.containers[kind = bag_1].items[0]",
        &[("item", 5057.into()), ("quantity", 12.into())],
    );
    test.game.set(
        "inventory.containers[kind = armoury_head].items[0]",
        &[("item", 5111.into()), ("quantity", 1.into())],
    );
    assert_eq!(
        click(
            &test,
            "query:inventory.containers.items[item != 0] { item, quantity }"
        ),
        "query: 5057,12",
        "the armoury chest is another domain"
    );
}
