use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use angelica_pe::{Image, Signature};
use iced_x86::{Decoder, DecoderOptions, Instruction, OpKind, Register};

use crate::Anatomia;
use crate::model::{Check, Entry, Kind, OperandKind, Slot, Step};

/// What a present entry resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// An RVA, for every kind except fields.
    Address(u32),
    /// A field's offset.
    Offset(u32),
}

impl Value {
    #[must_use]
    pub fn get(self) -> u32 {
        match self {
            Self::Address(value) | Self::Offset(value) => value,
        }
    }
}

/// Why an entry is absent from a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Absence(pub String);

impl fmt::Display for Absence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Every entry's result against one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub(crate) results: BTreeMap<String, Result<Value, Absence>>,
    pub(crate) confirmed: Vec<String>,
}

impl Resolution {
    #[must_use]
    pub fn results(&self) -> &BTreeMap<String, Result<Value, Absence>> {
        &self.results
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Result<Value, Absence>> {
        self.results.get(name)
    }

    pub fn present(&self) -> impl Iterator<Item = (&str, Value)> {
        self.results
            .iter()
            .filter_map(|(name, result)| result.as_ref().ok().map(|value| (name.as_str(), *value)))
    }

    pub fn absent(&self) -> impl Iterator<Item = (&str, &Absence)> {
        self.results.iter().filter_map(|(name, result)| {
            result
                .as_ref()
                .err()
                .map(|absence| (name.as_str(), absence))
        })
    }

    /// Present fields whose offset is a constant of the description, which
    /// a record confirmed for this executable.
    #[must_use]
    pub fn confirmed(&self) -> &[String] {
        &self.confirmed
    }
}

impl Anatomia {
    /// Resolves every entry against an image loaded at `load_base`: the
    /// image's preferred base for a file, the module's address in the game.
    /// A field with a constant offset is present only when `confirmed`
    /// names it.
    #[must_use]
    pub fn resolve(
        &self,
        image: &Image<'_>,
        load_base: u64,
        confirmed: &BTreeSet<String>,
    ) -> Resolution {
        let mut resolver = Resolver {
            anatomia: self,
            image,
            load_base,
            results: BTreeMap::new(),
        };
        let mut constants = Vec::new();
        for name in &self.order {
            let Some(entry) = self.entries.get(name) else {
                continue;
            };
            let result = match self.unusable(entry) {
                Some(reason) => Err(Absence(reason)),
                None => match &entry.kind {
                    Kind::Field { at: Some(at), .. } => {
                        if confirmed.contains(name) {
                            constants.push(name.clone());
                            resolver.checked(entry, Value::Offset(*at))
                        } else {
                            Err(Absence(
                                "a constant offset that no build record confirms for this executable"
                                    .to_owned(),
                            ))
                        }
                    }
                    _ => resolver.resolve_entry(entry),
                },
            };
            resolver.results.insert(name.clone(), result);
        }
        for name in &self.unread {
            let reason = self.broken.get(name).cloned().unwrap_or_default();
            resolver.results.insert(name.clone(), Err(Absence(reason)));
        }
        constants.sort();
        constants.retain(|name| matches!(resolver.results.get(name), Some(Ok(_))));
        Resolution {
            results: resolver.results,
            confirmed: constants,
        }
    }

    /// Why an entry is absent in every build, when it is.
    fn unusable(&self, entry: &Entry) -> Option<String> {
        if let Some(reason) = self.broken.get(&entry.name) {
            return Some(reason.clone());
        }
        if let Kind::Field { of, .. } = &entry.kind
            && let Some(reason) = self.broken.get(of)
        {
            return Some(format!("structure `{of}` is absent: {reason}"));
        }
        None
    }
}

struct Resolver<'a, 'b> {
    anatomia: &'a Anatomia,
    image: &'a Image<'b>,
    load_base: u64,
    results: BTreeMap<String, Result<Value, Absence>>,
}

/// Where a pipeline stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    Empty,
    Address(u32),
    Number(i64),
}

