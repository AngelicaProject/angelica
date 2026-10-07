//! PE image model and signature scanning.
//!
//! An [`Image`] reads a 64-bit PE image from bytes in either of two layouts:
//! the executable file as stored on disk, or the module as Windows maps it
//! into memory. Both layouts answer in relative virtual addresses (RVAs), so
//! code that resolves the game map gives the same result offline and in the
//! game. The crate only reads the bytes it is given.

mod image;
mod signature;
#[cfg(feature = "testing")]
pub mod testing;

pub use image::{Image, ImageError, Layout, Section};
pub use signature::{Signature, SignatureError};
