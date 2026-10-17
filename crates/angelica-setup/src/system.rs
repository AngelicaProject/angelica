//! What the installer asks of the system. On Windows the registry is
//! written with the system's `reg.exe`; the only `unsafe` code starts a copy
//! of the installer with administrator rights, reads the interface language,
//! opens Windows' folder dialog, and lists the running programs' names. On
//! Linux it reads `/proc` and the locale's variables and starts the
//! desktop's folder dialog, without `unsafe` code.
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

/// `CREATE_NO_WINDOW`: helper programs never flash a console.
#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Angelica";

#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
pub fn unregister() {
    let _ = reg(&["delete", UNINSTALL_KEY, "/f"]);
}

/// The user's home folder.
#[cfg(not(windows))]
#[must_use]
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// The user's data folder: `XDG_DATA_HOME`, or `~/.local/share`.
#[cfg(not(windows))]
#[must_use]
pub fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|data| !data.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".local/share")))
}

/// The desktop entry that removes Angelica, among the user's applications.
#[cfg(not(windows))]
fn desktop_entry() -> Option<PathBuf> {
    data_home().map(|data| data.join("applications/angelica-remove.desktop"))
}

/// `program` as one argument of a desktop entry's `Exec`: in double quotes,
/// with `"`, `` ` ``, `$`, and `\` escaped.
#[cfg(not(windows))]
fn exec_argument(program: &Path) -> String {
    let mut quoted = String::from('"');
    for character in program.display().to_string().chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

/// Lists *Remove Angelica* among the user's applications, starting
/// `uninstaller`.
///
/// # Errors
/// A failure to write the entry.
#[cfg(not(windows))]
pub fn register(uninstaller: &Path, version: &str) -> Result<(), String> {
    let path = desktop_entry().ok_or("there is no home folder")?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Remove Angelica\nName[ru]=Удалить Angelica\nComment=Angelica {version}\nExec={} --uninstall\nTerminal=false\nCategories=Game;\n",
        exec_argument(uninstaller)
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&path, entry).map_err(|e| format!("{}: {e}", path.display()))
}

/// Removes the desktop entry.
#[cfg(not(windows))]
pub fn unregister() {
    if let Some(path) = desktop_entry() {
        let _ = std::fs::remove_file(path);
    }
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

/// The desktop's own dialog for choosing a folder, opened at `start`:
/// KDE's, as on the Steam Deck, or else GNOME's; `None` when the player
/// closes it or neither is installed.
#[cfg(not(windows))]
#[must_use]
pub fn pick_folder(_owner: isize, start: Option<&Path>) -> Option<PathBuf> {
    use std::ffi::OsString;
    let start = start
        .map(Path::to_path_buf)
        .or_else(home)
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut zenity_start = OsString::from("--filename=");
    zenity_start.push(start.join(""));
    let dialogs: [(&str, Vec<OsString>); 2] = [
        (
            "kdialog",
            vec!["--getexistingdirectory".into(), start.into_os_string()],
        ),
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                zenity_start,
            ],
        ),
    ];
    for (program, args) in dialogs {
        let Ok(output) = std::process::Command::new(program).args(args).output() else {
            continue;
        };
        if !output.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_owned();
        return (!path.is_empty()).then(|| PathBuf::from(path));
    }
    None
}

/// The game's program, as Windows names its process.
const GAME_PROGRAM: &str = "ffxiv_dx11.exe";

/// Whether `name`, a running program's file name, is the game's.
fn is_game(name: &str) -> bool {
    name.eq_ignore_ascii_case(GAME_PROGRAM)
}

