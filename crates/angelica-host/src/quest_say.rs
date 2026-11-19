//! Resolve recognized quest comparisons against the original and translated sheet cells.
// Adapted from Harmonia. Based on Harmonia (https://github.com/AngelicaProject/Harmonia),
// Copyright (C) 2026 pokeda.
use crate::{
    excel_text::{SheetBinding, SheetInfo},
    game_data::{Cell, GameData, Row, Value},
    quest_say_script,
    sheets::{Field, Sheets},
};
use angelica_map::ColumnType;
use angelica_text::{sestring, table::Variant};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const LIMIT: usize = 500;
const QUEST_SHEET: &str = "Quest";
const STEPS: &str = "QuestSystemDefine";
const COMMANDS: &str = "TextCommand";

pub(crate) struct Phrase {
    pub sequence: u8,
    pub targets: Vec<u32>,
    original: Vec<u8>,
    translated: Vec<char>,
    exact: bool,
}

pub(crate) struct Quest {
    pub id: u16,
    pub language: u8,
    pub rows: BTreeSet<u32>,
    pub phrases: Vec<Phrase>,
}

fn column(fields: &[Field], name: &str, kind: ColumnType) -> Option<u16> {
    fields
        .iter()
        .find(|field| field.name == name && field.column_type == kind)
        .map(|field| field.index)
}

fn cell(row: &Row, column: u16) -> Option<&Cell> {
    row.cells.get(usize::from(column))
}

fn text(row: &Row, index: u16) -> Option<&[u8]> {
    match cell(row, index)? {
        Cell::Text(bytes) => Some(bytes),
        Cell::Value(_) => None,
    }
}

