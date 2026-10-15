//! The code of a function, exactly: what `in` locators and `contains`
//! checks search, and what the host checks before it hooks a function.

use std::collections::BTreeSet;
use std::ops::Range;

use angelica_pe::Image;
use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, Register};

/// The longest leaf function, from its start to the end of its farthest
/// instruction.
pub(crate) const MAX_LEAF_FUNCTION: u32 = 0x1_0000;

/// The code of one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCode {
    /// The function's first instruction.
    pub start: u32,
    /// Every range of its code: its exception directory entries, chained
    /// chunks included, or the one range of a leaf function.
    pub ranges: Vec<Range<u32>>,
    /// The length of its prologue, from its unwind information; none for a
    /// leaf function.
    pub prologue: Option<u32>,
}

/// The code of the function that starts at `start`: its ranges in the
/// exception directory, or, for a leaf function, the range
/// [`leaf_function`] finds. `None` when `start` is inside another
/// function's code, or the leaf function's code cannot be followed.
#[must_use]
pub fn function_code(image: &Image<'_>, start: u32) -> Option<FunctionCode> {
    let ranges = image.function_chunks(start);
    if !ranges.is_empty() {
        return Some(FunctionCode {
            start,
            ranges,
            prologue: image.prologue_len(start),
        });
    }
    leaf_function(image, start).map(|range| FunctionCode {
        start,
        ranges: vec![range],
        prologue: None,
    })
}

/// The code of the function in the exception directory whose code holds
/// `rva`. `None` for an address outside every entry, as in a leaf function,
/// whose start nothing records.
#[must_use]
pub fn function_holding(image: &Image<'_>, rva: u32) -> Option<FunctionCode> {
    function_code(image, image.function_root(rva)?)
}

/// The code of the leaf function that starts at `start`, which has no
/// exception directory entry: from `start` to the end of the farthest
/// instruction its control flow reaches.
///
/// The flow follows every instruction to the next, conditional branches,
/// and unconditional jumps to code it already spans; it ends at a return,
/// `int3`, `ud2`, and a jump through a RIP-relative pointer, a tail call. A
/// jump beyond what it spans is a tail call unless other flow reaches its
/// target. `None` when the flow reaches an invalid instruction, an indirect
/// jump whose targets are unknown, an address before `start`, the code of a
/// function in the exception directory, or more than 64 KiB from `start`.
#[must_use]
pub fn leaf_function(image: &Image<'_>, start: u32) -> Option<Range<u32>> {
    if image.function_at(start).is_some() || !image.is_code(start) {
        return None;
    }
    let section = image.section_at(start)?;
    let section_end = section
        .virtual_address
        .checked_add(u32::try_from(image.section_data(section).len()).ok()?)?;
    let limit = section_end.min(start.saturating_add(MAX_LEAF_FUNCTION));

    let mut reached = BTreeSet::new();
    let mut work = vec![start];
    let mut jumps = Vec::new();
    let mut end = start;
    loop {
        while let Some(mut at) = work.pop() {
            while !reached.contains(&at) {
                if at < start || at >= limit || image.function_at(at).is_some() {
                    return None;
                }
                let instruction = decode(image, at, limit)?;
                reached.insert(at);
                let next = at + u32::try_from(instruction.len()).ok()?;
                end = end.max(next);
                match instruction.flow_control() {
                    FlowControl::Return | FlowControl::Interrupt | FlowControl::Exception => {
                        break;
                    }
                    FlowControl::UnconditionalBranch => {
                        let target = u32::try_from(instruction.near_branch_target()).ok()?;
                        if (start..end).contains(&target) {
                            work.push(target);
                        } else {
                            jumps.push(target);
                        }
                        break;
                    }
                    FlowControl::IndirectBranch => {
                        if instruction.memory_base() == Register::RIP {
                            break;
                        }
                        return None;
                    }
                    FlowControl::ConditionalBranch => {
                        work.push(u32::try_from(instruction.near_branch_target()).ok()?);
                    }
                    _ => {}
                }
                at = next;
            }
        }
        // A jump whose target the flow now spans stays in the function.
        let (inside, beyond): (Vec<_>, Vec<_>) = jumps
            .into_iter()
            .partition(|target| (start..end).contains(target) && !reached.contains(target));
        jumps = beyond;
        if inside.is_empty() {
            break;
        }
        work = inside;
    }
    Some(start..end)
}

/// The instruction at `at`, which must end before `limit`.
fn decode(image: &Image<'_>, at: u32, limit: u32) -> Option<Instruction> {
    let available = (limit - at).min(15) as usize;
    let bytes = image.bytes_at(at, available)?;
    let instruction = Decoder::with_ip(64, bytes, u64::from(at), DecoderOptions::NONE).decode();
    (!instruction.is_invalid()).then_some(instruction)
}
