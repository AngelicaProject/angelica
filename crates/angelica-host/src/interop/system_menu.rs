//! Angelica's entry in the game's System menu
//! (`docs/formats/game-structures.md`). The detour of the HUD agent's
//! `OpenSystemMenu` gives the game a copy of the menu's entries with
//! Angelica's first; the detour of the UI module's `ExecuteMainCommand`
//! takes Angelica's command, which opens or closes the menu, and passes every
//! other command on.
use super::entries::Entries;
use crate::{catalog::Catalog, commands::Commands};
use angelica_hook::Detour;
use angelica_map::ValueKind::{I32, Ptr, U8, U32, Void};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

const OPEN: &str = "ui.agent_hud.open_system_menu";
const EXECUTE: &str = "ui.ui_module.execute_main_command";
/// The command of Angelica's entry; no `MainCommand` row has it.
const COMMAND: u32 = 0x4147_0001;
/// What the entry shows: the game's boxed letter A, then the name.
static TEXT: &[u8] = "\u{E071} Angelica\0".as_bytes();
/// What the checks of `OpenSystemMenu` in the map confirm: a value is 16
/// bytes, the entries' commands are values from 5 on and their texts values
/// from 25 on, and the menu holds at most 20 entries.
const VALUE: usize = 16;
const COMMANDS: usize = 5;
const TEXTS: usize = 25;
const MAX_ENTRIES: u32 = 20;
/// The game's value kinds for an integer and for text.
const INT: u8 = 3;
const STRING: u8 = 8;

struct Menu {
    commands: Arc<Commands>,
    kind: usize,
    integer: usize,
    text: usize,
}

static MENU: OnceLock<Menu> = OnceLock::new();
static OPEN_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static EXECUTE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

type Open = unsafe extern "system" fn(usize, usize, u32);
type Execute = unsafe extern "system" fn(usize, u32);

/// Installs both detours once.
pub fn install(catalog: &Catalog, base: usize, commands: Arc<Commands>) -> Result<(), String> {
    if MENU.get().is_some() {
        return Ok(());
    }
    let entries = Entries { catalog, base };
    let menu = Menu {
        commands,
        kind: entries.field("ui.atk_value.kind", U8)?,
        integer: entries.field("ui.atk_value.integer", I32)?,
        text: entries.field("ui.atk_value.text", Ptr)?,
    };
    if menu.kind >= VALUE || menu.integer + 4 > VALUE || menu.text + 8 > VALUE {
        return Err("the value's fields lie outside its 16 bytes".into());
    }
    let open = entries.function(OPEN, &[Ptr, Ptr, U32], Void)?;
    let execute = entries.function(EXECUTE, &[Ptr, U32], Void)?;
    // SAFETY: the map verified each address as the start of a function with
    // this ABI in the executable's code; each replacement has the same ABI.
    let mut open = unsafe { Detour::new(open.address, open_menu as Open as usize, &open.code?) }
        .map_err(|e| format!("system menu detour: {e}"))?;
    // SAFETY: as above.
    let mut execute = unsafe {
        Detour::new(
            execute.address,
            execute_command as Execute as usize,
            &execute.code?,
        )
    }
    .map_err(|e| format!("main command detour: {e}"))?;
    OPEN_ORIGINAL.store(open.trampoline(), Ordering::Release);
    EXECUTE_ORIGINAL.store(execute.trampoline(), Ordering::Release);
    if MENU.set(menu).is_err() {
        return Ok(());
    }
    // The command first, so that an entry the menu shows is always answered.
    // SAFETY: the adapter and the trampolines are published; nothing else in
    // this process changes these bytes meanwhile.
    unsafe { execute.enable() }.map_err(|e| format!("main command detour: {e}"))?;
    // SAFETY: as above.
    unsafe { open.enable() }.map_err(|e| format!("system menu detour: {e}"))?;
    // Installed until the process exits.
    std::mem::forget(execute);
    std::mem::forget(open);
    Ok(())
}