/// Whether the game is running, from any folder.
///
/// # Errors
/// When Windows does not list the running programs.
#[cfg(windows)]
pub fn game_running() -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    // SAFETY: CreateToolhelp32Snapshot takes plain values and returns a
    // handle or INVALID_HANDLE_VALUE.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("Windows did not list the running programs".into());
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0),
        // SAFETY: the rest of PROCESSENTRY32W is plain data that the calls
        // below fill in.
        ..unsafe { std::mem::zeroed() }
    };
    let mut running = false;
    // SAFETY: `snapshot` is a live snapshot handle and `entry` a
    // PROCESSENTRY32W whose dwSize is set, as both calls require.
    let mut more = unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0;
    while more {
        let length = entry
            .szExeFile
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(entry.szExeFile.len());
        if is_game(&String::from_utf16_lossy(&entry.szExeFile[..length])) {
            running = true;
            break;
        }
        // SAFETY: as for Process32FirstW above.
        more = unsafe { Process32NextW(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: `snapshot` is a live handle, closed once.
    unsafe { CloseHandle(snapshot) };
    Ok(running)
}

/// Whether the game is running in Wine: a process with the game's program
/// as its name, as Wine names it, or in its command line.
///
/// # Errors
/// When `/proc` cannot be read.
#[cfg(not(windows))]
pub fn game_running() -> Result<bool, String> {
    let listing = std::fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?;
    Ok(listing.filter_map(Result::ok).any(|process| {
        let path = process.path();
        std::fs::read_to_string(path.join("comm")).is_ok_and(|name| is_game(name.trim_end()))
            || std::fs::read(path.join("cmdline"))
                .is_ok_and(|line| runs_game(&String::from_utf8_lossy(&line)))
    }))
}

/// Whether a command line runs the game's program. Wine gives a Windows
/// program's whole command line, `Z:\...\ffxiv_dx11.exe <arguments>`, as
/// one argument.
#[cfg(not(windows))]
fn runs_game(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.match_indices(GAME_PROGRAM).any(|(start, found)| {
        let before = line[..start].chars().next_back();
        let after = line[start + found.len()..].chars().next();
        matches!(before, None | Some('/' | '\\' | '"' | '\0'))
            && matches!(after, None | Some('"' | '\0' | ' ' | '\t'))
    })
}

/// Whether Wine runs in `prefix`: Wine's server works in a folder named
/// after the prefix folder's device and inode, `server-<dev>-<ino>`.
#[cfg(not(windows))]
#[must_use]
pub fn prefix_running(prefix: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let Ok(metadata) = std::fs::metadata(prefix) else {
        return false;
    };
    let server = format!("server-{:x}-{:x}", metadata.dev(), metadata.ino());
    let Ok(listing) = std::fs::read_dir("/proc") else {
        return false;
    };
    listing.filter_map(Result::ok).any(|process| {
        std::fs::read_link(process.path().join("cwd"))
            .is_ok_and(|folder| folder.file_name().is_some_and(|name| *name == *server))
    })
}

/// Whether `XIVLauncher.Core`, which starts the game with Dalamud, is
/// installed.
#[cfg(not(windows))]
#[must_use]
pub fn dalamud_installed() -> bool {
    home().is_some_and(|home| {
        home.join(".xlcore").is_dir() || home.join(".var/app/dev.goats.xivlauncher").is_dir()
    })
}

/// Whether `XIVLauncher`, which starts the game with Dalamud, is installed.
#[cfg(windows)]
#[must_use]
pub fn dalamud_installed() -> bool {
    std::env::var_os("APPDATA")
        .is_some_and(|appdata| Path::new(&appdata).join("XIVLauncher").is_dir())
}

/// Whether the system shows its interface in Russian: Windows' language,
/// or the locale on Linux.
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
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(std::env::var_os)
            .find(|value| !value.is_empty())
            .is_some_and(|value| value.to_string_lossy().starts_with("ru"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn the_game_is_found_in_the_command_lines_wine_gives() {
        assert!(runs_game(
            "Z:\\home\\deck\\.steam\\steam\\steamapps\\common\\FINAL FANTASY XIV Online\\game\\ffxiv_dx11.exe DEV.TestSID=0 SYS.Region=3\0"
        ));
        assert!(runs_game("start.exe\0/exec\0./ffxiv_dx11.exe\0"));
        assert!(runs_game("\"C:\\Game\\FFXIV_DX11.EXE\""));
        assert!(!runs_game("Z:\\game\\ffxivboot.exe\0"));
        assert!(!runs_game("/usr/bin/vim notes-ffxiv_dx11.exe.txt\0"));
    }

    #[cfg(not(windows))]
    #[test]
    fn the_uninstaller_is_one_quoted_argument_of_the_desktop_entry() {
        assert_eq!(
            exec_argument(Path::new("/home/deck/.local/share/angelica/angelica-setup")),
            "\"/home/deck/.local/share/angelica/angelica-setup\""
        );
        assert_eq!(
            exec_argument(Path::new("/home/a \"b\" $c/x")),
            r#""/home/a \"b\" \$c/x""#
        );
    }

    #[test]
    fn the_game_is_known_by_its_program_in_any_case() {
        assert!(is_game("ffxiv_dx11.exe"));
        assert!(is_game("FFXIV_DX11.EXE"));
        assert!(!is_game("ffxiv.exe"));
        assert!(!is_game("ffxivboot.exe"));
        assert!(!is_game("XIVLauncher.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn a_running_program_named_as_the_game_is_found() {
        let folder = tempfile::tempdir().unwrap();
        let program = folder.path().join(GAME_PROGRAM);
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(
            std::path::Path::new(&system).join(r"System32\PING.EXE"),
            &program,
        )
        .unwrap();
        let mut child = std::process::Command::new(&program)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let found = game_running();
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found, Ok(true));
    }
}
