use super::{input::Input, painter::Painter};
use crate::{host::Host, panel::Panel};
use angelica_hook::VtableSlot;
use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_WARP,
            Direct3D11::{
                D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
                DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
                IDXGISwapChain,
            },
        },
    },
    core::Interface,
};
use windows_sys::Win32::{
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
            GetWindowThreadProcessId, PostMessageW, RegisterClassW, RegisterWindowMessageW,
            WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
};

pub(super) static OVERLAY: OnceLock<Arc<Overlay>> = OnceLock::new();
static PRESENT: AtomicUsize = AtomicUsize::new(0);
static RESIZE: AtomicUsize = AtomicUsize::new(0);

pub struct Overlay {
    host: Arc<Host>,
    map_status: Vec<String>,
    // Retain the COM provider that owns the patched vtable until process exit.
    _bootstrap: IDXGISwapChain,
    pub input: Input,
    disabled: AtomicBool,
    /// Whether frames are drawn; without, `Present` is only passed on.
    attach: bool,
    frames: Mutex<Option<Frames>>,
    frame_message: u32,
    pending_frame: Mutex<Option<f64>>,
}
struct Frames {
    painter: Painter,
    context: egui::Context,
    panel: Panel,
    started: Instant,
    last: Instant,
    window: usize,
    /// Frames Angelica drew into the game's back buffer.
    drawn: u64,
}
pub struct OverlayHooks {
    present: VtableSlot,
    resize: VtableSlot,
}

impl OverlayHooks {
    pub fn install(host: Arc<Host>, map_status: Vec<String>, attach: bool) -> Result<Self, String> {
        let message_name: Vec<_> = "Angelica.Frame.v1\0".encode_utf16().collect();
        // SAFETY: registers a system-allocated message ID for this owned UTF-16 string.
        let frame_message = unsafe { RegisterWindowMessageW(message_name.as_ptr()) };
        if frame_message == 0 {
            return Err("cannot register overlay frame message".into());
        }
        let (window, chain) = dummy_chain()?;
        let vtable = chain.vtable();
        // SAFETY: typed vtable fields are Windows-owned aligned COM slots; callbacks match.
        let mut present = unsafe {
            VtableSlot::new(
                std::ptr::addr_of!(vtable.Present) as usize,
                present as *const () as usize,
            )
        };
        // SAFETY: as above, with the documented ResizeBuffers ABI.
        let mut resize = unsafe {
            VtableSlot::new(
                std::ptr::addr_of!(vtable.ResizeBuffers) as usize,
                resize as *const () as usize,
            )
        };
        PRESENT.store(present.original(), Ordering::Release);
        RESIZE.store(resize.original(), Ordering::Release);
        let overlay = Arc::new(Overlay {
            host,
            map_status,
            _bootstrap: chain.clone(),
            input: Input::default(),
            disabled: AtomicBool::new(false),
            attach,
            frames: Mutex::new(None),
            frame_message,
            pending_frame: Mutex::new(None),
        });
        let published = OVERLAY.set(overlay).is_ok();
        let result = if published {
            // SAFETY: callbacks and previous COM function pointers are published first.
            unsafe { present.enable().and_then(|()| resize.enable()) }.map_err(|e| e.to_string())
        } else {
            Err("overlay is already installed".into())
        };
        drop(chain);
        // SAFETY: this hidden window belongs to this startup call and is no longer used.
        unsafe { DestroyWindow(window as _) };
        if let Err(error) = result {
            if let Some(overlay) = OVERLAY.get() {
                overlay.disabled.store(true, Ordering::Release);
            }
            // SAFETY: rollback restores only slots which still contain our callbacks.
            unsafe {
                let _ = resize.disable();
                let _ = present.disable();
            }
            return Err(error);
        }
        Ok(Self { present, resize })
    }
    pub fn disable(&mut self) -> bool {
        if let Some(overlay) = OVERLAY.get() {
            overlay.disabled.store(true, Ordering::Release);
            overlay.input.release();
        }
        // SAFETY: removal restores only our own slot values and refuses later tools' changes.
        unsafe {
            let resized = self.resize.disable().is_ok();
            let presented = self.present.disable().is_ok();
            resized && presented
        }
    }
}

