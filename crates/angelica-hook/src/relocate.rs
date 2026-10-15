//! Moving the first instructions of a function into a trampoline.

use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Code, Decoder, DecoderOptions, FlowControl, Instruction,
    InstructionBlock, OpKind, Register,
};

/// The length of the jump a detour writes over the start of a function: a
/// `jmp rel32`.
pub const JUMP_LEN: usize = 5;

/// Why instructions cannot be moved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelocateError {
    #[error("the instruction at offset {0} is invalid or cut off")]
    Invalid(usize),
    #[error("the code ends with an unconditional transfer at offset {0}, before {JUMP_LEN} bytes")]
    TooShort(usize),
    #[error("the moved instructions cannot be encoded at their new address: {0}")]
    Encode(String),
}

/// The first instructions of a function, moved to a new address and
/// followed by a jump back to the instruction after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    /// The number of original bytes the moved instructions occupied.
    pub stolen_len: usize,
    /// The trampoline's code, to be placed at the new address.
    pub code: Vec<u8>,
    /// For each moved instruction, its offset in the original code and its
    /// offset in the trampoline.
    pub offsets: Vec<(u32, u32)>,
}

impl Relocation {
    /// The trampoline offset of the moved instruction at `offset` in the
    /// original code.
    #[must_use]
    pub fn to_trampoline(&self, offset: u32) -> Option<u32> {
        self.offsets
            .iter()
            .find(|&&(old, _)| old == offset)
            .map(|&(_, new)| new)
    }

    /// The original offset of the moved instruction at `offset` in the
    /// trampoline.
    #[must_use]
    pub fn to_original(&self, offset: u32) -> Option<u32> {
        self.offsets
            .iter()
            .find(|&&(_, new)| new == offset)
            .map(|&(old, _)| old)
    }
}

/// Decodes whole instructions from the start of `code`, located at `ip`,
/// until they cover at least `min_len` bytes, and encodes them at `new_ip`
/// followed by a jump back to `ip + stolen_len`. Relative branches and
/// RIP-relative operands keep their targets.
///
/// # Errors
///
/// Returns a [`RelocateError`] when an instruction is invalid, the code
/// leaves unconditionally before `min_len` bytes, or the instructions cannot
/// be encoded at `new_ip` (a RIP-relative operand out of reach).
pub fn relocate(
    code: &[u8],
    ip: u64,
    min_len: usize,
    new_ip: u64,
) -> Result<Relocation, RelocateError> {
    let mut decoder = Decoder::with_ip(64, code, ip, DecoderOptions::NONE);
    let mut instructions = Vec::new();
    let mut stolen_len = 0;
    while stolen_len < min_len {
        if !decoder.can_decode() {
            return Err(RelocateError::Invalid(stolen_len));
        }
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(RelocateError::Invalid(stolen_len));
        }
        stolen_len += instruction.len();
        let leaves = matches!(
            instruction.flow_control(),
            FlowControl::Return
                | FlowControl::UnconditionalBranch
                | FlowControl::IndirectBranch
                | FlowControl::Interrupt
        );
        if leaves && stolen_len < min_len {
            return Err(RelocateError::TooShort(stolen_len - instruction.len()));
        }
        instructions.push(instruction);
    }

    let back = ip + stolen_len as u64;
    let mut block = instructions.clone();
    block.push(
        Instruction::with_branch(Code::Jmp_rel32_64, back)
            .map_err(|error| RelocateError::Encode(error.to_string()))?,
    );
    let encoded = BlockEncoder::encode(
        64,
        InstructionBlock::new(&block, new_ip),
        BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS,
    )
    .map_err(|error| RelocateError::Encode(error.to_string()))?;

    let offsets = instructions
        .iter()
        .zip(&encoded.new_instruction_offsets)
        .map(|(instruction, &new)| {
            // Offsets fit in 32 bits: the stolen code is a few bytes long.
            let old = u32::try_from(instruction.ip() - ip).unwrap_or(u32::MAX);
            (old, new)
        })
        .collect();
    Ok(Relocation {
        stolen_len,
        code: encoded.code_buffer,
        offsets,
    })
}

/// Why a function's code cannot be shown free of branches into the bytes a
/// modification overwrites.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlowError {
    #[error(
        "an instruction at {from:#X} branches to {to:#X}, inside the bytes a detour overwrites"
    )]
    BranchInto { from: u64, to: u64 },
    #[error("the function's code reaches an invalid instruction at {0:#X}")]
    Invalid(u64),
    #[error("the function jumps at {0:#X} to targets its code does not state")]
    IndirectJump(u64),
    #[error("{0:#X} is not an instruction of the function's code")]
    NotAnInstruction(u64),
}