fn plain(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > LIMIT || bytes.is_empty() || bytes.iter().any(|&byte| byte < 0x20) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

pub(crate) fn commands(data: &GameData, sheets: &Sheets, language: u8) -> Option<BTreeSet<String>> {
    let fields = sheets.fields(COMMANDS, data).ok()?;
    let main = column(&fields, "command", ColumnType::Text)?;
    let mut ids = Vec::new();
    data.visit_text(COMMANDS, 2, main, |id, text| {
        if text.eq_ignore_ascii_case(b"/say") {
            ids.push(id);
        }
    })
    .ok()?;
    let &[id] = ids.as_slice() else {
        return None;
    };
    let mut names = BTreeSet::new();
    for name in ["command", "short_command", "alias", "short_alias"] {
        let index = column(&fields, name, ColumnType::Text)?;
        for language in [2, language] {
            let bytes = data.text_cell(COMMANDS, language, id, index).ok()??;
            if let Some(name) = plain(&bytes)
                .filter(|name| name.starts_with('/') && !name.contains(char::is_whitespace))
            {
                names.insert(name.to_lowercase());
            }
        }
    }
    (!names.is_empty()).then_some(names)
}

fn quest_id(name: &str) -> Option<(&str, u16)> {
    let path = name.strip_prefix("quest/")?;
    let (directory, name) = path.split_once('/')?;
    if directory.len() != 3
        || !directory.bytes().all(|b| b.is_ascii_digit())
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let (_, suffix) = name.split_at_checked(name.len().checked_sub(5)?)?;
    if suffix.len() != 5 || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((name, suffix.parse().ok()?))
}

pub(crate) fn prepare(
    data: &GameData,
    sheets: &Sheets,
    info: &SheetInfo,
    binding: &SheetBinding,
) -> Option<Quest> {
    let (name, id) = quest_id(&info.name)?;
    if info.variant != Variant::Rows
        || info.columns.len() != 2
        || info.columns[0].index != 0
        || info.columns[1].index != 1
        || id == 0
    {
        return None;
    }
    let language = info.language;
    if language == 0 {
        return None;
    }
    let script = data.read(&format!("game_script/{}.luab", info.name)).ok()?;
    let comparisons = quest_say_script::read(&script)?;
    if comparisons.is_empty() {
        return None;
    }

    let fields = sheets.fields(QUEST_SHEET, data).ok()?;
    let row = data
        .row(QUEST_SHEET, language, (0x10000 + u32::from(id), 0))
        .ok()??;
    if text(&row, column(&fields, "identifier", ColumnType::Text)?)? != name.as_bytes() {
        return None;
    }
    let names = column(&fields, "first_script_instruction", ColumnType::Text)?
        ..=column(&fields, "last_script_instruction", ColumnType::Text)?;
    let values = column(&fields, "first_script_arg", ColumnType::Uint32)?
        ..=column(&fields, "last_script_arg", ColumnType::Uint32)?;
    if names.clone().count() != values.clone().count() {
        return None;
    }
    let mut parameters = BTreeMap::new();
    for (key, value) in names.zip(values) {
        let Cell::Value(Value::Unsigned(value)) = cell(&row, value)? else {
            return None;
        };
        if let Some(name) = plain(text(&row, key)?) {
            parameters.insert(name, u32::try_from(*value).ok()?);
        }
    }

    let fields = sheets.fields(STEPS, data).ok()?;
    let key = column(&fields, "key", ColumnType::Text)?;
    let value = column(&fields, "value", ColumnType::Int32)?;
    let steps = data.rows(STEPS, 0, None, 1024).ok()?;
    let mut keys = BTreeMap::new();
    data.visit_text(&info.name, language, 0, |row, key| {
        if let Some(key) = plain(key) {
            keys.entry(key.to_owned())
                .and_modify(|id| *id = None)
                .or_insert(Some(row));
        }
    })
    .ok()?;

    let mut quest = Quest {
        id,
        language,
        rows: BTreeSet::new(),
        phrases: Vec::new(),
    };
    for comparison in comparisons {
        let step = steps
            .iter()
            .find(|row| text(row, key) == Some(comparison.sequence.as_bytes()))?;
        let Cell::Value(Value::Signed(sequence)) = cell(step, value)? else {
            return None;
        };
        let sequence = u8::try_from(*sequence).ok()?;
        let targets = comparison
            .targets
            .iter()
            .map(|name| parameters.get(name).copied().filter(|id| *id != 0))
            .collect::<Option<Vec<_>>>()?;
        let id = (*keys.get(comparison.key)?)?;
        let original = data.text_cell(&info.name, language, id, 1).ok()??;
        let Some(shown) = binding.shown(id, 1, &original) else {
            if binding.lists(id, 0) {
                return None;
            }
            continue;
        };
        if original == shown {
            continue;
        }
        plain(&original)?;
        let translated = plain(shown)?.chars().map(normalize).collect();
        quest.rows.insert(id);
        quest.phrases.push(Phrase {
            sequence,
            targets,
            original,
            translated,
            exact: comparison.exact,
        });
    }
    quest
        .phrases
        .sort_by_key(|phrase| std::cmp::Reverse(phrase.translated.len()));
    (!quest.phrases.is_empty()).then_some(quest)
}

pub(crate) fn command(line: &[u8]) -> (&[u8], usize) {
    if !line.starts_with(b"/") {
        return (&[], 0);
    }
    line.iter()
        .position(|&byte| byte == b' ')
        .map_or((line, line.len()), |end| (&line[..end], end + 1))
}

fn normalize(character: char) -> char {
    let normalized = match character {
        '\u{a0}' | '\u{3000}' | '\u{202f}' | '\u{2002}'..='\u{200a}' => ' ',
        '＝' => '=',
        '！' => '!',
        '？' => '?',
        '\u{2010}'..='\u{2015}' | '−' | '－' | 'ー' | 'ｰ' => '-',
        '’' | '‘' | '′' | '`' | '＇' => '\'',
        '“' | '”' | '＂' | '«' | '»' => '"',
        'ё' | 'Ё' => 'е',
        other => other,
    };
    normalized.to_lowercase().next().unwrap_or(normalized)
}

impl Phrase {
    pub(crate) fn replace(&self, body: &[u8]) -> Option<Vec<u8>> {
        if !sestring::is_well_formed(body) || body.contains(&0) {
            return None;
        }
        let shorter_length = self
            .translated
            .iter()
            .rposition(|c| !matches!(c, '.' | '!' | '?' | '\u{2026}'))
            .map_or(0, |at| at + 1);
        let shorter = &self.translated[..shorter_length];
        if self.exact {
            let normalized = std::str::from_utf8(body)
                .ok()?
                .trim()
                .chars()
                .map(normalize)
                .collect::<Vec<_>>();
            return (!body.contains(&2)
                && (normalized == self.translated
                    || (!shorter.is_empty() && normalized == shorter)))
                .then(|| self.original.clone());
        }
        let mut at = 0;
        while at < body.len() {
            if body[at] == 2 {
                at += sestring::macro_len(&body[at..])?;
                continue;
            }
            let end = body[at..]
                .iter()
                .position(|&byte| byte == 2)
                .map_or(body.len(), |end| at + end);
            let segment = std::str::from_utf8(&body[at..end]).ok()?;
            let offsets: Vec<_> = segment
                .char_indices()
                .map(|(position, c)| (position, normalize(c)))
                .collect();
            let find = |needle: &[char]| {
                (!needle.is_empty())
                    .then(|| {
                        offsets.windows(needle.len()).position(|window| {
                            window.iter().map(|&(_, c)| c).eq(needle.iter().copied())
                        })
                    })
                    .flatten()
                    .map(|index| {
                        (
                            offsets[index].0,
                            offsets
                                .get(index + needle.len())
                                .map_or(segment.len(), |&(end, _)| end),
                        )
                    })
            };
            if let Some((start, stop)) = find(&self.translated).or_else(|| find(shorter)) {
                return Some([&body[..at + start], &self.original, &body[at + stop..]].concat());
            }
            at = end;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_replaces_only_the_matching_text_and_keeps_macros() {
        let mut phrase = Phrase {
            sequence: 1,
            targets: vec![42],
            original: b"Rise!".to_vec(),
            translated: "Встаёшь!".chars().map(normalize).collect(),
            exact: false,
        };
        let link = b"\x02\x10\x01\x03";
        let text = ["Друг, ".as_bytes(), link, "ВСТАЕШЬ, верно?".as_bytes()].concat();
        let want = ["Друг, ".as_bytes(), link, "Rise!, верно?".as_bytes()].concat();
        assert_eq!(phrase.replace(&text), Some(want));
        phrase.exact = true;
        assert_eq!(phrase.replace(&text), None);
        assert_eq!(
            phrase.replace("  Встаешь  ".as_bytes()),
            Some(b"Rise!".to_vec())
        );
        assert_eq!(phrase.replace("Встаешь?".as_bytes()), None);
    }

    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH and ANGELICA_MAP_PATH"]
    fn prepares_validated_phrases_from_installed_quest_data() {
        use crate::excel_text::ExcelText;
        use angelica_text::table::{Column, Writer};
        use std::path::PathBuf;
        let game = PathBuf::from(std::env::var_os("ANGELICA_GAME_PATH").unwrap());
        let map = PathBuf::from(std::env::var_os("ANGELICA_MAP_PATH").unwrap());
        let map = angelica_map::Map::load(&map).unwrap();
        let sheets = Sheets::new(map.sheets().cloned());
        let data = GameData::new(game.parent());
        assert!(commands(&data, &sheets, 2).unwrap().contains("/say"));
        let name_column = column(
            &sheets.fields(QUEST_SHEET, &data).unwrap(),
            "identifier",
            ColumnType::Text,
        )
        .unwrap();
        let mut quests = Vec::new();
        data.visit_text(QUEST_SHEET, 2, name_column, |_, name| {
            if let Ok(name) = std::str::from_utf8(name)
                && let Some(suffix) = name.get(name.len().saturating_sub(5)..)
                && suffix.len() == 5
                && suffix.bytes().all(|b| b.is_ascii_digit())
            {
                quests.push(format!("quest/{}/{name}", &suffix[..3]));
            }
        })
        .unwrap();
        let mut recognized = 0;
        let mut prepared = 0;
        for name in quests {
            let Ok(script) = data.read(&format!("game_script/{name}.luab")) else {
                continue;
            };
            let Some(comparisons) = quest_say_script::read(&script) else {
                continue;
            };
            let Some(phrase) = comparisons.first() else {
                continue;
            };
            recognized += 1;
            let Ok(info) = data.sheet(&name) else {
                continue;
            };
            let columns = info
                .columns
                .iter()
                .enumerate()
                .filter_map(|(index, &(kind, offset))| {
                    (kind == angelica_sqpack::excel::ColumnKind::String).then_some(Column {
                        index: u16::try_from(index).ok()?,
                        offset,
                    })
                })
                .collect::<Vec<_>>();
            let mut row = None;
            data.visit_text(&name, 2, 0, |id, key| {
                if key == phrase.key.as_bytes() {
                    row = Some(id);
                }
            })
            .unwrap();
            let Some(row) = row else {
                continue;
            };
            let original = data.text_cell(&name, 2, row, 1).unwrap().unwrap();
            let mut table = Writer::new(2, b"quest-say-test").unwrap();
            table.sheet(&name, Variant::Rows, &columns).unwrap();
            let guard = angelica_text::guard(b"quest-say-test", &original);
            table
                .cell(&name, row, 0, 1, guard, "Встаёшь!".as_bytes())
                .unwrap();
            let excel = ExcelText::default();
            excel.add_table("test", 1, table.finish().unwrap()).unwrap();
            let info = SheetInfo {
                name,
                variant: Variant::Rows,
                language: 2,
                columns,
            };
            let mut binding = excel.bind(&info);
            let Some(quest) = prepare(&data, &sheets, &info, &binding) else {
                continue;
            };
            binding.keep_rows(quest.rows);
            assert!(!binding.lists(row, 0));
            assert_eq!(
                quest.phrases[0].replace("Встаешь".as_bytes()),
                Some(original)
            );
            prepared += 1;
        }
        eprintln!("{recognized} quests recognized, {prepared} prepared with a translated phrase");
        assert!(prepared > 0);
    }
}
