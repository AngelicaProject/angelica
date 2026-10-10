//! The game's own windows for [`crate::game_ui`]: finds a window by its
//! name and an element by its id with the game's functions, reads and sets
//! an element's visibility, and reads and sets a text element's text
//! (`docs/formats/game-structures.md`).
use super::entries::Entries;
use crate::{
    catalog::Catalog,
    game_ui::{Element, Screen},
};
use angelica_map::ValueKind::{I32, I64, Ptr, Struct, U8, U32, Void};
use std::sync::OnceLock;

const STAGE: &str = "ui.stage";
const UNIT_MANAGER: &str = "ui.stage.unit_manager";
const GET_UNIT: &str = "ui.unit_manager.get_unit_by_name";
const GET_NODE: &str = "ui.unit_base.get_node_by_id";
const GET_TEXT_NODE: &str = "ui.unit_base.get_text_node_by_id";
const SET_VISIBLE: &str = "ui.res_node.set_visible";
const FLAGS: &str = "ui.res_node.flags";
const SET_TEXT: &str = "ui.text_node.set_text";
const TEXT: &str = "ui.text_node.text";
const STRING: &str = "text.utf8_string.string_ptr";
const USED: &str = "text.utf8_string.used_size";
/// The bit of an element's flags that is set while it is visible.
const VISIBLE: u32 = 1 << 20;
/// The most bytes of an element's text the host reads.
const MAX_TEXT: usize = 8192;

type GetUnit = unsafe extern "system" fn(usize, usize, i32) -> usize;
type GetNode = unsafe extern "system" fn(usize, u32) -> usize;
type SetVisible = unsafe extern "system" fn(usize, u8);
type SetText = unsafe extern "system" fn(usize, usize);

struct Adapter {
    stage: usize,
    unit_manager: usize,
    get_unit: usize,
    get_node: usize,
    get_text_node: usize,
    set_visible: usize,
    flags: usize,
    set_text: usize,
    text: usize,
    string: usize,
    used: usize,
}

static ADAPTER: OnceLock<Result<Adapter, String>> = OnceLock::new();

fn adapter(catalog: &Catalog, base: usize) -> Result<Adapter, String> {
    let entries = Entries { catalog, base };
    Ok(Adapter {
        stage: entries.global(STAGE, Ptr)?,
        unit_manager: entries.field(UNIT_MANAGER, Ptr)?,
        get_unit: entries.function(GET_UNIT, &[Ptr, Ptr, I32], Ptr)?.address,
        get_node: entries.function(GET_NODE, &[Ptr, U32], Ptr)?.address,
        get_text_node: entries.function(GET_TEXT_NODE, &[Ptr, U32], Ptr)?.address,
        set_visible: entries.function(SET_VISIBLE, &[Ptr, U8], Void)?.address,
        flags: entries.field(FLAGS, U32)?,
        set_text: entries.function(SET_TEXT, &[Ptr, Ptr], Void)?.address,
        text: entries.field(TEXT, Struct)?,
        string: entries.field(STRING, Ptr)?,
        used: entries.field(USED, I64)?,
    })
}

/// The live windows through the game's window manager.
struct Live<'a> {
    adapter: &'a Adapter,
    manager: usize,
}

impl Screen for Live<'_> {
    fn element(&mut self, window: &str, id: u32, text: bool) -> Option<Element> {
        let name = std::ffi::CString::new(window).ok()?;
        // SAFETY: the manager is the live window manager, the name is
        // NUL-terminated, and the functions have the ABI the map verified;
        // they only read, and return zero or a live window or element.
        let node = unsafe {
            let unit = std::mem::transmute::<usize, GetUnit>(self.adapter.get_unit)(
                self.manager,
                name.as_ptr() as usize,
                1,
            );
            if unit == 0 {
                return None;
            }
            let get = if text {
                self.adapter.get_text_node
            } else {
                self.adapter.get_node
            };
            std::mem::transmute::<usize, GetNode>(get)(unit, id)
        };
        (node != 0).then_some(Element { handle: node })
    }

    fn visible(&mut self, element: Element) -> bool {
        // SAFETY: the handle is a live element the game returned in this
        // frame on this thread; the field lies inside it.
        let flags = unsafe {
            std::ptr::read_unaligned((element.handle + self.adapter.flags) as *const u32)
        };
        flags & VISIBLE != 0
    }

    fn set_visible(&mut self, element: Element, visible: bool) {
        // SAFETY: a live element and the ABI the map verified.
        unsafe {
            std::mem::transmute::<usize, SetVisible>(self.adapter.set_visible)(
                element.handle,
                u8::from(visible),
            );
        }
    }

    fn text(&mut self, element: Element) -> Vec<u8> {
        let string = element.handle + self.adapter.text;
        // SAFETY: the handle is a live text element, whose text is a game
        // string: a pointer to its bytes and their count with the NUL. The
        // bytes are read within that count and the host's limit.
        unsafe {
            let bytes = std::ptr::read_unaligned((string + self.adapter.string) as *const usize);
            let used = std::ptr::read_unaligned((string + self.adapter.used) as *const i64);
            let length = usize::try_from(used.saturating_sub(1)).unwrap_or(0);
            if bytes == 0 || length > MAX_TEXT {
                return Vec::new();
            }
            let text = std::slice::from_raw_parts(bytes as *const u8, length);
            text.iter().copied().take_while(|byte| *byte != 0).collect()
        }
    }

    fn set_text(&mut self, element: Element, text: &[u8]) {
        let Ok(text) = std::ffi::CString::new(text) else {
            return;
        };
        // SAFETY: a live text element and NUL-terminated text that outlives
        // the call, which copies it.
        unsafe {
            std::mem::transmute::<usize, SetText>(self.adapter.set_text)(
                element.handle,
                text.as_ptr() as usize,
            );
        }
    }
}

/// Runs `work` with the game's windows; without them yet, does nothing.
/// Called on the game's main thread, which owns the windows.
pub fn with_screen(
    catalog: &Catalog,
    base: usize,
    work: &mut dyn FnMut(&mut dyn Screen),
) -> Result<(), String> {
    let adapter = ADAPTER
        .get_or_init(|| adapter(catalog, base))
        .as_ref()
        .map_err(Clone::clone)?;
    // SAFETY: the global holds zero or the live stage, whose window manager
    // field holds zero or the live manager.
    let manager = unsafe {
        let stage = std::ptr::read_unaligned(adapter.stage as *const usize);
        if stage == 0 {
            return Ok(());
        }
        std::ptr::read_unaligned((stage + adapter.unit_manager) as *const usize)
    };
    if manager != 0 {
        super::guard((), || work(&mut Live { adapter, manager }));
    }
    Ok(())
}
