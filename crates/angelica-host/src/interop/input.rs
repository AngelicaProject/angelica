use egui::{Event, Key, Modifiers, PointerButton};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
    pub visible: AtomicBool,
    pub mouse: AtomicBool,
    pub keyboard: AtomicBool,
    window: AtomicUsize,
    original: AtomicUsize,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            raw: Mutex::new(egui::RawInput::default()),
            visible: AtomicBool::new(true),
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

    fn event(&self, message: u32, wparam: usize, lparam: isize) -> bool {
        if message == WM_KEYDOWN && wparam == 0x2d && (lparam.cast_unsigned() & (1 << 30)) == 0 {
            self.visible.fetch_xor(true, Ordering::AcqRel);
            self.release();
            return true;
        }
        if !self.visible.load(Ordering::Acquire) {
            return false;
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
        match message {
            WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP
            | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                self.mouse.load(Ordering::Acquire)
            }
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR => {
                self.keyboard.load(Ordering::Acquire)
            }
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
