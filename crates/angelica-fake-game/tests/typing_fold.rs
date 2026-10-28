//! The typing fold example over a scripted game: it intercepts a function
//! its own Anatomia entries describe, resolved in the game's executable when
//! it starts, and its handler changes the game's string in place in an
//! intercept instance.
use angelica_fake_game::{TestHost, build_component};
use angelica_pe::testing::{CODE, PeBuilder};
use runtime::{
    host::{Action, Status},
    intercepts::{Arg, Returned},
    sandbox::Limits,
};
use std::sync::OnceLock;

const MANIFEST: &str = include_str!("../../../examples/typing-fold/angelica.toml");
const ENTRIES: [(&str, &str); 4] = [
    (
        "anatomia.toml",
        include_str!("../../../examples/typing-fold/anatomia/anatomia.toml"),
    ),
    (
        "functions/minigame.toml",
        include_str!("../../../examples/typing-fold/anatomia/functions/minigame.toml"),
    ),
    (
        "functions/text.toml",
        include_str!("../../../examples/typing-fold/anatomia/functions/text.toml"),
    ),
    (
        "types/text.toml",
        include_str!("../../../examples/typing-fold/anatomia/types/text.toml"),
    ),
];
const BASE: u64 = 0x1_4000_0000;
const NORMALIZE: u32 = 0x1000;
const TO_LOWER: u32 = 0x1400;
/// Calls of the measurement.
const CALLS: u32 = 2_000;

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-typing-fold"))
}

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| {
            if byte == "??" {
                0
            } else {
                u8::from_str_radix(byte, 16).unwrap()
            }
        })
        .collect()
}

/// An executable with the normalizer and the string's lowercase function
/// where the plugin's signatures find them.
fn executable() -> Vec<u8> {
    let mut code = vec![0xCC; 0x600];
    let normalize = hex(
        "48 89 5C 24 18 48 89 7C 24 20 55 48 8D 6C 24 F0 48 81 EC 10 01 00 00 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 00 4C 8B 05 ?? ?? ?? ?? 48 8B DA 48 8B F9 48 8D 15",
    );
    code[..normalize.len()].copy_from_slice(&normalize);
    code[0x177..0x17B].copy_from_slice(&[0x83, 0x7F, 0x40, 0x06]);
    let to_lower = hex("45 33 C0 4C 8B C9 4C 39 41 10 0F 86 ?? ?? ?? ?? 45 8D 50 06");
    let at = (TO_LOWER - 0x1000) as usize;
    code[at..at + to_lower.len()].copy_from_slice(&to_lower);
    let string = hex("49 8B 11 49 03 D0 0F B6 02");
    code[at + 0x20..at + 0x20 + string.len()].copy_from_slice(&string);
    PeBuilder::new()
        .section(".text", CODE, &code)
        .functions(&[(NORMALIZE, NORMALIZE + 0x200), (TO_LOWER, TO_LOWER + 0x80)])
        .build_mapped()
}

/// A running scripted game with the plugin approved.
fn started() -> TestHost {
    let mut test = TestHost::new(Limits::default());
    test.use_executable(BASE, executable());
    test.install_with_entries(MANIFEST, component(), &ENTRIES)
        .unwrap();
    test.host.queue(Action::Approve("typing-fold".into()));
    test.frames(1);
    assert_eq!(
        test.host.plugin("typing-fold").unwrap().status(),
        Status::Running
    );
    test
}

/// A `Utf8String` holding `value`: its bytes and the string object.
fn string(test: &TestHost, value: &str) -> (u64, u64) {
    let mut terminated = value.as_bytes().to_vec();
    terminated.push(0);
    let buffer = test.place(&terminated);
    let mut object = vec![0u8; 0x68];
    object[..8].copy_from_slice(&buffer.to_le_bytes());
    (buffer, test.place(&object))
}

/// The game normalizes the text of the string `object`.
fn normalize(test: &TestHost, object: u64) {
    let (returned, _) = test.call_intercepted(
        "typing-fold",
        "minigame.keybound.normalize",
        &[Arg::both(0x10), Arg::both(object)],
        |_| Returned {
            integer: object,
            float: 0,
        },
    );
    assert_eq!(returned.integer, object, "the game's result stays");
}

#[test]
fn the_handler_folds_the_normalized_text_in_place() {
    let test = started();
    assert_eq!(
        *test.game.intercepted.lock().unwrap(),
        [BASE + u64::from(NORMALIZE)],
        "the function the plugin's own entry finds is hooked"
    );
    let (bytes, object) = string(&test, "Ёлка Титан");
    normalize(&test, object);
    let folded = "елка титан".as_bytes();
    assert_eq!(
        test.bytes_at(bytes, folded.len() + 1),
        [folded, &[0]].concat()
    );

    // Latin is the game's own; nothing changes.
    let (bytes, object) = string(&test, "Iron Giant");
    normalize(&test, object);
    assert_eq!(test.bytes_at(bytes, 10), b"Iron Giant");
}

#[test]
fn a_stopped_plugin_handles_nothing() {
    let test = started();
    let address = test
        .services()
        .intercepts
        .address_of("typing-fold", "minigame.keybound.normalize")
        .unwrap();
    test.host.queue(Action::Disable("typing-fold".into()));
    test.frames(1);
    let (bytes, object) = string(&test, "Титан");
    let mut args = [Arg::both(0x10), Arg::both(object)];
    let decided = test.services().intercepts.call(address, &mut args, |_| {
        unreachable!("with no handler the game's code runs as usual")
    });
    assert_eq!(decided, None);
    assert_eq!(
        test.bytes_at(bytes, 10),
        "Титан".as_bytes(),
        "the game's text stays"
    );
}

/// What a handler costs: the plugin's call in its intercept instance,
/// reading and writing the text through the host.
#[test]
fn a_handler_costs_microseconds() {
    let test = started();
    let (_, object) = string(&test, "Ёлка Титан");
    for _ in 0..CALLS {
        normalize(&test, object);
    }
    let cost = test.services().intercepts.cost("typing-fold");
    assert_eq!(cost.calls, u64::from(CALLS));
    let each = cost.time / CALLS;
    println!("one handler call: {each:?} on average over {CALLS} calls");
    assert!(each < std::time::Duration::from_millis(1), "{each:?}");
}
