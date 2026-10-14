//! Plugin commands and messages in the game's chat
//! (`docs/formats/game-structures.md`). The command detour hands a line
//! whose first word is a plugin's command to the host instead of the game;
//! printing builds the game's strings and adds them to the chat log, while
//! watches of the log's events record nothing, so a plugin never hears its
//! own message.
use super::entries::{Entries, Function};
use crate::{catalog::Catalog, commands::Commands};
use angelica_hook::Detour;
use angelica_map::ValueKind::{I32, I64, Ptr, U8, U16, U32, U64, Void};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

const EXECUTE: &str = "shell.command_module.execute_command_inner";
/// The longest line the detour reads; longer lines go to the game.
const MAX_LINE: i64 = 4096;
/// The chat log's Echo channel.
const ECHO: u16 = 56;
/// Memory the adapter gives one game string; the game's constructor uses
/// its first 0x62 bytes.
const STRING: usize = 0x80;

struct Lines {
    commands: Arc<Commands>,
    string: usize,
    used: usize,
}

static LINES: OnceLock<Lines> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

type Execute = unsafe extern "system" fn(usize, usize, usize);

/// Installs the command detour once.
pub fn install_commands(
    catalog: &Catalog,
    base: usize,
    commands: Arc<Commands>,
) -> Result<(), String> {
    if LINES.get().is_some() {
        return Ok(());
    }
    let entries = Entries { catalog, base };
    let lines = Lines {
        commands,
        string: entries.field("text.utf8_string.string_ptr", Ptr)?,
        used: entries.field("text.utf8_string.used_size", I64)?,
    };
    let function = entries.function(EXECUTE, &[Ptr, Ptr, Ptr], Void)?;
    install_resolved(lines, &function)
}

fn install_resolved(lines: Lines, function: &Function) -> Result<(), String> {
    // SAFETY: the map verified this address as the start of a function with
    // this ABI in the executable's code; the replacement has the same ABI.
    let mut detour = unsafe {
        Detour::new(
            function.address,
            execute as Execute as usize,
            function.length,
        )
    }
    .map_err(|e| format!("command detour: {e}"))?;
    ORIGINAL.store(detour.trampoline(), Ordering::Release);
    if LINES.set(lines).is_err() {
        return Ok(());
    }
    // SAFETY: the adapter and the trampoline are published; nothing else in
    // this process changes these bytes meanwhile.
    unsafe { detour.enable() }.map_err(|e| format!("command detour: {e}"))?;
    // Installed until the process exits; without plugin commands it only
    // passes lines on.
    std::mem::forget(detour);
    Ok(())
}

unsafe extern "system" fn execute(module: usize, line: usize, ui: usize) {
    if let Some(lines) = LINES.get()
        && super::guard(false, || lines.take(line))
    {
        return;
    }
    // SAFETY: the trampoline runs the game's function with its own ABI and
    // the game's arguments.
    unsafe {
        std::mem::transmute::<usize, Execute>(ORIGINAL.load(Ordering::Acquire))(module, line, ui);
    }
}

impl Lines {
    /// Whether the host takes the line: the game passes a live string whose
    /// used size counts its terminator.
    fn take(&self, line: usize) -> bool {
        if line == 0 {
            return false;
        }
        // SAFETY: the game passes a live Utf8String; its bytes are read
        // within the used size the string records.
        let text = unsafe {
            let bytes = std::ptr::read_unaligned((line + self.string) as *const usize);
            let used = std::ptr::read_unaligned((line + self.used) as *const i64);
            if bytes == 0 || !(2..=MAX_LINE).contains(&used) {
                return false;
            }
            std::slice::from_raw_parts(bytes as *const u8, usize::try_from(used - 1).unwrap_or(0))
        };
        self.commands.submit(text)
    }
}

/// The functions that print to the chat log.
struct Printer {
    framework: usize,
    ui_module: usize,
    log_module: usize,
    print_message: usize,
    construct: usize,
    destroy: usize,
}

type Getter = unsafe extern "system" fn(usize) -> usize;
type PrintMessage = unsafe extern "system" fn(usize, u16, usize, usize, i32, u8) -> u32;
type Construct = unsafe extern "system" fn(usize, usize, u64) -> usize;
type Destroy = unsafe extern "system" fn(usize);

static PRINTER: OnceLock<Result<Printer, String>> = OnceLock::new();

