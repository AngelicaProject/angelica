//! What the installer asks of Windows. The registry is read and written
//! with the system's `reg.exe`; the only `unsafe` code starts a copy of the
//! installer with administrator rights and reads the interface language.
use std::path::Path;

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

/// The value `name` of `key`, as `reg query` prints it.
fn reg_value(key: &str, name: &str) -> Option<String> {
    let output = reg(&["query", key, "/v", name])?;
    output.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix(name)?.trim_start();
        let (_, value) = rest.split_once("REG_SZ")?;
        Some(value.trim().to_owned())
    })
}

/// Steam's folder.
#[must_use]
pub fn steam_path() -> Option<String> {
    reg_value(r"HKCU\Software\Valve\Steam", "SteamPath")
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
