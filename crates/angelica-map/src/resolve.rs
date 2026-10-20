use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use angelica_pe::{Image, Signature};
use iced_x86::{Decoder, DecoderOptions, OpKind, Register};

use crate::Map;
use crate::model::{Check, Entry, EntryKind, Follow, Locator};

/// What a present entry resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// An RVA, for every kind except fields.
    Address(u32),
    /// A field offset.
    Offset(u32),
}

impl Value {
    /// The RVA or the offset.
    #[must_use]
    pub fn get(self) -> u32 {
        match self {
            Self::Address(value) | Self::Offset(value) => value,
        }
    }
}

/// Why an entry is absent from a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Absence {
    Recorded(String),
    NoMatch,
    Ambiguous(usize),
    OutsideImage,
    NotCode,
    Dependency(String),
    SlotNotInImage {
        vtable: String,
        slot: u32,
    },
    SlotNotCode(u32),
    NoFunctionRange(String),
    NoOperand,
    PatchMismatch,
    NotInVtable {
        slot: u32,
        code_slots: u32,
    },
    CheckFailed {
        index: usize,
        reason: Box<Absence>,
    },
    CheckDiffers {
        index: usize,
        found: u32,
    },
    /// A field whose offset is a constant of the map, which no build record
    /// confirms for this executable.
    Unconfirmed,
}

impl fmt::Display for Absence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recorded(reason) => formatter.write_str(reason),
            Self::NoMatch => write!(formatter, "signature has no match"),
            Self::Ambiguous(count) => write!(formatter, "signature matched {count} addresses"),
            Self::OutsideImage => write!(formatter, "address is outside the image"),
            Self::NotCode => write!(formatter, "address is not in a code section"),
            Self::Dependency(name) => write!(formatter, "depends on absent entry `{name}`"),
            Self::SlotNotInImage { vtable, slot } => {
                write!(
                    formatter,
                    "slot {slot} of `{vtable}` does not hold an address in the image"
                )
            }
            Self::SlotNotCode(slot) => write!(formatter, "slot {slot} does not point into code"),
            Self::NoFunctionRange(name) => {
                write!(formatter, "`{name}` has no code to search")
            }
            Self::NoOperand => write!(
                formatter,
                "matched instruction has no memory operand with a displacement"
            ),
            Self::PatchMismatch => {
                write!(formatter, "bytes at the patch site differ from `original`")
            }
            Self::Unconfirmed => write!(
                formatter,
                "a constant offset that no build record confirms for this executable"
            ),
            Self::NotInVtable { slot, code_slots } => write!(
                formatter,
                "address is not slot {slot} of a table whose first {code_slots} slots point into code"
            ),
            Self::CheckFailed { index, reason } => write!(formatter, "check {index}: {reason}"),
            Self::CheckDiffers { index, found } => {
                write!(
                    formatter,
                    "check {index} found {found:#X}, not the entry's value"
                )
            }
        }
    }
}

/// The result of resolving every entry of a map against one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub(crate) results: BTreeMap<String, Result<Value, Absence>>,
    pub(crate) constant_offsets: Vec<String>,
}

impl Resolution {
    /// Every entry's result, by name.
    #[must_use]
    pub fn results(&self) -> &BTreeMap<String, Result<Value, Absence>> {
        &self.results
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Result<Value, Absence>> {
        self.results.get(name)
    }

    /// Present entries and their values, by name.
    pub fn present(&self) -> impl Iterator<Item = (&str, Value)> {
        self.results
            .iter()
            .filter_map(|(name, result)| result.as_ref().ok().map(|value| (name.as_str(), *value)))
    }

    /// Absent entries and why, by name.
    pub fn absent(&self) -> impl Iterator<Item = (&str, &Absence)> {
        self.results.iter().filter_map(|(name, result)| {
            result
                .as_ref()
                .err()
                .map(|absence| (name.as_str(), absence))
        })
    }

    /// Present fields whose offset is a constant of the map rather than
    /// derived from code, by name: the ones confirmed for this build.
    #[must_use]
    pub fn constant_offsets(&self) -> &[String] {
        &self.constant_offsets
    }
}

impl Map {
    /// Resolves every entry against an image loaded at `load_base`: the
    /// image's preferred base for a file, the module address in the game.
    /// A field whose offset is a constant of the map is absent: only a build
    /// record confirms it.
    #[must_use]
    pub fn resolve(&self, image: &Image<'_>, load_base: u64) -> Resolution {
        self.resolve_confirming(image, load_base, &BTreeSet::new())
    }