fn printer(catalog: &Catalog, base: usize) -> Result<Printer, String> {
    let entries = Entries { catalog, base };
    Ok(Printer {
        framework: entries.global("framework.instance", Ptr)?,
        ui_module: entries
            .function("framework.get_ui_module", &[Ptr], Ptr)?
            .address,
        log_module: entries
            .function("ui.ui_module.get_log_module", &[Ptr], Ptr)?
            .address,
        print_message: entries
            .function(
                "ui.log_module.print_message",
                &[Ptr, U16, Ptr, Ptr, I32, U8],
                U32,
            )?
            .address,
        construct: entries
            .function("text.utf8_string.from_sequence", &[Ptr, Ptr, U64], Ptr)?
            .address,
        destroy: entries
            .function("text.utf8_string.dtor", &[Ptr], Void)?
            .address,
    })
}

/// Prints the game's text in the Echo channel. Called on the game's main
/// thread, where the game prints its own messages.
pub fn print(catalog: &Catalog, base: usize, message: &[u8]) -> Result<(), String> {
    let printer = PRINTER
        .get_or_init(|| printer(catalog, base))
        .as_ref()
        .map_err(Clone::clone)?;
    super::guard(Err("printing failed".into()), || printer.print(message))
}

impl Printer {
    fn print(&self, message: &[u8]) -> Result<(), String> {
        // SAFETY: the global holds zero or the live framework, the getters
        // take a live object of their class and return zero or a live
        // module, and every function has the ABI the map verified. The game
        // strings live in zeroed memory larger than the constructor uses,
        // are constructed before the call and destroyed after it; the log
        // copies them.
        unsafe {
            let framework = std::ptr::read_unaligned(self.framework as *const usize);
            if framework == 0 {
                return Err("the chat log does not exist yet".into());
            }
            let ui = std::mem::transmute::<usize, Getter>(self.ui_module)(framework);
            if ui == 0 {
                return Err("the chat log does not exist yet".into());
            }
            let log = std::mem::transmute::<usize, Getter>(self.log_module)(ui);
            if log == 0 {
                return Err("the chat log does not exist yet".into());
            }
            let construct = std::mem::transmute::<usize, Construct>(self.construct);
            let destroy = std::mem::transmute::<usize, Destroy>(self.destroy);
            let mut sender = [0u64; STRING / 8];
            let mut text = [0u64; STRING / 8];
            let sender = sender.as_mut_ptr() as usize;
            let text = text.as_mut_ptr() as usize;
            construct(sender, [0u8].as_ptr() as usize, 0);
            construct(text, message.as_ptr() as usize, message.len() as u64);
            crate::lens::watch::quietly(|| {
                std::mem::transmute::<usize, PrintMessage>(self.print_message)(
                    log, ECHO, sender, text, 0, 0,
                )
            });
            destroy(text);
            destroy(sender);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_command_lines_never_reach_the_game() {
        let folder = tempfile::tempdir().unwrap();
        angelica_sqpack::testing::FakeGame::new("1")
            .with_text(
                "TextCommand",
                &angelica_sqpack::testing::TextSheet::new(1, &[0]).row(1, &[(0, "/tell")]),
            )
            .write(folder.path())
            .unwrap();
        let data = crate::game_data::GameData::new(Some(folder.path()));
        let commands = Arc::new(Commands::default());
        commands
            .define(
                "p",
                vec![crate::commands::Command {
                    path: vec!["hello".into()],
                    aliases: Vec::new(),
                    description: String::new(),
                    parameters: Vec::new(),
                }],
                &data,
            )
            .unwrap();

        // The game's handler counts the lines it runs in the counter its
        // first argument points to: `inc qword [rcx]; nop; nop; ret`.
        let code = [0x48, 0xFF, 0x01, 0x90, 0x90, 0xC3, 0x90, 0x90, 0x90, 0x90];
        let function = Function {
            address: angelica_hook::testing::executable(&code),
            length: Some(code.len()),
        };
        install_resolved(
            Lines {
                commands: commands.clone(),
                string: 0,
                used: 0x10,
            },
            &function,
        )
        .unwrap();
        // SAFETY: the detoured synthetic function has the handler's ABI.
        let run = unsafe { std::mem::transmute::<usize, Execute>(function.address) };
        let send = |line: &str| {
            let mut bytes = line.as_bytes().to_vec();
            bytes.push(0);
            let mut string = [0u64; 4];
            string[0] = bytes.as_ptr() as u64;
            string[2] = bytes.len() as u64;
            let mut ran = 0u64;
            // SAFETY: the counter and the string live across the call.
            unsafe {
                run(
                    std::ptr::from_mut(&mut ran) as usize,
                    string.as_ptr() as usize,
                    0,
                );
            }
            ran
        };
        assert_eq!(send("/hello there"), 0, "the host takes it");
        assert_eq!(send("/tell someone hi"), 1, "the game runs it");
        assert_eq!(send("hello"), 1);
        assert_eq!(commands.take(), vec![b"/hello there".to_vec()]);
    }
}