unsafe extern "system" fn open_menu(agent: usize, values: usize, count: u32) {
    let entries = MENU
        .get()
        .filter(|_| values != 0 && count < MAX_ENTRIES)
        .and_then(|menu| super::guard(None, || Some(menu.with_entry(values, count))));
    let original = OPEN_ORIGINAL.load(Ordering::Acquire);
    // SAFETY: the trampoline runs the game's function with its own ABI; the
    // copy holds every value the function reads for `count + 1` entries and
    // lives until it returns, and the function copies the texts it keeps.
    unsafe {
        let open = std::mem::transmute::<usize, Open>(original);
        match &entries {
            Some(copy) => open(agent, copy.as_ptr() as usize, count + 1),
            None => open(agent, values, count),
        }
    }
}

unsafe extern "system" fn execute_command(module: usize, command: u32) {
    if command == COMMAND
        && let Some(menu) = MENU.get()
    {
        super::guard((), || menu.commands.toggle_menu());
        return;
    }
    // SAFETY: the trampoline runs the game's function with its own ABI and
    // the game's arguments.
    unsafe {
        std::mem::transmute::<usize, Execute>(EXECUTE_ORIGINAL.load(Ordering::Acquire))(
            module, command,
        );
    }
}

impl Menu {
    /// The menu's values with Angelica's entry before the `count` the game
    /// gave. Only the values `OpenSystemMenu` reads are filled; values are
    /// copied as they are, since the function only reads them.
    fn with_entry(&self, values: usize, count: u32) -> Vec<[u64; 2]> {
        let count = count as usize;
        let mut copy = vec![[0u64; 2]; TEXTS + MAX_ENTRIES as usize];
        for index in 0..count {
            for first in [COMMANDS, TEXTS] {
                // SAFETY: the game passes the values its function reads,
                // and it reads these for each of its `count` entries.
                copy[first + 1 + index] = unsafe {
                    std::ptr::read_unaligned((values + (first + index) * VALUE) as *const [u64; 2])
                };
            }
        }
        let bytes = |value: &mut [u64; 2]| -> *mut u8 { value.as_mut_ptr().cast() };
        // SAFETY: the offsets lie inside the 16 bytes of a value, as
        // `install` checked.
        unsafe {
            let command = bytes(&mut copy[COMMANDS]);
            command.add(self.kind).write(INT);
            command
                .add(self.integer)
                .cast::<u32>()
                .write_unaligned(COMMAND);
            let text = bytes(&mut copy[TEXTS]);
            text.add(self.kind).write(STRING);
            text.add(self.text)
                .cast::<usize>()
                .write_unaligned(TEXT.as_ptr() as usize);
        }
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_comes_first_and_the_game_s_follow() {
        let menu = Menu {
            commands: Arc::default(),
            kind: 0,
            integer: 8,
            text: 8,
        };
        let mut values = vec![[0u64; 2]; 45];
        values[COMMANDS] = [u64::from(INT), 31];
        values[COMMANDS + 1] = [u64::from(INT), 32];
        values[TEXTS] = [u64::from(STRING), 0x1234];
        let copy = menu.with_entry(values.as_ptr() as usize, 2);
        assert_eq!(copy.len(), 45);
        assert_eq!(copy[COMMANDS], [u64::from(INT), u64::from(COMMAND)]);
        assert_eq!(
            copy[TEXTS],
            [u64::from(STRING), TEXT.as_ptr() as usize as u64]
        );
        assert_eq!(copy[COMMANDS + 1], [u64::from(INT), 31]);
        assert_eq!(copy[COMMANDS + 2], [u64::from(INT), 32]);
        assert_eq!(copy[TEXTS + 1], [u64::from(STRING), 0x1234]);
        assert_eq!(copy[TEXTS + 2], [0, 0]);
        assert_eq!(copy[COMMANDS + 3], [0, 0]);
    }

    #[test]
    fn the_text_ends_with_nul() {
        assert_eq!(TEXT.last(), Some(&0));
        assert!(!TEXT[..TEXT.len() - 1].contains(&0));
    }
}
