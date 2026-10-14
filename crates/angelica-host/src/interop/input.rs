use egui::{Event, Key, Modifiers, PointerButton};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::GetKeyState,
        WindowsAndMessaging::{
            CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetWindowLongPtrW, SetWindowLongPtrW,
            WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFOCUS, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDPROC,
        },
    },
};

pub struct Input {
    pub raw: Mutex<egui::RawInput>,
    /// Whether the Angelica menu is open; the key `key` toggles it.
    pub menu: AtomicBool,
    /// The virtual-key code of the key that opens and closes the menu.
    pub key: AtomicU32,
    /// Whether the menu waits for the player to press its new key.
    pub choosing_key: AtomicBool,
    /// The virtual-key code pressed while the menu waited; 0 for none.
    pub chosen_key: AtomicU32,
    /// The virtual-key code of the key that stops plugins acting.
    pub stop_key: AtomicU32,
    /// Whether the stop key was pressed since the last frame.
    pub stop_pressed: AtomicBool,
    pub mouse: AtomicBool,
    pub keyboard: AtomicBool,
    window: AtomicUsize,
    original: AtomicUsize,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            raw: Mutex::new(egui::RawInput::default()),
            menu: AtomicBool::new(false),
            key: AtomicU32::new(crate::key::MenuKey::default().virtual_key()),
            choosing_key: AtomicBool::new(false),
            chosen_key: AtomicU32::new(0),
            stop_key: AtomicU32::new(crate::key::MenuKey::stop().virtual_key()),
            stop_pressed: AtomicBool::new(false),
            mouse: AtomicBool::new(false),
            keyboard: AtomicBool::new(false),
            window: AtomicUsize::new(0),
            original: AtomicUsize::new(0),
        }
    }
}

impl Input {
    pub fn attach(&self, window: usize) -> Result<(), String> {
        if self.window.load(Ordering::Acquire) != 0 {
            return Ok(());
        }
        let window = window as HWND;
        // SAFETY: window comes from the swap chain's Windows descriptor.
        let original = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) }.cast_unsigned();
        if original == 0 {
            return Err("window has no procedure".into());
        }
        self.original.store(original, Ordering::Release);
        self.window.store(window as usize, Ordering::Release);
        // SAFETY: static procedure meets the WNDPROC ABI and original state is published first.
        let previous =
            unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, procedure as *const () as isize) }
                .cast_unsigned();
        if previous == 0 {
            self.window.store(0, Ordering::Release);
            return Err("SetWindowLongPtrW failed".into());
        }
        self.original.store(previous, Ordering::Release);
        Ok(())
    }

    /// Copy, cut, or paste for Ctrl and the key `wparam`.
    fn clipboard(&self, wparam: usize) -> Option<Event> {
        match wparam {
            0x43 => Some(Event::Copy),
            0x58 => Some(Event::Cut),
            0x56 => super::clipboard::text(self.window()).map(Event::Paste),
            _ => None,
        }
    }

    /// The game window the overlay draws over; null before it is attached.
    pub fn window(&self) -> HWND {
        self.window.load(Ordering::Acquire) as HWND
    }

    pub fn release(&self) {
        self.mouse.store(false, Ordering::Release);
        self.keyboard.store(false, Ordering::Release);
    }

    /// Takes a press of the menu key, which opens or closes the menu, and
    /// any press while the menu waits for its new key.
    fn menu_key(&self, message: u32, wparam: usize, lparam: isize) -> bool {
        // F10 alone arrives as a system key; with Alt held the context bit is set.
        let pressed = message == WM_KEYDOWN
            || (message == WM_SYSKEYDOWN && lparam.cast_unsigned() & (1 << 29) == 0);
        let Ok(code) = u32::try_from(wparam) else {
            return false;
        };
        if !pressed {
            return false;
        }
        if self.choosing_key.load(Ordering::Acquire) {
            self.chosen_key.store(code, Ordering::Release);
            return true;
        }
        if code == self.stop_key.load(Ordering::Acquire) {
            if lparam.cast_unsigned() & (1 << 30) == 0 {
                self.stop_pressed.store(true, Ordering::Release);
            }
            return true;
        }
        if code != self.key.load(Ordering::Acquire) {
            return false;
        }
        if lparam.cast_unsigned() & (1 << 30) == 0 {
            self.menu.fetch_xor(true, Ordering::AcqRel);
            self.release();
        }
        true
    }

    fn event(&self, message: u32, wparam: usize, lparam: isize) -> bool {
        if self.menu_key(message, wparam, lparam) {
            return true;
        }
        let Ok(mut input) = self.raw.try_lock() else {
            return false;
        };
        let modifiers = modifiers();
        input.events.push(Event::ModifiersChanged(modifiers));
        let position = egui::pos2(
            f32::from(signed_word(lparam.cast_unsigned(), 0)),
            f32::from(signed_word(lparam.cast_unsigned(), 2)),
        );
        match message {
            WM_MOUSEMOVE => input.events.push(Event::PointerMoved(position)),
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
            | WM_MBUTTONUP => {
                let button = match message {
                    WM_LBUTTONDOWN | WM_LBUTTONUP => PointerButton::Primary,
                    WM_RBUTTONDOWN | WM_RBUTTONUP => PointerButton::Secondary,
                    _ => PointerButton::Middle,
                };
                input.events.push(Event::PointerButton {
                    pos: position,
                    button,
                    pressed: matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN),
                    modifiers,
                });
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let amount = f32::from(signed_word(wparam, 2)) / 120.;
                let delta = if message == WM_MOUSEWHEEL {
                    egui::vec2(0., amount)
                } else {
                    egui::vec2(amount, 0.)
                };
                input.events.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta,
                    modifiers,
                    phase: egui::TouchPhase::Move,
                });
            }
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                // Keys reach Angelica's windows only while the player types
                // in a text field, and `Esc` while the menu is open: a key
                // meant for the game must not move the focus between
                // Angelica's controls.
                let typing = self.keyboard.load(Ordering::Acquire);
                let closing = wparam == 27 && self.menu.load(Ordering::Acquire);
                if !typing && !closing {
                    return false;
                }
                if message == WM_KEYDOWN
                    && modifiers.ctrl
                    && !modifiers.alt
                    && let Some(event) = self.clipboard(wparam)
                {
                    input.events.push(event);
                }
                if let Some(key) = key(wparam) {
                    input.events.push(Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: message == WM_KEYDOWN || message == WM_SYSKEYDOWN,
                        repeat: lparam.cast_unsigned() & (1 << 30) != 0,
                        modifiers,
                    });
                }
            }
            WM_CHAR => {
                if !self.keyboard.load(Ordering::Acquire) {
                    return wparam == 27 && self.menu.load(Ordering::Acquire);
                }
                if let Some(character) = u32::try_from(wparam)
                    .ok()
                    .and_then(char::from_u32)
                    .filter(|c| !c.is_control())
                {
                    input.events.push(Event::Text(character.to_string()));
                }
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                input.focused = message == WM_SETFOCUS;
                input
                    .events
                    .push(Event::WindowFocused(message == WM_SETFOCUS));
                if message == WM_KILLFOCUS {
                    self.release();
                }
            }
            0x02a3 => input.events.push(Event::PointerGone), // WM_MOUSELEAVE from the Windows SDK.
            _ => return false,
        }
        if input.events.len() > 1024 {
            input.events.clear();
            self.release();
        }
        self.consumes(message, wparam)
    }

    /// Whether the game does not receive a message Angelica took.
    fn consumes(&self, message: u32, wparam: usize) -> bool {
        match message {
            WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL => self.mouse.load(Ordering::Acquire),
            // `Esc` closes the open menu, so the game does not receive it.
            WM_KEYDOWN | WM_CHAR => {
                self.keyboard.load(Ordering::Acquire)
                    || (wparam == 27 && self.menu.load(Ordering::Acquire))
            }
            // The open menu leaves system keys such as Alt+F4 to the game.
            WM_SYSKEYDOWN | WM_SYSKEYUP => {
                self.keyboard.load(Ordering::Acquire) && !self.menu.load(Ordering::Acquire)
            }
            // The game always learns that a key or a mouse button was
            // released, so one it saw go down never stays down for it, as
            // when the camera turns with a button held and the pointer ends
            // over Angelica's window.
            _ => false,
        }
    }
}