impl Resolver<'_, '_> {
    fn resolve_entry(&self, entry: &Entry) -> Result<Value, Absence> {
        for reference in entry.references() {
            if !matches!(self.results.get(reference), Some(Ok(_))) {
                return Err(Absence(format!("depends on absent entry `{reference}`")));
            }
        }
        let value = match self.run(&entry.find)? {
            Cursor::Address(address) if !matches!(entry.kind, Kind::Field { .. }) => {
                Value::Address(address)
            }
            Cursor::Number(number) if matches!(entry.kind, Kind::Field { .. }) => {
                u32::try_from(number)
                    .map(Value::Offset)
                    .map_err(|_| Absence(format!("offset {number} is not 0 to {}", u32::MAX)))?
            }
            Cursor::Empty => return Err(Absence("`find` ends without a value".to_owned())),
            Cursor::Address(_) => {
                return Err(Absence(
                    "`find` ends at an address; a field needs an offset".to_owned(),
                ));
            }
            Cursor::Number(_) => {
                return Err(Absence(
                    "`find` ends at a number; this entry needs an address".to_owned(),
                ));
            }
        };
        self.checked(entry, value)
    }

    /// Checks a found value for its kind, then the entry's checks.
    fn checked(&self, entry: &Entry, value: Value) -> Result<Value, Absence> {
        for reference in entry.references() {
            if !matches!(self.results.get(reference), Some(Ok(_))) {
                return Err(Absence(format!("depends on absent entry `{reference}`")));
            }
        }
        match &entry.kind {
            Kind::Function { .. } | Kind::Location { .. } => {
                if !self.image.is_code(value.get()) {
                    return Err(Absence("address is not in a code section".to_owned()));
                }
            }
            Kind::Vtable { code_slots, slots } => {
                for slot in 0..*code_slots {
                    let in_code = self
                        .slot_target(value.get(), slot)
                        .is_some_and(|target| self.image.is_code(target));
                    if !in_code {
                        return Err(Absence(format!("slot {slot} does not point into code")));
                    }
                }
                for (name, (slot, _)) in slots {
                    if self.slot_target(value.get(), *slot).is_none() {
                        return Err(Absence(format!(
                            "slot `{name}` ({slot}) does not hold an address in the image"
                        )));
                    }
                }
            }
            Kind::Global { .. } | Kind::Field { .. } => {}
        }
        for (index, check) in entry.checks.iter().enumerate() {
            self.check(check, value)
                .map_err(|reason| Absence(format!("check {index}: {reason}")))?;
        }
        Ok(value)
    }

    fn run(&self, steps: &[Step]) -> Result<Cursor, Absence> {
        let mut cursor = Cursor::Empty;
        let mut region: Option<Vec<std::ops::Range<u32>>> = None;
        for (index, step) in steps.iter().enumerate() {
            let restricted = region.take();
            cursor = self
                .step(step, cursor, restricted, &mut region)
                .map_err(|reason| Absence(format!("step {index} (`{}`): {reason}", step.kind())))?;
        }
        if region.is_some() {
            return Err(Absence("`in` is followed by no scan".to_owned()));
        }
        Ok(cursor)
    }

