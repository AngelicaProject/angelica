//! Plugins' entries in the menu of an item the player carries
//! (`docs/formats/game-structures.md`, "Item menus"). When the game opens
//! the menu, the adapter adds each entry with the game's own function and
//! points its text at the label the host keeps; when the player chooses
//! one, the game's handler runs as for any entry it does not know, and the
//! host notes the choice for the plugin. Neither hook calls plugin code.
use super::entries::{Entries, Function};
use crate::{catalog::Catalog, menus::Menus};
use angelica_hook::Detour;
use angelica_map::ValueKind::{Bool, I32, Ptr, Struct, U8, U32, U64, Void};
use std::sync::{
    Arc, Mutex, OnceLock, PoisonError,
    atomic::{AtomicUsize, Ordering},
};

const ADD: &str = "ui.agent_inventory_context.add_menu_item";
const OPEN: &str = "ui.agent_inventory_context.open_menu";
const RECEIVE: &str = "ui.agent_inventory_context.receive_event";
/// The events of plugins' entries: the game's own are below 0x80, and an
/// event it does not know does nothing.
const FIRST_EVENT: u8 = 0xF0;
/// The kinds of the agent's event for a chosen entry.
const CHOSEN: [u64; 2] = [0x47, 0x48];
/// A menu shows at most 32 entries: one bit each in the agent's masks.
const MOST_ENTRIES: i32 = 32;
/// An Addon row whose text the game's function looks up for the entry; the
/// adapter then points the entry at the plugin's label instead.
const ADDON_TEXT: u32 = 0x97A;
/// The kind of a value holding an integer.
const INTEGER: u8 = 3;
/// Bytes of one of the agent's values.
const VALUE: usize = 16;

struct Menu {
    menus: Arc<Menus>,
    add: usize,
    entry_start: usize,
    entry_count: usize,
    events: usize,
    values: usize,
    item: usize,
    item_id: usize,
    /// The entries the open menu shows, by their position after
    /// [`FIRST_EVENT`], and the labels of this menu and the one before,
    /// whose texts the game may still read.
    shown: Mutex<Shown>,
}

#[derive(Default)]
struct Shown {
    entries: Vec<(String, String)>,
    labels: [Vec<Vec<u8>>; 2],
}

static MENU: OnceLock<Menu> = OnceLock::new();
static OPEN_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static RECEIVE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

type Add = unsafe extern "system" fn(usize, u8, u32, u8, u8);
type Open = unsafe extern "system" fn(usize, u8, u8);
type Receive = unsafe extern "system" fn(usize, usize, usize, u32, u64) -> usize;

/// Installs the two hooks once.
pub fn install(catalog: &Catalog, base: usize, menus: Arc<Menus>) -> Result<(), String> {
    if MENU.get().is_some() {
        return Ok(());
    }
    let entries = Entries { catalog, base };
    let menu = Menu {
        menus,
        add: entries
            .function(ADD, &[Ptr, U8, U32, Bool, Bool], Void)?
            .address,
        entry_start: entries.field("ui.agent_inventory_context.entry_start", I32)?,
        entry_count: entries.field("ui.agent_inventory_context.entry_count", I32)?,
        events: entries.field("ui.agent_inventory_context.events", U8)?,
        values: entries.field("ui.agent_inventory_context.values", Struct)?,
        item: entries.field("ui.agent_inventory_context.item", Ptr)?,
        item_id: entries.field("inventory.inventory_item.item_id", U32)?,
        shown: Mutex::new(Shown::default()),
    };
    let open = entries.function(OPEN, &[Ptr, Bool, U8], Void)?;
    let receive = entries.function(RECEIVE, &[Ptr, Ptr, Ptr, U32, U64], Ptr)?;
    install_resolved(menu, &open, &receive)
}

fn install_resolved(menu: Menu, open: &Function, receive: &Function) -> Result<(), String> {
    // SAFETY: the map verified both addresses as the starts of functions
    // with these ABIs in the executable's code; the replacements have the
    // same ABIs.
    let mut open_detour =
        unsafe { Detour::new(open.address, opened as Open as usize, &open.code.clone()?) }
            .map_err(|e| format!("item menu detour: {e}"))?;
    // SAFETY: as above.
    let mut receive_detour = unsafe {
        Detour::new(
            receive.address,
            received as Receive as usize,
            &receive.code.clone()?,
        )
    }
    .map_err(|e| format!("item menu event detour: {e}"))?;
    OPEN_ORIGINAL.store(open_detour.trampoline(), Ordering::Release);
    RECEIVE_ORIGINAL.store(receive_detour.trampoline(), Ordering::Release);
    if MENU.set(menu).is_err() {
        return Ok(());
    }
    // SAFETY: the adapter and the trampolines are published; nothing else
    // in this process changes these bytes meanwhile.
    unsafe { receive_detour.enable() }.map_err(|e| format!("item menu event detour: {e}"))?;
    // SAFETY: as above.
    unsafe { open_detour.enable() }.map_err(|e| format!("item menu detour: {e}"))?;
    // Installed until the process exits; without plugins' entries they
    // only pass the calls on.
    std::mem::forget(open_detour);
    std::mem::forget(receive_detour);
    Ok(())
}

