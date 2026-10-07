//! Modifications of code in the current process: entry detours, hooks
//! inside functions, code patches, and vtable slot replacement.
//!
//! Relocation of the instructions a detour overwrites ([`relocate`]) is pure
//! and runs anywhere. Installing anything is Windows-only: it allocates
//! executable memory near the target, suspends the other threads of the
//! process while bytes change, and moves a suspended thread whose
//! instruction pointer is inside the changed bytes to the equivalent
//! instruction.
//!
//! Memory that held a trampoline or a relay is never freed: a thread may
//! still be executing in it after a detour is removed.

mod error;
pub mod relocate;

#[cfg(windows)]
mod detour;
#[cfg(windows)]
mod memory;
#[cfg(windows)]
mod mid;
#[cfg(windows)]
mod patch;
#[cfg(windows)]
mod splice;
#[cfg(windows)]
mod threads;
#[cfg(windows)]
mod vtable;

#[cfg(all(windows, feature = "testing"))]
pub mod testing;

#[cfg(windows)]
pub use detour::Detour;
pub use error::HookError;
#[cfg(windows)]
pub use mid::{Context, MidHandler, MidHook};
#[cfg(windows)]
pub use patch::CodePatch;
pub use relocate::RelocateError;
#[cfg(windows)]
pub use vtable::VtableSlot;