    /// Resolves like [`Map::resolve`], with the fields whose constant
    /// offsets were checked in this build, by name, present.
    #[must_use]
    pub fn resolve_confirming(
        &self,
        image: &Image<'_>,
        load_base: u64,
        confirmed: &BTreeSet<String>,
    ) -> Resolution {
        let mut resolver = Resolver {
            image,
            load_base,
            confirmed,
            results: BTreeMap::new(),
        };
        let mut constant_offsets = Vec::new();
        for name in self.order() {
            let Some(entry) = self.get(name) else {
                continue;
            };
            let result = resolver.resolve_entry(entry);
            if result.is_ok()
                && matches!(
                    entry.kind,
                    EntryKind::Field {
                        offset: Some(_),
                        ..
                    }
                )
            {
                constant_offsets.push(name.clone());
            }
            resolver.results.insert(name.clone(), result);
        }
        constant_offsets.sort();
        Resolution {
            results: resolver.results,
            constant_offsets,
        }
    }
}

struct Resolver<'a, 'b> {
    image: &'a Image<'b>,
    load_base: u64,
    confirmed: &'a BTreeSet<String>,
    results: BTreeMap<String, Result<Value, Absence>>,
}

impl Resolver<'_, '_> {
    fn resolve_entry(&self, entry: &Entry) -> Result<Value, Absence> {
        for reference in entry.references() {
            if !matches!(self.results.get(reference), Some(Ok(_))) {
                return Err(Absence::Dependency(reference.to_owned()));
            }
        }

        let value = match (&entry.kind, &entry.locator) {
            (
                EntryKind::Field {
                    offset: Some(offset),
                    ..
                },
                _,
            ) => {
                if !self.confirmed.contains(&entry.name) {
                    return Err(Absence::Unconfirmed);
                }
                Value::Offset(*offset)
            }
            (_, Some(locator)) => self.locate(locator)?,
            (_, None) => unreachable!("entries without a locator are constant fields"),
        };

        match &entry.kind {
            EntryKind::Function { .. } | EntryKind::Location { .. } => self.require_code(value)?,
            EntryKind::Vtable { slots } => {
                for slot in 0..*slots {
                    let target = self
                        .slot_target(value.get(), slot)
                        .ok_or(Absence::SlotNotCode(slot))?;
                    if !self.image.is_code(target) {
                        return Err(Absence::SlotNotCode(slot));
                    }
                }
            }
            EntryKind::Patch { original, .. } => {
                self.require_code(value)?;
                if self.image.bytes_at(value.get(), original.len()) != Some(original.as_slice()) {
                    return Err(Absence::PatchMismatch);
                }
            }
            EntryKind::Global { .. } | EntryKind::Field { .. } => {}
        }

        for (index, check) in entry.checks.iter().enumerate() {
            self.check(index, check, value)?;
        }
        Ok(value)
    }

    fn locate(&self, locator: &Locator) -> Result<Value, Absence> {
        match locator {
            Locator::Signature {
                signature,
                at,
                follow,
            } => {
                let start = single(&self.image.scan_code(signature))?;
                let address = u32::try_from(i64::from(start) + at)
                    .ok()
                    .filter(|&address| address < self.image.size_of_image())
                    .ok_or(Absence::OutsideImage)?;
                match follow {
                    None => Ok(Value::Address(address)),
                    Some(Follow::Rel32) => self
                        .image
                        .follow_rel32(address)
                        .map(Value::Address)
                        .ok_or(Absence::OutsideImage),
                }
            }
            Locator::VtableSlot { vtable, slot } => {
                let table = self.value_of(vtable).get();
                self.slot_target(table, *slot)
                    .map(Value::Address)
                    .ok_or_else(|| Absence::SlotNotInImage {
                        vtable: vtable.clone(),
                        slot: *slot,
                    })
            }
            Locator::Operand {
                function,
                signature,
            } => {
                let start = self.value_of(function).get();
                let chunks = self.whole_function(start);
                if chunks.is_empty() {
                    return Err(Absence::NoFunctionRange(function.clone()));
                }
                let found: Vec<&[u8]> = chunks
                    .iter()
                    .flat_map(|chunk| {
                        signature
                            .find_in(chunk)
                            .into_iter()
                            .map(move |offset| &chunk[offset..])
                    })
                    .collect();
                match found.as_slice() {
                    [] => Err(Absence::NoMatch),
                    [code] => displacement(code).map(Value::Offset),
                    _ => Err(Absence::Ambiguous(found.len())),
                }
            }
        }
    }