unsafe extern "system" fn opened(agent: usize, refresh: u8, flag: u8) {
    if let Some(menu) = MENU.get() {
        super::guard((), || menu.add_entries(agent));
    }
    // SAFETY: the trampoline runs the game's function with its own ABI and
    // the game's arguments.
    unsafe {
        std::mem::transmute::<usize, Open>(OPEN_ORIGINAL.load(Ordering::Acquire))(
            agent, refresh, flag,
        );
    }
}

unsafe extern "system" fn received(
    agent: usize,
    answer: usize,
    values: usize,
    count: u32,
    kind: u64,
) -> usize {
    if let Some(menu) = MENU.get() {
        super::guard((), || menu.note_choice(agent, values, count, kind));
    }
    // SAFETY: as in `opened`. The game's handler does nothing for an event
    // it does not know, so a plugin's entry closes the menu like any other.
    unsafe {
        std::mem::transmute::<usize, Receive>(RECEIVE_ORIGINAL.load(Ordering::Acquire))(
            agent, answer, values, count, kind,
        )
    }
}

/// A little-endian integer the game's memory holds at `address`, read so
/// that an unreadable address fails instead of faulting.
fn read_i32(address: usize) -> Option<i32> {
    let bytes = super::memory::read(address, 4).ok()?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

fn read_u8(address: usize) -> Option<u8> {
    super::memory::read(address, 1).ok()?.first().copied()
}

fn read_ptr(address: usize) -> Option<usize> {
    let bytes = super::memory::read(address, 8).ok()?;
    usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok()
}

impl Menu {
    /// Adds the plugins' entries after the game's own, unless the menu has
    /// them already, as when the game opens the same menu again.
    fn add_entries(&self, agent: usize) {
        if agent == 0 {
            return;
        }
        let (Some(start), Some(count)) = (
            read_i32(agent + self.entry_start),
            read_i32(agent + self.entry_count),
        ) else {
            return;
        };
        if !(0..=MOST_ENTRIES).contains(&count) {
            return;
        }
        let (Ok(start), Ok(count)) = (usize::try_from(start), usize::try_from(count)) else {
            return;
        };
        if start > 64 {
            return;
        }
        let ours = (start..start + count).any(|position| {
            read_u8(agent + self.events + position).is_some_and(|event| event >= FIRST_EVENT)
        });
        if ours {
            return;
        }
        let items = self.menus.items();
        let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        shown.entries.clear();
        if items.is_empty() {
            return;
        }
        let room = usize::try_from(MOST_ENTRIES).unwrap_or(0) - count;
        let labels: Vec<Vec<u8>> = items
            .iter()
            .take(room)
            .map(|item| {
                let mut label = item.label.clone();
                label.push(0);
                label
            })
            .collect();
        let add = self.add;
        for (index, label) in labels.iter().enumerate() {
            let position = start + count + index;
            let event = FIRST_EVENT + u8::try_from(index).unwrap_or(0);
            // SAFETY: `add` is the game's function that adds an entry to
            // the live agent the game passed to the hooked function, with
            // the ABI the map verified, on the main thread the game calls
            // it on; the count stays below the 32 entries its masks hold.
            // It writes the entry's value at `position` as a value of kind
            // 8, whose text pointer the adapter then replaces with the
            // label: the game does not free that kind, and the label lives
            // in `shown` until two menus later.
            unsafe {
                std::mem::transmute::<usize, Add>(add)(agent, event, ADDON_TEXT, 0, 0);
                let value = agent + self.values + position * VALUE;
                std::ptr::write_unaligned((value + 8) as *mut usize, label.as_ptr() as usize);
            }
        }
        shown.entries = items
            .into_iter()
            .take(labels.len())
            .map(|item| (item.plugin, item.entry))
            .collect();
        shown.labels.swap(0, 1);
        shown.labels[0] = labels;
    }

    /// Notes the player's choice of a plugin's entry: an event of the kind
    /// for a chosen entry, whose values are 0 and the entry's position.
    fn note_choice(&self, agent: usize, values: usize, count: u32, kind: u64) {
        if !CHOSEN.contains(&kind) || count < 2 || values == 0 || agent == 0 {
            return;
        }
        let integer = |value: usize| {
            (read_u8(value)? == INTEGER)
                .then(|| read_i32(value + 8))
                .flatten()
        };
        if integer(values) != Some(0) {
            return;
        }
        let (Some(index), Some(start)) =
            (integer(values + VALUE), read_i32(agent + self.entry_start))
        else {
            return;
        };
        let Some(event) = usize::try_from(start + index)
            .ok()
            .and_then(|position| read_u8(agent + self.events + position))
        else {
            return;
        };
        let shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        let Some((plugin, entry)) = event
            .checked_sub(FIRST_EVENT)
            .and_then(|at| shown.entries.get(usize::from(at)))
        else {
            return;
        };
        let item = read_ptr(agent + self.item)
            .filter(|&item| item != 0)
            .and_then(|item| super::memory::read(item + self.item_id, 4).ok())
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes);
        if let Some(value) = item.filter(|&item| item != 0) {
            self.menus.choose(crate::menus::Chosen {
                plugin: plugin.clone(),
                entry: entry.clone(),
                value,
            });
        }
    }
}
