use crate::RelocateError;

/// Why a modification cannot be installed or removed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookError {
    #[error(transparent)]
    Relocate(#[from] RelocateError),
    #[error(transparent)]
    Flow(#[from] crate::relocate::FlowError),
    #[error("no free memory within 2 GiB of {0:#X} for a trampoline")]
    NoNearMemory(usize),
    #[error("the bytes at {0:#X} changed since the modification was prepared")]
    Changed(usize),
    #[error("the bytes at {0:#X} are not the expected original bytes")]
    Mismatch(usize),
    #[error("a thread is executing inside the bytes at {0:#X}; try again")]
    ThreadInside(usize),
    #[error("`original` and `replacement` differ in length")]
    LengthMismatch,
    #[error("{call} failed with error {code}")]
    Os { call: &'static str, code: u32 },
}