    fn check(&self, index: usize, check: &Check, value: Value) -> Result<(), Absence> {
        let found = match check {
            Check::Locator(locator) => {
                self.locate(locator)
                    .map_err(|reason| Absence::CheckFailed {
                        index,
                        reason: Box::new(reason),
                    })?
            }
            Check::Equals(name) => self.value_of(name),
            Check::Contains {
                function,
                signature,
            } => {
                let start = function
                    .as_deref()
                    .map_or(value, |name| self.value_of(name))
                    .get();
                let chunks = self.whole_function(start);
                let failed = |reason| Absence::CheckFailed {
                    index,
                    reason: Box::new(reason),
                };
                if chunks.is_empty() {
                    return Err(failed(Absence::NoFunctionRange(
                        function.clone().unwrap_or_else(|| "the entry".to_owned()),
                    )));
                }
                let found: usize = chunks
                    .iter()
                    .map(|chunk| signature.find_in(chunk).len())
                    .sum();
                return match found {
                    0 => Err(failed(Absence::NoMatch)),
                    1 => Ok(()),
                    _ => Err(failed(Absence::Ambiguous(found))),
                };
            }
            Check::InVtable { slot, code_slots } => {
                return if self.in_vtable(value.get(), *slot, *code_slots) {
                    Ok(())
                } else {
                    Err(Absence::CheckFailed {
                        index,
                        reason: Box::new(Absence::NotInVtable {
                            slot: *slot,
                            code_slots: *code_slots,
                        }),
                    })
                };
            }
        };
        if found == value {
            Ok(())
        } else {
            Err(Absence::CheckDiffers {
                index,
                found: found.get(),
            })
        }
    }

    /// All the code of the function that starts at `start`, as `in` locators
    /// and `contains` checks search it: every range in the exception
    /// directory that belongs to it, chained chunks included; for a leaf
    /// function, the code its control flow reaches.
    fn whole_function(&self, start: u32) -> Vec<&[u8]> {
        crate::code::function_code(self.image, start)
            .map(|code| code.ranges)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|range| {
                self.image
                    .bytes_at(range.start, (range.end - range.start) as usize)
            })
            .collect()
    }

    /// The value of an entry already resolved as present.
    fn value_of(&self, name: &str) -> Value {
        match self.results.get(name) {
            Some(Ok(value)) => *value,
            _ => unreachable!("references are checked before use"),
        }
    }

    fn require_code(&self, value: Value) -> Result<(), Absence> {
        if self.image.is_code(value.get()) {
            Ok(())
        } else {
            Err(Absence::NotCode)
        }
    }

    /// The RVA held in slot `slot` of the table at `table`.
    fn slot_target(&self, table: u32, slot: u32) -> Option<u32> {
        let address = self
            .image
            .read_u64(table.checked_add(slot.checked_mul(8)?)?)?;
        let rva = u32::try_from(address.checked_sub(self.load_base)?).ok()?;
        (rva < self.image.size_of_image()).then_some(rva)
    }

    /// Whether `address` is slot `slot` of a table outside code whose first
    /// `code_slots` slots all point into code.
    fn in_vtable(&self, address: u32, slot: u32, code_slots: u32) -> bool {
        let Some(absolute) = self.load_base.checked_add(u64::from(address)) else {
            return false;
        };
        let signature =
            Signature::from_bytes(&absolute.to_le_bytes()).expect("eight bytes form a signature");
        self.image
            .sections()
            .iter()
            .filter(|section| !section.is_code())
            .any(|section| {
                let data = self.image.section_data(section);
                signature.find_in(data).into_iter().any(|offset| {
                    let Ok(offset) = u32::try_from(offset) else {
                        return false;
                    };
                    let reference = section.virtual_address + offset;
                    reference % 8 == 0
                        && reference.checked_sub(slot * 8).is_some_and(|table| {
                            (0..code_slots).all(|index| {
                                self.slot_target(table, index)
                                    .is_some_and(|target| self.image.is_code(target))
                            })
                        })
                })
            })
    }
}

fn single(found: &[u32]) -> Result<u32, Absence> {
    match found {
        [] => Err(Absence::NoMatch),
        [address] => Ok(*address),
        _ => Err(Absence::Ambiguous(found.len())),
    }
}

/// The displacement of the memory operand of the instruction at the start
/// of `code`, when it has one that is not RIP-relative.
fn displacement(code: &[u8]) -> Result<u32, Absence> {
    let instruction = Decoder::new(64, code, DecoderOptions::NONE).decode();
    if instruction.is_invalid() {
        return Err(Absence::NoOperand);
    }
    let has_memory =
        (0..instruction.op_count()).any(|operand| instruction.op_kind(operand) == OpKind::Memory);
    if !has_memory
        || instruction.memory_base() == Register::RIP
        || instruction.memory_base() == Register::None
    {
        return Err(Absence::NoOperand);
    }
    u32::try_from(instruction.memory_displacement64()).map_err(|_| Absence::NoOperand)
}
