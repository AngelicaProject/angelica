//! The player's input on plugins' surfaces: the mouse goes to the topmost
//! surface that takes it at the pointer, unless Angelica's menu takes it;
//! a press keeps the pointer with its surface until the release, puts the
//! surface on top, and gives it focus; the keyboard goes to the focused
//! surface while it takes the keyboard and the menu has no text field in
//! use. What reaches no surface reaches the game.
use super::{Gpu, Id, Placed};

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// The player's input this frame, in pixels of the game's window.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Moved {
        x: f32,
        y: f32,
    },
    Gone,
    Button {
        x: f32,
        y: f32,
        button: Button,
        pressed: bool,
        modifiers: Modifiers,
    },
    Wheel {
        x: f32,
        y: f32,
    },
    Key {
        name: String,
        pressed: bool,
        modifiers: Modifiers,
    },
    Text(String),
}

/// An event on one of a plugin's surfaces, in its own pixels, with the
/// plugin's id of the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Moved {
        surface: u32,
        x: f32,
        y: f32,
    },
    Left(u32),
    Button {
        surface: u32,
        x: f32,
        y: f32,
        button: Button,
        pressed: bool,
        modifiers: Modifiers,
    },
    Wheel {
        surface: u32,
        x: f32,
        y: f32,
    },
    Key {
        surface: u32,
        name: String,
        pressed: bool,
        modifiers: Modifiers,
    },
    Text {
        surface: u32,
        text: String,
    },
    FocusLost(u32),
}

/// Whether the surfaces take the mouse and the keyboard from the game this
/// frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Taken {
    pub mouse: bool,
    pub keyboard: bool,
}

/// Where the pointer is and which surface holds it.
#[derive(Debug, Default)]
pub(super) struct Pointer {
    at: Option<(f32, f32)>,
    hovered: Option<Id>,
    /// The surface a press began on, until the release.
    captured: Option<Id>,
}

/// The topmost of `placed` that takes the mouse at the point.
fn under(placed: &[Placed], x: f32, y: f32) -> Option<&Placed> {
    placed.iter().rev().find(|surface| {
        #[allow(clippy::cast_precision_loss)]
        let (local_x, local_y) = (x - surface.area.x as f32, y - surface.area.y as f32);
        surface.area.contains(x, y)
            && surface
                .input
                .iter()
                .any(|rect| rect.contains(local_x, local_y))
    })
}

impl Gpu {
    /// Routes this frame's input: `menu_pointer` when Angelica's menu takes
    /// the mouse, `menu_keyboard` when a text field of it is in use.
    pub fn route(&self, inputs: &[Input], menu_pointer: bool, menu_keyboard: bool) -> Taken {
        let mut shared = self.shared();
        let mut events: Vec<(Id, Event)> = Vec::new();
        let mut focus = None;
        for input in inputs {
            match input {
                Input::Key {
                    name,
                    pressed,
                    modifiers,
                } => {
                    if let Some(id) = keyboard(&shared, menu_keyboard) {
                        events.push((
                            id,
                            Event::Key {
                                surface: 0,
                                name: name.clone(),
                                pressed: *pressed,
                                modifiers: *modifiers,
                            },
                        ));
                    }
                }
                Input::Text(text) => {
                    if let Some(id) = keyboard(&shared, menu_keyboard) {
                        events.push((
                            id,
                            Event::Text {
                                surface: 0,
                                text: text.clone(),
                            },
                        ));
                    }
                }
                _ => pointer(&mut shared, input, menu_pointer, &mut events, &mut focus),
            }
        }
        let taken = Taken {
            mouse: shared.pointer.captured.is_some() || shared.pointer.hovered.is_some(),
            keyboard: keyboard(&shared, menu_keyboard).is_some(),
        };
        drop(shared);
        if let Some(Focus(target)) = focus
            && let Some(lost) = self.focus(target)
        {
            events.push((lost.id, Event::FocusLost(0)));
        }
        let mut shared = self.shared();
        for (id, event) in events {
            let Some((plugin, surface)) = shared.local_of(id) else {
                continue;
            };
            let event = with_surface(event, surface);
            let queue = shared.input.entry(plugin).or_default();
            if queue.len() < 1024 {
                queue.push(event);
            }
        }
        taken
    }