/// The bootstrap swap chain that finds `Present`, made on the software
/// rasterizer: it is kept for the life of the process, and one kept on the
/// graphics adapter without its window made the game's device fail on a
/// `GeForce` RTX 3060. The swap chain's vtable is DXGI's, the same for both.
fn dummy_chain() -> Result<(usize, IDXGISwapChain), String> {
    create_chain(D3D_DRIVER_TYPE_WARP)
}

pub(super) fn create_chain(
    driver: windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE,
) -> Result<(usize, IDXGISwapChain), String> {
    let class: Vec<_> = "AngelicaOverlayBootstrap\0".encode_utf16().collect();
    // SAFETY: NULL requests the process executable's instance handle.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let registration = WNDCLASSW {
        lpfnWndProc: Some(DefWindowProcW),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        ..Default::default()
    };
    // SAFETY: class string and registration are valid; class procedure is a system function.
    unsafe { RegisterClassW(&raw const registration) };
    // SAFETY: create a private hidden window without user-supplied pointers or callbacks.
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            32,
            32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if window.is_null() {
        return Err(format!(
            "overlay bootstrap window: {}",
            std::io::Error::last_os_error()
        ));
    }
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_DESC {
            Width: 32,
            Height: 32,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: HWND(window),
        Windowed: true.into(),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        Flags: 0,
    };
    let mut chain = None;
    let mut device = None;
    let mut context = None;
    let mut feature = windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL::default();
    // SAFETY: descriptor references our owned window; outputs are valid and no code draws yet.
    let result = unsafe {
        D3D11CreateDeviceAndSwapChain(
            None,
            driver,
            HMODULE(std::ptr::null_mut()),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&raw const desc),
            Some(&raw mut chain),
            Some(&raw mut device),
            Some(&raw mut feature),
            Some(&raw mut context),
        )
    };
    // Request all outputs: some graphics interposers treat a missing device output
    // as a feature-support probe and deliberately do not create the swap chain.
    let result = result.map_err(|e| e.to_string()).and_then(|()| {
        if device.is_none() || context.is_none() {
            return Err("Direct3D returned incomplete device outputs".into());
        }
        chain.ok_or_else(|| "Direct3D returned no swap chain".into())
    });
    match result {
        Ok(chain) => Ok((window as usize, chain)),
        Err(error) => {
            // SAFETY: failed startup still owns this hidden window.
            unsafe { DestroyWindow(window) };
            Err(error)
        }
    }
}

impl Overlay {
    pub(super) fn accepts_input(&self) -> bool {
        !self.disabled.load(Ordering::Acquire)
    }
    pub(super) fn window_frame(&self, message: u32) -> bool {
        if message != self.frame_message {
            return false;
        }
        let elapsed = self
            .pending_frame
            .lock()
            .ok()
            .and_then(|mut frame| frame.take());
        if !self.disabled.load(Ordering::Acquire)
            && let Some(elapsed) = elapsed
        {
            if self.host.on_main_thread() {
                self.host.frame(elapsed);
            } else {
                self.fail("window message thread does not match the game's startup thread");
            }
        }
        true
    }

    fn request_frame(&self, window: usize, elapsed: f64) {
        if self.host.on_main_thread() {
            self.host.frame(elapsed);
            return;
        }
        if let Ok(mut pending) = self.pending_frame.try_lock() {
            let post = pending.is_none();
            *pending = Some(pending.unwrap_or(0.) + elapsed);
            // SAFETY: posts our registered message without guest data or pointers.
            if post && unsafe { PostMessageW(window as _, self.frame_message, 0, 0) } == 0 {
                *pending = None;
            }
        }
    }

