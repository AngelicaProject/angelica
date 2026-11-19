//! Names inside the chat encoder's wire links. The rest of a line is copied verbatim.
// Adapted from Harmonia's OutgoingChatText for Angelica's SeString parser.
// Based on Harmonia (https://github.com/AngelicaProject/Harmonia),
// Copyright (C) 2026 pokeda.
use super::{
    ETX, STRING_EXPRESSION, STX, canonical_integer, expression, is_well_formed, push_integer,
};

const FIXED: u8 = 0x2E;
const CHAT_LINK: u32 = 200;
const ITEM: u32 = 4;
const STATUS: u32 = 10;
const QUEST: u32 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Name {
    Item(u32),
    EventItem(u32),
    Quest(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    MissingName(Name),
    TooLong,
}

/// Restores original item and quest labels and removes custom status text.
/// Returns new bytes without a terminator, or `None` if nothing changed.
///
/// # Errors
/// A recognized name is unavailable or the complete replacement exceeds `limit`.
pub fn restore(
    line: &[u8],
    limit: usize,
    mut original: impl FnMut(Name) -> Option<String>,
) -> Result<Option<Vec<u8>>, Error> {
    if line.len() > limit
        || !line.windows(2).any(|bytes| bytes == [STX, FIXED])
        || line.contains(&0)
        || !is_well_formed(line)
    {
        return Ok(None);
    }
    let mut replacement = Vec::new();
    let mut copied = 0;
    let mut cursor = 0;
    while let Some(start) = line[cursor..].iter().position(|&byte| byte == STX) {
        let start = cursor + start;
        let Some((length, size)) = line.get(start + 2..).and_then(canonical_integer) else {
            return Ok(None);
        };
        let body_start = start + 2 + size;
        cursor = body_start + length as usize + 1;
        if line[start + 1] == FIXED
            && let Some(body) = restore_body(&line[body_start..cursor - 1], &mut original)?
        {
            replacement.extend_from_slice(&line[copied..start]);
            replacement.extend_from_slice(&[STX, FIXED]);
            push_integer(
                &mut replacement,
                u32::try_from(body.len()).map_err(|_| Error::TooLong)?,
            );
            replacement.extend_from_slice(&body);
            replacement.push(ETX);
            copied = cursor;
            if replacement.len() > limit {
                return Err(Error::TooLong);
            }
        }
    }
    if copied == 0 {
        return Ok(None);
    }
    if replacement.len() + line.len() - copied > limit {
        return Err(Error::TooLong);
    }
    replacement.extend_from_slice(&line[copied..]);
    Ok(Some(replacement))
}

fn take_integer(bytes: &mut &[u8]) -> Option<u32> {
    let (value, size) = canonical_integer(bytes)?;
    *bytes = &bytes[size..];
    Some(value)
}

fn take_string<'a>(bytes: &mut &'a [u8]) -> Option<&'a [u8]> {
    let encoded = *bytes;
    if encoded.first() != Some(&STRING_EXPRESSION) {
        return None;
    }
    let (_, size) = canonical_integer(&encoded[1..])?;
    let end = expression(encoded, 0)?;
    *bytes = &encoded[end..];
    Some(&encoded[1 + size..end])
}