    #[allow(clippy::too_many_lines)]
    fn step(
        &self,
        step: &Step,
        cursor: Cursor,
        restricted: Option<Vec<std::ops::Range<u32>>>,
        region: &mut Option<Vec<std::ops::Range<u32>>>,
    ) -> Result<Cursor, String> {
        if restricted.is_some() && !matches!(step, Step::Scan { .. }) {
            return Err("`in` restricts only the scan right after it".to_owned());
        }
        let address = || match cursor {
            Cursor::Address(address) => Ok(address),
            _ => Err("needs an address".to_owned()),
        };
        Ok(match step {
            Step::Scan { signature, data } => {
                let found = match restricted {
                    Some(ranges) => {
                        if *data {
                            return Err("a scan in a function searches code".to_owned());
                        }
                        self.scan_ranges(signature, &ranges)
                    }
                    None if *data => self.scan_data(signature),
                    None => self.image.scan_code(signature),
                };
                match found.as_slice() {
                    [] => return Err("signature has no match".to_owned()),
                    [address] => Cursor::Address(*address),
                    _ => return Err(format!("signature matched {} addresses", found.len())),
                }
            }
            Step::In(function) => {
                let start = self.value_of(function).get();
                let ranges = angelica_map::function_code(self.image, start)
                    .map(|code| code.ranges)
                    .unwrap_or_default();
                if ranges.is_empty() {
                    return Err(format!("`{function}` has no code to search"));
                }
                *region = Some(ranges);
                cursor
            }
            Step::Offset(n) => match cursor {
                Cursor::Address(address) => Cursor::Address(self.within(i64::from(address) + n)?),
                Cursor::Number(number) => {
                    Cursor::Number(number.checked_add(*n).ok_or("the offset overflows")?)
                }
                Cursor::Empty => return Err("needs a value".to_owned()),
            },
            Step::Rel32(n) => {
                let at = self.within(i64::from(address()?) + n)?;
                Cursor::Address(
                    self.image
                        .follow_rel32(at)
                        .ok_or("the operand's target is outside the image")?,
                )
            }
            Step::Rel8(n) => {
                let at = self.within(i64::from(address()?) + n)?;
                let byte = self
                    .image
                    .bytes_at(at, 1)
                    .ok_or("the operand is outside the image")?[0];
                #[allow(clippy::cast_possible_wrap)]
                let displacement = i64::from(byte as i8);
                Cursor::Address(self.within(i64::from(at) + 1 + displacement)?)
            }
            Step::Operand { kind, index } => self.operand(address()?, *kind, *index)?,
            Step::Deref => {
                let at = address()?;
                let section = self
                    .image
                    .section_at(at)
                    .filter(|section| !section.is_code())
                    .ok_or("the cursor is not in a data section")?;
                let _ = section;
                let pointer = self
                    .image
                    .read_u64(at)
                    .ok_or("the pointer is outside the image")?;
                Cursor::Address(
                    self.rva_of(pointer)
                        .ok_or("the pointer is outside the image")?,
                )
            }
            Step::Vtable { vtable, slot } => {
                let Some(Entry {
                    kind: Kind::Vtable { slots, code_slots },
                    ..
                }) = self.anatomia.entries.get(vtable)
                else {
                    return Err(format!("`{vtable}` is not a virtual table"));
                };
                let index = match slot {
                    Slot::Index(index) => *index,
                    Slot::Name(name) => slots
                        .get(name)
                        .map(|(index, _)| *index)
                        .ok_or_else(|| format!("`{vtable}` has no slot `{name}`"))?,
                };
                let declared = slots.values().any(|(slot, _)| *slot == index);
                if index >= *code_slots && !declared {
                    return Err(format!("`{vtable}` declares no slot {index}"));
                }
                let table = self.value_of(vtable).get();
                Cursor::Address(self.slot_target(table, index).ok_or_else(|| {
                    format!("slot {index} of `{vtable}` does not hold an address in the image")
                })?)
            }
            Step::Entry(name) => match self.value_of(name) {
                Value::Address(address) => Cursor::Address(address),
                Value::Offset(offset) => Cursor::Number(i64::from(offset)),
            },
            Step::Import { dll, function } => {
                let found = self.image.import_slots(dll, function);
                match found.as_slice() {
                    [] => return Err(format!("the image does not import `{dll}!{function}`")),
                    [slot] => Cursor::Address(*slot),
                    _ => return Err(format!("`{dll}!{function}` has {} slots", found.len())),
                }
            }
        })
    }