    fn frame(&self, chain: &IDXGISwapChain) -> Result<(), String> {
        if self.disabled.load(Ordering::Acquire) || !self.attach {
            return Ok(());
        }
        // SAFETY: typed swap chain descriptor is queried from Windows.
        let description = unsafe { chain.GetDesc() }.map_err(|e| e.to_string())?;
        let mut process = 0;
        // SAFETY: swap-chain window and writable process ID are valid.
        unsafe {
            GetWindowThreadProcessId(description.OutputWindow.0, &raw mut process);
        }
        // SAFETY: obtains the current process's ID without other preconditions.
        if process != unsafe { GetCurrentProcessId() } {
            return Ok(());
        }
        let Ok(mut frames) = self.frames.try_lock() else {
            return Ok(());
        };
        if frames.is_none() {
            let attached = self.attach(chain, description.OutputWindow.0 as usize)?;
            super::log(&format!(
                "overlay attached to a process swap chain: {}",
                attached.painter.describe(chain)
            ));
            *frames = Some(attached);
        }
        let frame = frames.as_mut().ok_or("renderer initialization failed")?;
        if description.OutputWindow.0 as usize != frame.window {
            return Ok(());
        }
        // Says whether the device stopped before Angelica drew into it.
        if let Some(reason) = frame.painter.removed() {
            return Err(removed(frame.drawn, &reason));
        }
        let now = Instant::now();
        let elapsed = now.duration_since(frame.last).as_secs_f64();
        frame.last = now;
        self.request_frame(frame.window, elapsed);
        // SAFETY: DPI query for the swap-chain window obtained from Windows.
        let dpi = unsafe { GetDpiForWindow(description.OutputWindow.0) };
        let scale = (f32::from(u16::try_from(dpi).map_err(|e| e.to_string())?) / 96.).max(1.);
        let mut rect = windows_sys::Win32::Foundation::RECT::default();
        // SAFETY: valid window and writable client rectangle.
        if unsafe { GetClientRect(description.OutputWindow.0, &raw mut rect) } == 0 {
            return Err("GetClientRect failed".into());
        }
        let input = self.raw_input(&rect, scale, now.duration_since(frame.started))?;
        let menu = self.input.menu.load(Ordering::Acquire);
        let panel = &mut frame.panel;
        let chosen = self.input.chosen_key.swap(0, Ordering::AcqRel);
        if chosen != 0 {
            panel.key_pressed(&self.host, chosen);
        }
        let mut keep = menu;
        let mut output = frame.context.run_ui(input, |ui| {
            keep = panel.draw(&self.host, ui.ctx(), menu);
        });
        if menu && !keep {
            self.input.menu.store(false, Ordering::Release);
        }
        let (key, choosing) = panel.menu_key();
        self.input.key.store(key, Ordering::Release);
        self.input.choosing_key.store(choosing, Ordering::Release);
        self.copy(std::mem::take(&mut output.platform_output.commands));
        self.input
            .mouse
            .store(frame.context.egui_wants_pointer_input(), Ordering::Release);
        // The game keeps the keyboard unless the player types in a text
        // field; a pressed button or a chosen row does not take it.
        self.input
            .keyboard
            .store(frame.context.text_edit_focused(), Ordering::Release);
        let primitives = frame
            .context
            .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let result = frame.painter.paint(
            chain,
            &primitives,
            &mut output.textures_delta,
            output.pixels_per_point,
        );
        output.textures_delta.clear();
        if let Some(reason) = frame.painter.removed() {
            return Err(removed(frame.drawn, &reason));
        }
        result?;
        frame.drawn += 1;
        Ok(())
    }
    /// This frame's input, in points of the window's `rect` at `scale`.
    fn raw_input(
        &self,
        rect: &windows_sys::Win32::Foundation::RECT,
        scale: f32,
        time: std::time::Duration,
    ) -> Result<egui::RawInput, String> {
        let mut input = self
            .input
            .raw
            .lock()
            .map_err(|_| "input state poisoned")?
            .take();
        for event in &mut input.events {
            match event {
                egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, .. } => {
                    pos.x /= scale;
                    pos.y /= scale;
                }
                _ => {}
            }
        }
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(
                f32::from(u16::try_from(rect.right - rect.left).map_err(|e| e.to_string())?)
                    / scale,
                f32::from(u16::try_from(rect.bottom - rect.top).map_err(|e| e.to_string())?)
                    / scale,
            ),
        ));
        input.time = Some(time.as_secs_f64());
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale);
        Ok(input)
    }

    /// The renderer, egui with Angelica's fonts and style, and the panel
    /// for the game's `window`, whose input it starts receiving.
    fn attach(&self, chain: &IDXGISwapChain, window: usize) -> Result<Frames, String> {
        let context = egui::Context::default();
        context.set_fonts(crate::fonts::definitions(system_fonts()?));
        crate::theme::apply(&context, crate::theme::Theme::default());
        self.input.attach(window)?;
        Ok(Frames {
            painter: Painter::new(chain)?,
            context,
            panel: Panel::new(&self.host, self.map_status.clone(), system_language()),
            started: Instant::now(),
            last: Instant::now(),
            window,
            drawn: 0,
        })
    }

    /// Puts the text the player copied on the clipboard.
    fn copy(&self, commands: Vec<egui::OutputCommand>) {
        for command in commands {
            if let egui::OutputCommand::CopyText(text) = command {
                super::clipboard::set_text(self.input.window(), &text);
            }
        }
    }

    fn fail(&self, reason: &str) {
        self.disabled.store(true, Ordering::Release);
        self.input.release();
        if let Ok(mut frames) = self.frames.try_lock() {
            *frames = None;
        }
        self.host.diagnostic(format!("overlay disabled: {reason}"));
        super::log(&format!("overlay disabled: {reason}"));
    }
}

