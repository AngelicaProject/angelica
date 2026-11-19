//! Keep recognized quest phrases together with their shell replacement rules.
use super::{
    entries::Entries,
    game::WindowsGame,
    game_string::{Strings, TextLayout, pointer, read},
    memory,
};
use crate::{
    excel_text::{SheetBinding, SheetInfo},
    game_data::GameData,
    quest_say::{self, Quest},
    sheets::Sheets,
};
use angelica_map::ValueKind::{Ptr, Struct, U8, U16, U32};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock, RwLock},
};

const QUEST_COUNT: usize = 30;
const QUEST_STRIDE: usize = 24;
const OBJECT_COUNT: usize = 819;

struct Adapter {
    data: Arc<GameData>,
    sheets: Arc<Sheets>,
    text: TextLayout,
    strings: Strings,
    main_thread: u32,
    channel: usize,
    excel_module: usize,
    get_language: usize,
    quests: usize,
    quest_id: usize,
    sequence: usize,
    objects: usize,
    base_id: usize,
    registered: RwLock<BTreeMap<(u16, u8), Arc<Quest>>>,
    aliases: RwLock<BTreeMap<u8, Arc<BTreeSet<String>>>>,
}

static ADAPTER: OnceLock<Result<Adapter, String>> = OnceLock::new();

pub(super) fn install(game: &WindowsGame) {
    ADAPTER.get_or_init(|| {
        let result = resolve(game);
        match &result {
            Ok(_) => super::log("quest Say phrases: in place"),
            Err(error) => super::log(&format!("quest Say phrases: unavailable: {error}")),
        }
        result
    });
}

fn resolve(game: &WindowsGame) -> Result<Adapter, String> {
    let map = Entries {
        catalog: &game.catalog,
        base: game.base,
    };
    let sequence = map.field("game.quest_work.sequence", U8)?;
    let quest_id = map.field("game.quest_work.id", U16)?;
    if sequence >= QUEST_STRIDE || quest_id >= QUEST_STRIDE - 1 {
        return Err("the quest fields are outside the accepted quest record".into());
    }
    let first = map
        .field("game.quest_manager.first_accepted_id", U16)?
        .checked_sub(quest_id)
        .ok_or("the accepted quest array is outside the manager")?;
    let quests = map.global("game.quest_manager.instance", Struct)? + first;
    let objects = map.global("object.manager.instance", Struct)?
        + map.field("object.manager.indexed_objects", Struct)?;
    let adapter = Adapter {
        data: game.data.clone(),
        sheets: game.sheets.clone(),
        text: TextLayout::resolve(&map)?,
        strings: Strings::resolve(&map)?,
        main_thread: thread_id(),
        channel: map.field("shell.command_module.current_channel", Struct)?,
        excel_module: map.field("ui.ui_module.excel_module", Ptr)?,
        get_language: map
            .function("excel.module_interface.get_language", &[Ptr], Ptr)?
            .address,
        quests,
        quest_id,
        sequence,
        objects,
        base_id: map.field("object.game_object.base_id", U32)?,
        registered: RwLock::default(),
        aliases: RwLock::default(),
    };
    super::chat::install_commands(&game.catalog, game.base, game.commands.clone())?;
    Ok(adapter)
}

pub(super) fn protect(info: &SheetInfo, binding: &mut SheetBinding) {
    if binding.is_empty() || !info.name.starts_with("quest/") {
        return;
    }
    let Some(Ok(adapter)) = ADAPTER.get() else {
        return;
    };
    let Some(quest) = quest_say::prepare(&adapter.data, &adapter.sheets, info, binding) else {
        return;
    };
    if adapter.command_names(quest.language).is_none() {
        return;
    }
    if let Ok(mut registered) = adapter.registered.write() {
        binding.keep_rows(quest.rows.clone());
        registered.insert((quest.id, quest.language), Arc::new(quest));
    }
}

pub(super) fn with_line<T>(
    module: usize,
    line: usize,
    ui: usize,
    call: impl FnOnce(usize) -> T,
) -> T {
    if let Some(Ok(adapter)) = ADAPTER.get()
        && thread_id() == adapter.main_thread
        && let Some(changed) = super::guard(None, || adapter.replacement(module, line, ui))
    {
        return adapter.strings.with(&changed, call);
    }
    call(line)
}

fn thread_id() -> u32 {
    // SAFETY: this Windows query takes no arguments and returns the current thread id.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

impl Adapter {
    fn language(&self, ui: usize) -> Option<u8> {
        type GetLanguage = unsafe extern "system" fn(usize) -> usize;
        if ui == 0 {
            return None;
        }
        let excel = pointer(ui.checked_add(self.excel_module)?)?;
        // SAFETY: the shell runs on the main thread. The map verifies this
        // getter's ABI and the Excel interface field of its UI module.
        let code = unsafe { std::mem::transmute::<usize, GetLanguage>(self.get_language)(excel) };
        u8::try_from(u32::from_le_bytes(read(code)?))
            .ok()?
            .checked_add(1)
    }

    fn command_names(&self, language: u8) -> Option<Arc<BTreeSet<String>>> {
        if let Some(names) = self.aliases.read().ok()?.get(&language) {
            return Some(names.clone());
        }
        let names = Arc::new(quest_say::commands(&self.data, &self.sheets, language)?);
        self.aliases.write().ok()?.insert(language, names.clone());
        Some(names)
    }

    fn replacement(&self, module: usize, line: usize, ui: usize) -> Option<Vec<u8>> {
        let registry = self.registered.read().ok()?;
        if registry.is_empty() || module == 0 {
            return None;
        }
        let line = self.text.text(line, quest_say::LIMIT)?;
        let (command, body) = quest_say::command(&line);
        let channel;
        let command = if command.is_empty() {
            channel = self.text.text(module.checked_add(self.channel)?, 64)?;
            quest_say::command(&channel).0
        } else {
            command
        };
        if command.is_empty() || body == line.len() {
            return None;
        }
        let command = std::str::from_utf8(command).ok()?.to_lowercase();
        let language = self.language(ui)?;
        if !self.aliases.read().ok()?.get(&language)?.contains(&command) {
            return None;
        }
        let quests = memory::read(self.quests, QUEST_COUNT * QUEST_STRIDE).ok()?;
        let mut changed = None;
        for work in quests.as_chunks::<QUEST_STRIDE>().0 {
            let id = u16::from_le_bytes([work[self.quest_id], work[self.quest_id + 1]]);
            let Some(quest) = registry.get(&(id, language)) else {
                continue;
            };
            for phrase in &quest.phrases {
                if phrase.sequence != work[self.sequence] {
                    continue;
                }
                let Some(text) = phrase.replace(&line[body..]) else {
                    continue;
                };
                if body + text.len() > quest_say::LIMIT || !self.target_loaded(&phrase.targets) {
                    continue;
                }
                let replacement = [&line[..body], &text].concat();
                if changed
                    .as_ref()
                    .is_some_and(|previous| *previous != replacement)
                {
                    return None;
                }
                changed = Some(replacement);
                break;
            }
        }
        changed
    }

    fn target_loaded(&self, ids: &[u32]) -> bool {
        let Ok(objects) = memory::read(self.objects, OBJECT_COUNT * size_of::<usize>()) else {
            return false;
        };
        objects
            .as_chunks::<{ size_of::<usize>() }>()
            .0
            .iter()
            .any(|&bytes| {
                let address = usize::from_le_bytes(bytes);
                address != 0
                    && address
                        .checked_add(self.base_id)
                        .and_then(read)
                        .map(u32::from_le_bytes)
                        .is_some_and(|id| ids.contains(&id))
            })
    }
}