    fn check(&self, check: &Check, value: Value) -> Result<(), String> {
        match check {
            Check::Bytes { bytes, at } => {
                let address = self.within(i64::from(value.get()) + at)?;
                if self.image.bytes_at(address, bytes.len()) == Some(bytes.as_slice()) {
                    Ok(())
                } else {
                    Err("the bytes differ".to_owned())
                }
            }
            Check::Contains {
                signature,
                function,
            } => {
                let start = function
                    .as_deref()
                    .map_or(value, |name| self.value_of(name))
                    .get();
                let ranges = angelica_map::function_code(self.image, start)
                    .map(|code| code.ranges)
                    .unwrap_or_default();
                if ranges.is_empty() {
                    return Err(format!(
                        "`{}` has no code to search",
                        function.as_deref().unwrap_or("the entry")
                    ));
                }
                match self.scan_ranges(signature, &ranges).len() {
                    0 => Err("signature has no match".to_owned()),
                    1 => Ok(()),
                    found => Err(format!("signature matched {found} addresses")),
                }
            }
            Check::Find(steps) => {
                let found = match self.run(steps).map_err(|absence| absence.0)? {
                    Cursor::Address(address) => Value::Address(address),
                    Cursor::Number(number) => {
                        Value::Offset(u32::try_from(number).map_err(|_| format!("found {number}"))?)
                    }
                    Cursor::Empty => return Err("`find` ends without a value".to_owned()),
                };
                same(found, value)
            }
            Check::Equals(name) => same(self.value_of(name), value),
            Check::InVtable { slot, code_slots } => {
                if self.in_vtable(value.get(), *slot, *code_slots) {
                    Ok(())
                } else {
                    Err(format!(
                        "address is not slot {slot} of a table whose first {code_slots} slots point into code"
                    ))
                }
            }
            Check::Immediate { at, equals } => {
                let address = self.within(i64::from(value.get()) + at)?;
                match self.operand(address, OperandKind::Imm, None)? {
                    Cursor::Number(found) if found == *equals => Ok(()),
                    Cursor::Number(found) => Err(format!("the immediate is {found:#X}")),
                    _ => Err("the instruction has no immediate".to_owned()),
                }
            }
        }
    }

