//! What the installer asks of Windows. The registry is written
//! with the system's `reg.exe`; the only `unsafe` code starts a copy of the
//! installer with administrator rights, reads the interface language, and
//! opens Windows' folder dialog.
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

/// `CREATE_NO_WINDOW`: helper programs never flash a console.
#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Angelica";

fn reg(args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("reg");
    command.args(args);
    #[cfg(windows)]
    command.creation_flags(NO_WINDOW);
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Lists Angelica under the user's installed apps, with `uninstaller` as
/// the program that removes it.
///
/// # Errors
/// When `reg.exe` refuses a value.
pub fn register(uninstaller: &Path, version: &str) -> Result<(), String> {
    let uninstall = format!("\"{}\" --uninstall", uninstaller.display());
    let icon = uninstaller.display().to_string();
    for (name, kind, value) in [
        ("DisplayName", "REG_SZ", "Angelica"),
        ("DisplayVersion", "REG_SZ", version),
        ("Publisher", "REG_SZ", "AngelicaProject"),
        ("UninstallString", "REG_SZ", uninstall.as_str()),
        ("DisplayIcon", "REG_SZ", icon.as_str()),
        ("NoModify", "REG_DWORD", "1"),
        ("NoRepair", "REG_DWORD", "1"),
    ] {
        reg(&[
            "add",
            UNINSTALL_KEY,
            "/v",
            name,
            "/t",
            kind,
            "/d",
            value,
            "/f",
        ])
        .ok_or_else(|| format!("the registry refused {name}"))?;
    }
    Ok(())
}

/// Removes Angelica from the installed apps.
pub fn unregister() {
    let _ = reg(&["delete", UNINSTALL_KEY, "/f"]);
}

/// The window the player is using now, to own a dialog opened from it.
#[must_use]
pub fn foreground_window() -> isize {
    #[cfg(windows)]
    {
        // SAFETY: GetForegroundWindow takes no arguments and only returns a
        // handle, which may be null.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Windows' own dialog for choosing a folder, over `owner` and opened at
/// `start`; `None` when the player closes it.
#[cfg(windows)]
#[must_use]
pub fn pick_folder(owner: isize, start: Option<&Path>) -> Option<PathBuf> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
        SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
    };
    use windows::core::HSTRING;

    // SAFETY: the calling thread is this function's own and has no COM
    // state yet; it is uninitialized below once the dialog is released.
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let picked = (|| {
        // SAFETY: FileOpenDialog is the class of IFileOpenDialog, created
        // in this process on the thread COM was initialized on.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.ok()?;
        // SAFETY: the dialog is a live object; options only add flags.
        unsafe {
            let options = dialog.GetOptions().ok()?;
            dialog
                .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
                .ok()?;
        }
        if let Some(start) = start {
            // The shell parses only backslashes; Steam's paths have both.
            let wide: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(start.as_os_str())
                .map(|unit| {
                    if unit == u16::from(b'/') {
                        u16::from(b'\\')
                    } else {
                        unit
                    }
                })
                .collect();
            let path = HSTRING::from_wide(&wide);
            // SAFETY: `path` is a valid wide string for the call's length;
            // a folder that no longer exists only leaves the default.
            let item = unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(&path, None) };
            if let Ok(item) = item {
                // SAFETY: `item` is a live shell item.
                let _ = unsafe { dialog.SetFolder(&item) };
            }
        }
        let owner = (owner != 0).then_some(HWND(owner as *mut core::ffi::c_void));
        // SAFETY: the dialog is live and `owner` is the installer's window
        // or none; Show returns an error when it is closed without a choice.
        unsafe { dialog.Show(owner) }.ok()?;
        // SAFETY: Show succeeded, so the dialog has a result.
        let item = unsafe { dialog.GetResult() }.ok()?;
        // SAFETY: `item` is a live shell item; the name it returns is ours
        // to free.
        let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
        // SAFETY: `name` is a null-terminated wide string from the shell.
        let path = unsafe { name.to_string() }.ok();
        // SAFETY: `name` was allocated by the shell with CoTaskMemAlloc.
        unsafe { CoTaskMemFree(Some(name.0.cast())) };
        path.map(PathBuf::from)
    })();
    if initialized {
        // SAFETY: pairs the successful CoInitializeEx above on this thread,
        // after every COM object of the dialog is released.
        unsafe { CoUninitialize() };
    }
    picked
}

#[cfg(not(windows))]
#[must_use]
pub fn pick_folder(_owner: isize, _start: Option<&Path>) -> Option<PathBuf> {
    None
}

/// Whether Windows shows its interface in Russian.
#[must_use]
pub fn russian() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: GetUserDefaultUILanguage takes no arguments and only reads
        // the user's settings.
        let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        // The primary language is the low 10 bits; LANG_RUSSIAN is 0x19.
        language & 0x3ff == 0x19
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Runs this program again with administrator rights and `args`, waits for
/// it, and returns its exit code. Windows asks the player first.
///
/// # Errors
/// When the player declines, or the program cannot start.
#[cfg(windows)]
pub fn run_elevated(args: &str) -> Result<u32, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject},
        UI::{
            Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
            WindowsAndMessaging::SW_HIDE,
        },
    };
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let verb = wide("runas");
    let file = wide(&program.display().to_string());
    let parameters = wide(args);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0),
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        // SAFETY: the rest of SHELLEXECUTEINFOW is plain data for which
        // zero means "not given".
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: `info` is a valid SHELLEXECUTEINFOW whose string pointers
    // point at NUL-terminated buffers that outlive the call.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        return Err("Windows did not start the installer with administrator rights".into());
    }
    let process = info.hProcess;
    if process.is_null() {
        return Err("Windows gave no process to wait for".into());
    }
    // SAFETY: `process` is the handle SEE_MASK_NOCLOSEPROCESS returned; it
    // is waited on, read, and closed once.
    let code = unsafe {
        let waited = WaitForSingleObject(process, INFINITE);
        let mut code = 1u32;
        let read = GetExitCodeProcess(process, &raw mut code);
        CloseHandle(process);
        (waited == WAIT_OBJECT_0 && read != 0).then_some(code)
    };
    code.ok_or_else(|| "the installer with administrator rights did not finish".into())
}

#[cfg(not(windows))]
pub fn run_elevated(_args: &str) -> Result<u32, String> {
    Err("administrator rights are asked for only on Windows".into())
}