/// The language of the system's interface.
fn system_language() -> crate::language::Language {
    // SAFETY: returns the user's interface language without preconditions.
    let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    crate::language::Language::from_windows(language)
}

/// The system's fonts for what Angelica's own fonts do not cover: Segoe UI,
/// and the first Japanese font found.
fn system_fonts() -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::os::windows::ffi::OsStringExt;
    let mut fonts = Vec::new();
    let mut path = vec![0u16; 32768];
    // SAFETY: writable output array has the supplied Windows path capacity.
    let len = unsafe {
        windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW(
            path.as_mut_ptr(),
            32768,
        )
    } as usize;
    if len == 0 || len >= path.len() {
        return Err("Windows font directory is unavailable".into());
    }
    let root = std::path::PathBuf::from(std::ffi::OsString::from_wide(&path[..len])).join("Fonts");
    if let Ok(bytes) = std::fs::read(root.join("segoeui.ttf")) {
        fonts.push(("system-ui".to_owned(), bytes));
    }
    for name in ["meiryo.ttc", "msgothic.ttc", "YuGothR.ttc"] {
        if let Ok(bytes) = std::fs::read(root.join(name)) {
            fonts.push(("japanese".to_owned(), bytes));
            break;
        }
    }
    Ok(fonts)
}

/// `DXGI_PRESENT_TEST`: the game asks whether the window is visible, and
/// nothing is shown.
const PRESENT_TEST: u32 = 0x1;

/// A failure because the game's device stopped, with when it stopped.
fn removed(drawn: u64, reason: &str) -> String {
    if drawn == 0 {
        format!("the game's Direct3D device stopped before Angelica drew a frame: {reason}")
    } else {
        format!("the game's Direct3D device stopped after Angelica drew {drawn} frames: {reason}")
    }
}

unsafe extern "system" fn present(raw: *mut c_void, interval: u32, flags: u32) -> i32 {
    if flags & PRESENT_TEST == 0
        && let Some(overlay) = OVERLAY.get()
    {
        let completed = super::guard(false, || {
            // SAFETY: Present supplies a live IDXGISwapChain COM pointer for this call.
            if let Some(chain) = unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }
                && let Err(error) = overlay.frame(chain)
            {
                overlay.fail(&error);
            }
            true
        });
        if !completed {
            overlay.fail("panic in frame callback");
        }
    }
    // SAFETY: captured original function has this exact COM ABI and remains mapped.
    let original: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 =
        unsafe { std::mem::transmute(PRESENT.load(Ordering::Acquire)) };
    // SAFETY: pass the COM call unchanged, including any earlier tool's handler.
    unsafe { original(raw, interval, flags) }
}

unsafe extern "system" fn resize(
    raw: *mut c_void,
    count: u32,
    width: u32,
    height: u32,
    format: i32,
    flags: u32,
) -> i32 {
    super::guard((), || {
        if let Some(overlay) = OVERLAY.get() {
            if let Ok(mut frames) = overlay.frames.lock() {
                if frames.is_some() {
                    super::log(&format!(
                        "the game resizes its swap chain: {width}x{height}, format {format}, {count} buffers, flags {flags:#x}"
                    ));
                }
                *frames = None;
            }
            overlay.input.release();
        }
    });
    // SAFETY: original COM ResizeBuffers function has this exact ABI and is retained.
    let original: unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32) -> i32 =
        unsafe { std::mem::transmute(RESIZE.load(Ordering::Acquire)) };
    // SAFETY: forward original arguments after releasing every owned back-buffer reference.
    unsafe { original(raw, count, width, height, format, flags) }
}