    fn operand(
        &self,
        address: u32,
        kind: OperandKind,
        index: Option<u32>,
    ) -> Result<Cursor, String> {
        let code = self
            .image
            .bytes_at(address, 15)
            .or_else(|| {
                (1..15)
                    .rev()
                    .find_map(|length| self.image.bytes_at(address, length))
            })
            .ok_or("the instruction is outside the image")?;
        let mut decoder = Decoder::with_ip(
            64,
            code,
            self.load_base + u64::from(address),
            DecoderOptions::NONE,
        );
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err("no instruction at the cursor".to_owned());
        }
        let operand = pick_operand(&instruction, kind, index)
            .ok_or_else(|| format!("the instruction has no `{}` operand there", kind.name()))?;
        match kind {
            OperandKind::Disp => {
                if instruction.memory_base() == Register::RIP
                    || instruction.memory_base() == Register::None
                {
                    return Err(
                        "matched instruction has no memory operand with a displacement".to_owned(),
                    );
                }
                let displacement = u32::try_from(instruction.memory_displacement64())
                    .map_err(|_| "the displacement is negative".to_owned())?;
                Ok(Cursor::Number(i64::from(displacement)))
            }
            OperandKind::Imm => {
                #[allow(clippy::cast_possible_wrap)]
                let value = instruction.immediate(operand) as i64;
                Ok(Cursor::Number(value))
            }
            OperandKind::Target => {
                let target = if instruction.op_kind(operand) == OpKind::Memory {
                    instruction.ip_rel_memory_address()
                } else {
                    instruction.near_branch_target()
                };
                Ok(Cursor::Address(
                    self.rva_of(target)
                        .ok_or("the target is outside the image")?,
                ))
            }
        }
    }

    fn value_of(&self, name: &str) -> Value {
        match self.results.get(name) {
            Some(Ok(value)) => *value,
            _ => unreachable!("references are checked before use"),
        }
    }

    fn within(&self, address: i64) -> Result<u32, String> {
        u32::try_from(address)
            .ok()
            .filter(|&address| address < self.image.size_of_image())
            .ok_or_else(|| "address is outside the image".to_owned())
    }

    fn rva_of(&self, absolute: u64) -> Option<u32> {
        let rva = u32::try_from(absolute.checked_sub(self.load_base)?).ok()?;
        (rva < self.image.size_of_image()).then_some(rva)
    }

    fn scan_ranges(&self, signature: &Signature, ranges: &[std::ops::Range<u32>]) -> Vec<u32> {
        ranges
            .iter()
            .filter_map(|range| {
                self.image
                    .bytes_at(range.start, (range.end - range.start) as usize)
                    .map(|bytes| (range.start, bytes))
            })
            .flat_map(|(start, bytes)| {
                signature
                    .find_in(bytes)
                    .into_iter()
                    .filter_map(move |offset| {
                        u32::try_from(offset).ok().map(|offset| start + offset)
                    })
            })
            .collect()
    }

    fn scan_data(&self, signature: &Signature) -> Vec<u32> {
        let mut found = Vec::new();
        for section in self
            .image
            .sections()
            .iter()
            .filter(|section| !section.is_code())
        {
            let data = self.image.section_data(section);
            found.extend(signature.find_in(data).into_iter().filter_map(|offset| {
                u32::try_from(offset)
                    .ok()
                    .map(|offset| section.virtual_address + offset)
            }));
        }
        found.sort_unstable();
        found
    }

    fn slot_target(&self, table: u32, slot: u32) -> Option<u32> {
        let address = self
            .image
            .read_u64(table.checked_add(slot.checked_mul(8)?)?)?;
        self.rva_of(address)
    }

    fn in_vtable(&self, address: u32, slot: u32, code_slots: u32) -> bool {
        let Some(absolute) = self.load_base.checked_add(u64::from(address)) else {
            return false;
        };
        let signature =
            Signature::from_bytes(&absolute.to_le_bytes()).expect("eight bytes form a signature");
        self.scan_data(&signature).into_iter().any(|reference| {
            reference % 8 == 0
                && reference.checked_sub(slot * 8).is_some_and(|table| {
                    (0..code_slots).all(|index| {
                        self.slot_target(table, index)
                            .is_some_and(|target| self.image.is_code(target))
                    })
                })
        })
    }
}

fn same(found: Value, value: Value) -> Result<(), String> {
    if found == value {
        Ok(())
    } else {
        Err(format!("found {:#X}, not the entry's value", found.get()))
    }
}

/// The operand a step takes: at `index`, or the first of the kind.
fn pick_operand(instruction: &Instruction, kind: OperandKind, index: Option<u32>) -> Option<u32> {
    let fits = |operand: u32| {
        let op_kind = instruction.op_kind(operand);
        match kind {
            OperandKind::Disp => op_kind == OpKind::Memory,
            OperandKind::Imm => matches!(
                op_kind,
                OpKind::Immediate8
                    | OpKind::Immediate16
                    | OpKind::Immediate32
                    | OpKind::Immediate64
                    | OpKind::Immediate8to16
                    | OpKind::Immediate8to32
                    | OpKind::Immediate8to64
                    | OpKind::Immediate32to64
            ),
            OperandKind::Target => {
                matches!(op_kind, OpKind::NearBranch64)
                    || (op_kind == OpKind::Memory && instruction.is_ip_rel_memory_operand())
            }
        }
    };
    match index {
        Some(index) => (index < instruction.op_count() && fits(index)).then_some(index),
        None => (0..instruction.op_count()).find(|operand| fits(*operand)),
    }
}