fn signed_word(value: usize, at: usize) -> i16 {
    let bytes = value.to_le_bytes();
    i16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn modifiers() -> Modifiers {
    // SAFETY: querying virtual-key state has no pointer or thread preconditions.
    unsafe {
        Modifiers {
            alt: GetKeyState(0x12) < 0,
            ctrl: GetKeyState(0x11) < 0,
            shift: GetKeyState(0x10) < 0,
            mac_cmd: false,
            command: GetKeyState(0x11) < 0,
        }
    }
}
fn key(value: usize) -> Option<Key> {
    Some(match value {
        8 => Key::Backspace,
        9 => Key::Tab,
        13 => Key::Enter,
        27 => Key::Escape,
        32 => Key::Space,
        33 => Key::PageUp,
        34 => Key::PageDown,
        35 => Key::End,
        36 => Key::Home,
        37 => Key::ArrowLeft,
        38 => Key::ArrowUp,
        39 => Key::ArrowRight,
        40 => Key::ArrowDown,
        45 => Key::Insert,
        46 => Key::Delete,
        0x41 => Key::A,
        0x43 => Key::C,
        0x56 => Key::V,
        0x58 => Key::X,
        0x59 => Key::Y,
        0x5a => Key::Z,
        _ => return None,
    })
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let Some(overlay) = super::overlay::OVERLAY.get() else {
        // SAFETY: Windows supplied valid message arguments; fallback has the same ABI.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    let handled = super::guard(false, || {
        overlay.window_frame(message)
            || (overlay.accepts_input() && overlay.input.event(message, wparam, lparam))
    });
    if handled {
        return 0;
    }
    let original = overlay.input.original.load(Ordering::Acquire);
    // SAFETY: original is the retained Windows procedure captured before subclass installation.
    let original: WNDPROC = unsafe { std::mem::transmute(original) };
    // SAFETY: forward the original Windows message arguments unchanged.
    unsafe { CallWindowProcW(original, window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_always_receives_button_releases() {
        let input = Input::default();
        input.mouse.store(true, Ordering::Release);
        for down in [WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE] {
            assert!(input.consumes(down, 0), "{down:#x}");
        }
        for up in [
            WM_LBUTTONUP,
            WM_RBUTTONUP,
            WM_MBUTTONUP,
            WM_KEYUP,
            WM_SYSKEYUP,
        ] {
            assert!(!input.consumes(up, 0), "{up:#x}");
        }
        input.mouse.store(false, Ordering::Release);
        assert!(!input.consumes(WM_LBUTTONDOWN, 0));
    }
}
