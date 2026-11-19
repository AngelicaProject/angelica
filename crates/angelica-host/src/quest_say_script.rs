//! Recognizes the quest scripts that compare a chat line with a text row.
// Adapted from Harmonia. Based on Harmonia (https://github.com/AngelicaProject/Harmonia),
// Copyright (C) 2026 pokeda.
use std::collections::BTreeMap;

const FUNCTION: &str = "IsAcceptSayEvent";
const COMPARE: &str = "CompareString";
const SEQUENCE: &str = "GetQuestSequence";
const TARGET: &str = "GetBaseId";
const HEADER: &[u8] = b"\x1bLua\x51\0\x01\x04\x04\x04\x08\0";
const METHODS: &[&str] = &[
    COMPARE,
    SEQUENCE,
    TARGET,
    "GetQuestId",
    "GetQuestBitFlag8",
    "CompareFixedPhrase",
];

pub(crate) struct Phrase<'a> {
    pub key: &'a str,
    pub sequence: &'a str,
    pub targets: Vec<&'a str>,
    pub exact: bool,
}

struct Function<'a> {
    params: u8,
    code: &'a [u8],
    constants: Vec<Option<&'a str>>,
    functions: Vec<Function<'a>>,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(head)
    }

    fn count(&mut self, size: usize) -> Option<usize> {
        let count = u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize;
        (count <= 1 << 20 && count * size <= self.0.len()).then_some(count)
    }

    fn string(&mut self) -> Option<&'a [u8]> {
        let length = self.count(1)?;
        if length == 0 {
            return Some(&[]);
        }
        self.take(length)?.strip_suffix(&[0])
    }

    fn function(&mut self, depth: usize) -> Option<Function<'a>> {
        if depth > 32 {
            return None;
        }
        self.string()?;
        let params = self.take(12)?[9];
        let size = self.count(4)? * 4;
        let code = self.take(size)?;
        let mut constants = Vec::new();
        for _ in 0..self.count(1)? {
            constants.push(match self.take(1)?[0] {
                0 => None,
                1 => {
                    self.take(1)?;
                    None
                }
                3 => {
                    self.take(8)?;
                    None
                }
                4 => std::str::from_utf8(self.string()?).ok(),
                _ => return None,
            });
        }
        let mut functions = Vec::new();
        for _ in 0..self.count(1)? {
            functions.push(self.function(depth + 1)?);
        }
        let size = self.count(4)? * 4;
        self.take(size)?;
        for _ in 0..self.count(1)? {
            self.string()?;
            self.take(8)?;
        }
        for _ in 0..self.count(1)? {
            self.string()?;
        }
        Some(Function {
            params,
            code,
            constants,
            functions,
        })
    }
}

