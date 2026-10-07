//! Moving the first instructions of a function into a trampoline.

use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Code, Decoder, DecoderOptions, FlowControl, Instruction,
    InstructionBlock, OpKind,
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

/// The first relative branch in `function`, located at `ip`, whose target
/// lies inside `[start, start + len)` but is not `start` itself: code that
/// would jump into the middle of bytes a detour overwrites. Returns the
/// branch's address and its target.
#[must_use]
pub fn branch_into(function: &[u8], ip: u64, start: u64, len: usize) -> Option<(u64, u64)> {
    let end = start + len as u64;
    let mut decoder = Decoder::with_ip(64, function, ip, DecoderOptions::NONE);
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            continue;
        }
        let is_branch = (0..instruction.op_count()).any(|operand| {
            matches!(
                instruction.op_kind(operand),
                OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
            )
        });
        let target = instruction.near_branch_target();
        if is_branch && target > start && target < end {
            return Some((instruction.ip(), target));
        }
    }
    None
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
        assert_eq!(branch_into(&code, IP, IP, 7), Some((IP + 7, IP + 2)));
        // A branch to the start itself is allowed.
        assert_eq!(branch_into(&code, IP, IP + 2, 5), None);
        assert_eq!(branch_into(&code, IP, IP, 2), None);
    }
}