fn restore_body(
    body: &[u8],
    original: &mut impl FnMut(Name) -> Option<String>,
) -> Result<Option<Vec<u8>>, Error> {
    let mut args = body;
    let (Some(CHAT_LINK), Some(kind), Some(id)) = (
        take_integer(&mut args),
        take_integer(&mut args),
        take_integer(&mut args),
    ) else {
        return Ok(None);
    };
    let header_size = body.len() - args.len();
    if kind == STATUS {
        if take_integer(&mut args) == Some(1)
            && take_string(&mut args).is_some()
            && take_string(&mut args).is_some()
            && args.is_empty()
        {
            let mut replacement = body[..header_size].to_vec();
            replacement.extend_from_slice(&[1, 1]); // Non-custom status, resolved by ID.
            return Ok(Some(replacement));
        }
        return Ok(None);
    }
    if !matches!(kind, ITEM | QUEST)
        || take_integer(&mut args).is_none()
        || take_integer(&mut args) != Some(0)
        || take_integer(&mut args) != Some(0)
    {
        return Ok(None);
    }
    let prefix = &body[..body.len() - args.len()];
    let Some(current) = take_string(&mut args).filter(|_| args.is_empty()) else {
        return Ok(None);
    };
    let name = match kind {
        QUEST => Name::Quest(id),
        _ if id >= 2_000_000 => Name::EventItem(id),
        _ => Name::Item(id % 500_000),
    };
    let mut text = original(name)
        .filter(|text| {
            !text.trim().is_empty()
                && !text
                    .as_bytes()
                    .iter()
                    .any(|byte| matches!(*byte, 0 | STX | ETX))
        })
        .ok_or(Error::MissingName(name))?;
    if kind == ITEM {
        if (1_000_001..2_000_000).contains(&id) {
            text.push_str(" \u{E03C}");
        } else if (500_001..1_000_000).contains(&id) {
            text.push_str(" \u{E03D}");
        }
    }
    if current == text.as_bytes() {
        return Ok(None);
    }
    let mut replacement = prefix.to_vec();
    replacement.push(STRING_EXPRESSION);
    push_integer(
        &mut replacement,
        u32::try_from(text.len()).map_err(|_| Error::TooLong)?,
    );
    replacement.extend_from_slice(text.as_bytes());
    Ok(Some(replacement))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(numbers: &[u32], strings: &[&str]) -> Vec<u8> {
        let mut body = Vec::new();
        for &number in numbers {
            push_integer(&mut body, number);
        }
        for text in strings {
            body.push(STRING_EXPRESSION);
            push_integer(&mut body, u32::try_from(text.len()).unwrap());
            body.extend_from_slice(text.as_bytes());
        }
        let mut out = vec![STX, FIXED];
        push_integer(&mut out, u32::try_from(body.len()).unwrap());
        out.extend(body);
        out.push(ETX);
        out
    }

    #[test]
    fn restores_only_known_wire_links_and_keeps_the_numeric_fields() {
        let item = [200, 4, 1_000_123, 27, 0, 0];
        let event_item = [200, 4, 2_000_100, 0, 0, 0];
        let quest = [200, 12, 65_537, 0, 0, 0];
        let unknown = fixed(&[200, 40, 1], &["текст неизвестной ссылки"]);
        let before = [
            "Я нашёл ".as_bytes(),
            &fixed(&item, &["Зелье"]),
            &fixed(&event_item, &["Ключ"]),
            &unknown,
            &fixed(&quest, &["Задание"]),
            &fixed(&[200, 10, 99, 1], &["Эффект", "Описание"]),
            " и всё.".as_bytes(),
        ]
        .concat();
        let after = [
            "Я нашёл ".as_bytes(),
            &fixed(&item, &["Potion \u{E03C}"]),
            &fixed(&event_item, &["Key"]),
            &unknown,
            &fixed(&quest, &["A Good Adventurer Is Hard to Find"]),
            &fixed(&[200, 10, 99, 0, 0], &[]),
            " и всё.".as_bytes(),
        ]
        .concat();
        let name = |name| match name {
            Name::Item(123) => Some("Potion".into()),
            Name::EventItem(2_000_100) => Some("Key".into()),
            Name::Quest(65_537) => Some("A Good Adventurer Is Hard to Find".into()),
            _ => panic!("unexpected name {name:?}"),
        };
        assert_eq!(restore(&before, 1023, name), Ok(Some(after.clone())));
        assert_eq!(restore(&after, 1023, name), Ok(None));
    }

    #[test]
    fn refuses_missing_names_and_overlong_replacements_without_truncation() {
        let mut before = fixed(&[200, 4, 500_123, 0, 0, 0], &["x"]);
        before.extend_from_slice(b"following text");
        assert_eq!(
            restore(&before, 1023, |_| None),
            Err(Error::MissingName(Name::Item(123)))
        );
        assert_eq!(
            restore(&before, 1023, |_| Some("a".repeat(1023))),
            Err(Error::TooLong)
        );
        assert_eq!(
            restore(&before, 1023, |_| Some("X".into()))
                .unwrap()
                .unwrap(),
            [
                fixed(&[200, 4, 500_123, 0, 0, 0], &["X \u{E03D}"]),
                b"following text".to_vec()
            ]
            .concat()
        );
    }

    #[test]
    fn ignores_plain_text_malformed_macros_and_unrecognized_link_shapes() {
        let unknown = fixed(&[200, 4, 100, 1, 2, 0], &["Зелье"]);
        for line in ["Обычный текст".as_bytes(), &[STX, FIXED, 0xFF], &unknown] {
            assert_eq!(
                restore(line, 1023, |_| panic!("unexpected lookup")),
                Ok(None)
            );
        }
    }
}