impl<'a> Function<'a> {
    fn constant(&self, operand: usize) -> Option<&'a str> {
        self.constants
            .get(operand.checked_sub(256)?)
            .copied()
            .flatten()
    }

    fn code(&self) -> impl Iterator<Item = u32> + '_ {
        self.code
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&bytes| u32::from_le_bytes(bytes))
    }

    fn visit<'f>(&'f self, found: &mut Option<&'f Self>) -> Option<()> {
        let mut code = self.code().peekable();
        while let Some(ins) = code.next() {
            if ins & 63 == 36
                && let Some(&next) = code.peek()
                && next & 63 == 9
                && self.constant((next >> 23) as usize) == Some(FUNCTION)
            {
                if found.is_some() {
                    return None;
                }
                *found = Some(self.functions.get((ins >> 14) as usize)?);
            }
        }
        for child in &self.functions {
            child.visit(found)?;
        }
        Some(())
    }

    fn compares_outside(&self, accepted: &Self) -> bool {
        !std::ptr::eq(self, accepted)
            && (self.constants.contains(&Some(COMPARE))
                || self
                    .functions
                    .iter()
                    .any(|child| child.compares_outside(accepted)))
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Value<'a> {
    #[default]
    Other,
    Parameter(usize),
    Field(&'a str),
    Method(&'a str),
    Result(&'a str),
}

/// `None` for a script that names the hook but does not match the known shape.
pub(crate) fn read(script: &[u8]) -> Option<Vec<Phrase<'_>>> {
    if !script
        .windows(FUNCTION.len())
        .any(|name| name == FUNCTION.as_bytes())
    {
        return Some(Vec::new());
    }
    let main = Reader(script.strip_prefix(HEADER)?).function(0)?;
    let mut found = None;
    main.visit(&mut found)?;
    let function = found?;
    if function.params != 4 || !function.functions.is_empty() || main.compares_outside(function) {
        return None;
    }
    analyze(function)
}

fn analyze<'a>(function: &Function<'a>) -> Option<Vec<Phrase<'a>>> {
    use Value::{Field, Method, Other, Parameter, Result};
    let mut registers = [Other; 264];
    for (index, register) in registers.iter_mut().take(4).enumerate() {
        *register = Parameter(index);
    }
    let mut checks = BTreeMap::<&str, Vec<&str>>::new();
    let mut matches = Vec::new();
    let (mut sequence, mut target) = (None, None);
    for ins in function.code() {
        let a = ((ins >> 6) & 255) as usize;
        let b = (ins >> 23) as usize;
        let c = ((ins >> 14) & 511) as usize;
        match ins & 63 {
            0 => registers[a] = *registers.get(b).filter(|_| b < 256)?,
            1 | 2 => registers[a] = Other,
            6 => {
                registers[a] = match (registers.get(b).filter(|_| b < 256)?, function.constant(c)) {
                    (Parameter(0), Some(field)) => Field(field),
                    _ => Other,
                }
            }
            11 => {
                let method = function.constant(c)?;
                if !METHODS.contains(&method) {
                    return None;
                }
                registers[a + 1] = *registers.get(b).filter(|_| b < 256)?;
                registers[a] = Method(method);
            }
            28 => {
                let Method(method) = registers[a] else {
                    return None;
                };
                let receiver = registers[a + 1];
                if (method == SEQUENCE && receiver != Parameter(1))
                    || (method == TARGET && receiver != Parameter(2))
                {
                    return None;
                }
                if method == COMPARE {
                    target?;
                    let (key, exact) = comparison(b, &registers[a + 1..a + 5])?;
                    let item = (key, sequence?, exact);
                    if !matches.contains(&item) {
                        matches.push(item);
                    }
                }
                registers[a..].fill(Other);
                registers[a] = Result(method);
            }
            23 if b >= 256 || c >= 256 => {
                let register = if b >= 256 { c } else { b };
                if register >= 256 || matches!(registers[register], Result(SEQUENCE | TARGET)) {
                    return None;
                }
            }
            23 => {
                if a != 0 {
                    return None;
                }
                let ((Result(result), Field(field)) | (Field(field), Result(result))) =
                    (registers[b], registers[c])
                else {
                    return None;
                };
                match result {
                    SEQUENCE if field.starts_with("SEQ_") => {
                        sequence = Some(field);
                        target = None;
                    }
                    TARGET => {
                        target = Some(field);
                        let list = checks.entry(sequence?).or_default();
                        if !list.contains(&field) {
                            list.push(field);
                        }
                    }
                    _ => return None,
                }
            }
            22 | 30 => {}
            _ => return None,
        }
    }
    matches
        .into_iter()
        .map(|(key, sequence, exact)| {
            Some(Phrase {
                key,
                sequence,
                exact,
                targets: checks.get(sequence)?.clone(),
            })
        })
        .collect()
}

fn comparison<'a>(count: usize, arguments: &[Value<'a>]) -> Option<(&'a str, bool)> {
    use Value::{Field, Parameter};
    let [Parameter(0), Parameter(3), Field(key), mode] = arguments else {
        return None;
    };
    if !key.starts_with("TEXT_") {
        return None;
    }
    let exact = match (count, mode) {
        (4, _) | (5, Field("COMPARE_STRING_EQUAL")) => true,
        (5, Field("COMPARE_STRING_INCLUDE")) => false,
        _ => return None,
    };
    Some((key, exact))
}