    /// The input on `plugin`'s surfaces since it last took it.
    pub fn take_input(&self, plugin: &str) -> Vec<Event> {
        self.shared().input.remove(plugin).unwrap_or_default()
    }
}

/// The surface a press gives focus to, none for the game.
#[derive(Debug, Clone, Copy)]
struct Focus(Option<Id>);

/// Where a position of the window is in the surface `id`'s pixels.
fn local(placed: &[Placed], id: Id, x: f32, y: f32) -> Option<(f32, f32)> {
    placed
        .iter()
        .find(|surface| surface.id == id)
        .map(|surface| {
            #[allow(clippy::cast_precision_loss)]
            (x - surface.area.x as f32, y - surface.area.y as f32)
        })
}

/// Routes one input of the mouse; a press notes the surface that takes
/// focus in `focus`.
fn pointer(
    shared: &mut super::Shared,
    input: &Input,
    menu_pointer: bool,
    events: &mut Vec<(Id, Event)>,
    focus: &mut Option<Focus>,
) {
    let placed = shared.placed.clone();
    let under_pointer = |x: f32, y: f32| {
        (!menu_pointer)
            .then(|| under(&placed, x, y).map(|surface| surface.id))
            .flatten()
    };
    match input {
        Input::Moved { x, y } => {
            shared.pointer.at = Some((*x, *y));
            let target = shared.pointer.captured.or_else(|| under_pointer(*x, *y));
            if shared.pointer.hovered != target
                && let Some(old) = shared.pointer.hovered
            {
                events.push((old, Event::Left(0)));
            }
            shared.pointer.hovered = target;
            if let Some(id) = target
                && let Some((x, y)) = local(&placed, id, *x, *y)
            {
                events.push((id, Event::Moved { surface: 0, x, y }));
            }
        }
        Input::Gone => {
            shared.pointer.at = None;
            if let Some(old) = shared.pointer.hovered.take() {
                events.push((old, Event::Left(0)));
            }
        }
        Input::Button {
            x,
            y,
            button,
            pressed,
            modifiers,
        } => {
            let target = if *pressed {
                let target = under_pointer(*x, *y);
                shared.pointer.captured = target;
                *focus = Some(Focus(target));
                target
            } else {
                shared.pointer.captured.take()
            };
            if let Some(id) = target
                && let Some((x, y)) = local(&placed, id, *x, *y)
            {
                events.push((
                    id,
                    Event::Button {
                        surface: 0,
                        x,
                        y,
                        button: *button,
                        pressed: *pressed,
                        modifiers: *modifiers,
                    },
                ));
            }
        }
        Input::Wheel { x, y } => {
            if let Some(id) = shared.pointer.hovered {
                events.push((
                    id,
                    Event::Wheel {
                        surface: 0,
                        x: *x,
                        y: *y,
                    },
                ));
            }
        }
        Input::Key { .. } | Input::Text(_) => {}
    }
}

/// The surface the keyboard goes to.
fn keyboard(shared: &super::Shared, menu_keyboard: bool) -> Option<Id> {
    if menu_keyboard {
        return None;
    }
    let focused = shared.focused?;
    shared
        .placed
        .iter()
        .any(|surface| surface.id == focused && surface.keyboard)
        .then_some(focused)
}

fn with_surface(event: Event, surface: u32) -> Event {
    match event {
        Event::Moved { x, y, .. } => Event::Moved { surface, x, y },
        Event::Left(_) => Event::Left(surface),
        Event::Button {
            x,
            y,
            button,
            pressed,
            modifiers,
            ..
        } => Event::Button {
            surface,
            x,
            y,
            button,
            pressed,
            modifiers,
        },
        Event::Wheel { x, y, .. } => Event::Wheel { surface, x, y },
        Event::Key {
            name,
            pressed,
            modifiers,
            ..
        } => Event::Key {
            surface,
            name,
            pressed,
            modifiers,
        },
        Event::Text { text, .. } => Event::Text { surface, text },
        Event::FocusLost(_) => Event::FocusLost(surface),
    }
}
