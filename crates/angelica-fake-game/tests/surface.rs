//! The surface example over a scripted game: it draws its window with the
//! interface kit on a surface of its own through the GPU API, the surface
//! takes the mouse only where the window is, and the player's input there
//! reaches the plugin.
use angelica_fake_game::{TestHost, build_component};
use runtime::{
    gpu::{
        Op,
        input::{Button, Input, Modifiers},
    },
    host::{Action, Status},
    sandbox::Limits,
};
use std::sync::OnceLock;

const MANIFEST: &str = include_str!("../../../examples/surface/angelica.toml");

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-surface"))
}

fn started() -> TestHost {
    let mut test = TestHost::new(Limits::default());
    test.host.services().gpu.set_view((1280, 720), None);
    test.install(MANIFEST, component()).unwrap();
    test.host.queue(Action::Approve("surface".into()));
    test.frames(2);
    assert_eq!(
        test.host.plugin("surface").unwrap().status(),
        Status::Running
    );
    test
}

fn press(x: f32, y: f32, pressed: bool) -> Input {
    Input::Button {
        x,
        y,
        button: Button::Left,
        pressed,
        modifiers: Modifiers::default(),
    }
}

#[test]
fn the_kit_draws_a_window_on_the_plugins_surface() {
    let test = started();
    let gpu = &test.host.services().gpu;
    let placed = gpu.placed();
    assert_eq!(placed.len(), 1);
    let surface = &placed[0];
    assert_eq!((surface.area.width, surface.area.height), (1280, 720));
    assert!(!surface.input.is_empty(), "the window takes the mouse");
    test.frames(1);
    let frame = gpu.take();
    let draws = frame
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Render { commands, .. } => Some(commands.len()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        draws.last().is_some_and(|commands| *commands > 3),
        "each frame renders the window: {draws:?}"
    );
}

#[test]
fn the_window_takes_the_mouse_and_the_rest_lets_it_through() {
    let test = started();
    let gpu = &test.host.services().gpu;
    let window = gpu.placed()[0].input[0];
    #[allow(clippy::cast_precision_loss)]
    let (x, y) = (window.x as f32 + 10.0, window.y as f32 + 10.0);
    let inside = gpu.route(&[Input::Moved { x, y }, press(x, y, true)], false, false);
    assert!(inside.mouse);
    assert_eq!(
        gpu.focused("surface"),
        Some(1),
        "a press puts the surface on top and gives it focus"
    );
    let released = gpu.route(&[press(x, y, false)], false, false);
    assert!(released.mouse);
    test.frames(1);
    assert!(
        gpu.take_input("surface").is_empty(),
        "the plugin took its input"
    );
    assert_eq!(
        test.host.plugin("surface").unwrap().status(),
        Status::Running
    );

    let outside = gpu.route(
        &[Input::Moved {
            x: 1270.0,
            y: 710.0,
        }],
        false,
        false,
    );
    assert!(!outside.mouse, "the game gets the mouse outside the window");
    let under_menu = gpu.route(&[Input::Moved { x, y }], true, false);
    assert!(!under_menu.mouse, "Angelica's menu comes first");
}

#[test]
fn a_stopped_plugin_leaves_no_surface() {
    let test = started();
    test.host.queue(Action::Disable("surface".into()));
    test.frames(1);
    let gpu = &test.host.services().gpu;
    assert!(gpu.placed().is_empty());
    assert!(gpu.take().ops.iter().any(|op| matches!(op, Op::Destroy(_))));
}