/// Checks that no code of a function branches into the `len` bytes at
/// `location` that a modification overwrites, except to `location` itself.
///
/// `chunks` are the function's ranges of code, each at its address, and
/// `start` is the function's first instruction. When the bytes lie within
/// the function's prologue of `prologue` bytes, nothing branches into them:
/// the prologue runs only when the function is entered. Otherwise the code
/// is followed from the start of every chunk: each instruction to the next,
/// and every branch whose target is in the function; it ends at a return,
/// `int3`, `ud2`, an unconditional jump, and a jump through a RIP-relative
/// pointer. A jump through anything else has targets the code does not
/// state, and the check fails.
///
/// # Errors
///
/// Returns a [`FlowError`] when a branch targets the bytes, the followed
/// code reaches an invalid instruction or a jump through a register or
/// computed address, or `location` is not an instruction the followed code
/// reaches.
pub fn check_flow(
    chunks: &[(u64, &[u8])],
    start: u64,
    prologue: usize,
    location: u64,
    len: usize,
) -> Result<(), FlowError> {
    let end = location + len as u64;
    if location == start && len <= prologue {
        return Ok(());
    }
    let chunk_of = |address: u64| {
        chunks
            .iter()
            .find(|(begin, bytes)| (*begin..*begin + bytes.len() as u64).contains(&address))
    };
    let mut reached = std::collections::BTreeSet::new();
    let mut work: Vec<u64> = chunks.iter().map(|(begin, _)| *begin).collect();
    while let Some(mut at) = work.pop() {
        while !reached.contains(&at) {
            let Some(&(begin, bytes)) = chunk_of(at) else {
                break;
            };
            let Some(rest) = usize::try_from(at - begin)
                .ok()
                .and_then(|offset| bytes.get(offset..))
            else {
                break;
            };
            let instruction = Decoder::with_ip(64, rest, at, DecoderOptions::NONE).decode();
            if instruction.is_invalid() {
                return Err(FlowError::Invalid(at));
            }
            reached.insert(at);
            let is_branch = (0..instruction.op_count()).any(|operand| {
                matches!(
                    instruction.op_kind(operand),
                    OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
                )
            });
            if is_branch {
                let target = instruction.near_branch_target();
                if target > location && target < end {
                    return Err(FlowError::BranchInto {
                        from: at,
                        to: target,
                    });
                }
            }
            match instruction.flow_control() {
                FlowControl::Return | FlowControl::Interrupt | FlowControl::Exception => break,
                FlowControl::UnconditionalBranch => {
                    work.push(instruction.near_branch_target());
                    break;
                }
                FlowControl::IndirectBranch => {
                    if instruction.memory_base() == Register::RIP {
                        break;
                    }
                    return Err(FlowError::IndirectJump(at));
                }
                FlowControl::ConditionalBranch => work.push(instruction.near_branch_target()),
                _ => {}
            }
            at = instruction.next_ip();
        }
    }
    if reached.contains(&location) {
        Ok(())
    } else {
        Err(FlowError::NotAnInstruction(location))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP: u64 = 0x1_4000_1000;
    const NEW_IP: u64 = 0x1_4800_0000;

    fn decode_all(code: &[u8], ip: u64) -> Vec<Instruction> {
        let mut decoder = Decoder::with_ip(64, code, ip, DecoderOptions::NONE);
        let mut instructions = Vec::new();
        while decoder.can_decode() {
            instructions.push(decoder.decode());
        }
        instructions
    }

    #[test]
    fn moves_whole_instructions_and_jumps_back() {
        // lea rax, [rcx+rdx]; add rax, 0; ret
        let code = [0x48, 0x8D, 0x04, 0x11, 0x48, 0x83, 0xC0, 0x00, 0xC3];
        let relocation = relocate(&code, IP, JUMP_LEN, NEW_IP).unwrap();
        assert_eq!(relocation.stolen_len, 8);
        assert_eq!(relocation.offsets, vec![(0, 0), (4, 4)]);
        assert_eq!(&relocation.code[..8], &code[..8]);
        let moved = decode_all(&relocation.code, NEW_IP);
        let back = moved.last().unwrap();
        assert_eq!(back.flow_control(), FlowControl::UnconditionalBranch);
        assert_eq!(back.near_branch_target(), IP + 8);
        assert_eq!(relocation.to_trampoline(4), Some(4));
        assert_eq!(relocation.to_original(4), Some(4));
        assert_eq!(relocation.to_trampoline(2), None);
    }

    #[test]
    fn keeps_rip_relative_targets() {
        // mov rax, [rip+0x100]; ret
        let code = [0x48, 0x8B, 0x05, 0x00, 0x01, 0x00, 0x00, 0xC3];
        let relocation = relocate(&code, IP, JUMP_LEN, NEW_IP).unwrap();
        let moved = decode_all(&relocation.code, NEW_IP);
        assert_eq!(moved[0].memory_displacement64(), IP + 7 + 0x100);
    }

    #[test]
    fn keeps_branch_targets() {
        // test ecx, ecx; je +3; xor eax, eax; ret; mov eax, 7; ret
        let code = [
            0x85, 0xC9, 0x74, 0x03, 0x31, 0xC0, 0xC3, 0xB8, 0x07, 0x00, 0x00, 0x00, 0xC3,
        ];
        let relocation = relocate(&code, IP, JUMP_LEN, NEW_IP).unwrap();
        assert_eq!(relocation.stolen_len, 6);
        let moved = decode_all(&relocation.code, NEW_IP);
        assert_eq!(moved[1].near_branch_target(), IP + 7);
    }

    #[test]
    fn moves_a_jump_written_by_another_tool() {
        // jmp rel32 to somewhere else: another tool's hook.
        let code = [0xE9, 0x00, 0x10, 0x00, 0x00, 0xCC, 0xCC];
        let relocation = relocate(&code, IP, JUMP_LEN, NEW_IP).unwrap();
        assert_eq!(relocation.stolen_len, 5);
        let moved = decode_all(&relocation.code, NEW_IP);
        assert_eq!(moved[0].near_branch_target(), IP + 5 + 0x1000);
    }

    #[test]
    fn refuses_code_that_leaves_too_early() {
        assert_eq!(
            relocate(&[0xC3, 0x90, 0x90, 0x90, 0x90], IP, JUMP_LEN, NEW_IP),
            Err(RelocateError::TooShort(0))
        );
        // xor eax, eax; jmp short
        assert_eq!(
            relocate(&[0x31, 0xC0, 0xEB, 0x00, 0x90, 0x90], IP, JUMP_LEN, NEW_IP),
            Err(RelocateError::TooShort(2))
        );
    }

    #[test]
    fn refuses_invalid_or_cut_code() {
        assert_eq!(
            relocate(&[0x48, 0x8B], IP, JUMP_LEN, NEW_IP),
            Err(RelocateError::Invalid(0))
        );
        assert_eq!(
            relocate(&[0x90, 0x90], IP, JUMP_LEN, NEW_IP),
            Err(RelocateError::Invalid(2))
        );
    }

    #[test]
    fn refuses_a_rip_relative_operand_out_of_reach() {
        let code = [0x48, 0x8B, 0x05, 0x00, 0x01, 0x00, 0x00, 0xC3];
        assert!(matches!(
            relocate(&code, IP, JUMP_LEN, IP + 0x1_0000_0000),
            Err(RelocateError::Encode(_))
        ));
    }

    #[test]
    fn finds_a_branch_into_the_overwritten_bytes() {
        // 0: xor eax, eax; 2: inc eax; 4: cmp eax, 10; 7: jl 2; 9: ret
        let code = [0x31, 0xC0, 0xFF, 0xC0, 0x83, 0xF8, 0x0A, 0x7C, 0xF9, 0xC3];
        let chunks = [(IP, &code[..])];
        assert_eq!(
            check_flow(&chunks, IP, 0, IP, 7),
            Err(FlowError::BranchInto {
                from: IP + 7,
                to: IP + 2
            })
        );
        // A branch to the location itself is allowed.
        assert_eq!(check_flow(&chunks, IP, 0, IP + 2, 5), Ok(()));
        assert_eq!(check_flow(&chunks, IP, 0, IP, 2), Ok(()));
        // Nothing branches into a prologue.
        assert_eq!(check_flow(&chunks, IP, 7, IP, 7), Ok(()));
        assert_eq!(
            check_flow(&chunks, IP, 0, IP + 5, 1),
            Err(FlowError::NotAnInstruction(IP + 5))
        );
    }

    #[test]
    fn follows_every_chunk_of_the_function() {
        // The function: 0: xor eax, eax; 2: jmp to the chunk. The chunk:
        // 0: inc eax; 2: jl to the function's offset 1; 8: ret.
        let function = [0x31, 0xC0, 0xE9, 0xF9, 0x0F, 0, 0];
        let chunk_ip = IP + 0x1000;
        let back =
            i32::try_from(i64::try_from(IP + 1).unwrap() - i64::try_from(chunk_ip + 8).unwrap())
                .unwrap();
        let mut chunk = vec![0xFF, 0xC0, 0x0F, 0x8C];
        chunk.extend(back.to_le_bytes());
        chunk.push(0xC3);
        assert_eq!(
            check_flow(&[(IP, &function[..]), (chunk_ip, &chunk[..])], IP, 0, IP, 5),
            Err(FlowError::BranchInto {
                from: chunk_ip + 2,
                to: IP + 1
            }),
            "a chained chunk branches into the bytes"
        );
    }

    #[test]
    fn refuses_code_it_cannot_follow() {
        // 0: test ecx, ecx; 2: je 6; 4: jmp rax; 6: ret
        let code = [0x85, 0xC9, 0x74, 0x02, 0xFF, 0xE0, 0xC3];
        assert_eq!(
            check_flow(&[(IP, &code[..])], IP, 0, IP, 5),
            Err(FlowError::IndirectJump(IP + 4))
        );
        // 0: jmp [rip+0], a tail call through a pointer.
        let tail = [0xFF, 0x25, 0, 0, 0, 0];
        assert_eq!(check_flow(&[(IP, &tail[..])], IP, 0, IP, 6), Ok(()));
        // 0: test ecx, ecx; 2: je 4; 4: an invalid instruction.
        let invalid = [0x85, 0xC9, 0x74, 0x00, 0x06];
        assert_eq!(
            check_flow(&[(IP, &invalid[..])], IP, 0, IP, 2),
            Err(FlowError::Invalid(IP + 4))
        );
    }
}
