//! Direct access to the game's objects that Anatomia describes: the
//! central description of the game and the plugin's own entries
//! (`anatomia/` beside `angelica.toml`).
//!
//! A plugin holds handles, never addresses. Views wrap them with the
//! fields of their structure: [`crate::game`] has a view of every structure
//! of the central description, and [`angelica-views`] generates them for
//! the plugin's own entries at build time. Every read is a checked copy of
//! the game's memory; reading a central field needs its domain in the
//! manifest's `[lens] read`.
//!
//! ```ignore
//! use angelica_sdk::game::{character, inventory};
//!
//! if let Some(player) = character::local_player()? {
//!     let level = player.data()?.level()?;
//! }
//! let bags = inventory::inventory_manager_instance()?;
//! if let Some(containers) = bags.containers()? {
//!     for container in containers.iter() {
//!         // An element of a domain the plugin does not hold fails alone.
//!         let Ok(container) = container else { continue };
//!         let size = container.size()?;
//!     }
//! }
//! ```
//!
//! A handle lives until the next `tick` in the main instance and until the
//! handler returns in a handler. [`hold`] keeps one across frames while the
//! path it was reached by from its global still reaches the same object.
//!
//! [`angelica-views`]: https://github.com/AngelicaProject/angelica/tree/main/crates/angelica-views

use crate::Error;
pub use crate::api::memory::{FieldId, Handle, Value};
use std::marker::PhantomData;
use std::sync::OnceLock;

/// A field of a structure, looked up once on first use.
pub struct Field {
    name: &'static str,
    id: OnceLock<Result<FieldId, Error>>,
}

impl Field {
    /// The field `<structure>.<field>`.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            id: OnceLock::new(),
        }
    }

    /// The field's name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The field's id in this instance.
    ///
    /// # Errors
    /// An unknown field, or one this game build lacks.
    pub fn id(&self) -> Result<FieldId, Error> {
        self.id
            .get_or_init(|| crate::api::memory::field(self.name))
            .clone()
    }
}

/// A typed view of an object of a structure.
pub trait View: Sized + Copy {
    /// The structure's name in Anatomia.
    const STRUCTURE: &'static str;

    /// A view of the object `handle` stands for.
    fn from_handle(handle: Handle) -> Self;

    /// The handle the view reads through.
    fn handle(&self) -> Handle;
}

/// What a read gives, as a Rust value.
pub trait FromValue: Sized {
    /// # Errors
    /// A value of another kind.
    fn from_value(value: Value) -> Result<Self, Error>;
}

/// What a write takes, as a Rust value.
pub trait IntoValue {
    /// The value as the host takes it.
    fn into_value(self) -> Value;
}

fn wrong(expected: &str, value: &Value) -> Error {
    Error {
        kind: crate::ErrorKind::InvalidArgument,
        message: format!("expected {expected}, got {value:?}"),
    }
}

macro_rules! integers {
    ($($ty:ty => $variant:ident),* $(,)?) => {$(
        impl FromValue for $ty {
            fn from_value(value: Value) -> Result<Self, Error> {
                match value {
                    Value::$variant(number) => {
                        <$ty>::try_from(number).map_err(|_| wrong(stringify!($ty), &value))
                    }
                    other => Err(wrong(stringify!($ty), &other)),
                }
            }
        }

        impl IntoValue for $ty {
            fn into_value(self) -> Value {
                Value::$variant(self.into())
            }
        }
    )*};
}

integers!(
    u8 => Unsigned, u16 => Unsigned, u32 => Unsigned, u64 => Unsigned,
    i8 => Signed, i16 => Signed, i32 => Signed, i64 => Signed,
);

impl FromValue for bool {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Boolean(flag) => Ok(flag),
            other => Err(wrong("bool", &other)),
        }
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Boolean(self)
    }
}

impl FromValue for f32 {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Float32(number) => Ok(number),
            other => Err(wrong("f32", &other)),
        }
    }
}

impl IntoValue for f32 {
    fn into_value(self) -> Value {
        Value::Float32(self)
    }
}

impl FromValue for f64 {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Float64(number) => Ok(number),
            other => Err(wrong("f64", &other)),
        }
    }
}

impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::Float64(self)
    }
}

/// Text, as the game stores it, without its terminator.
impl FromValue for Vec<u8> {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Text(text) => Ok(text),
            Value::None => Ok(Vec::new()),
            other => Err(wrong("text", &other)),
        }
    }
}

impl IntoValue for &[u8] {
    fn into_value(self) -> Value {
        Value::Text(self.to_vec())
    }
}

/// A pointer: `None` while it points to nothing.
impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::None => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

impl<V: View> IntoValue for Option<&V> {
    fn into_value(self) -> Value {
        self.map_or(Value::None, |view| Value::Object(view.handle()))
    }
}

/// Memory of a type the description does not give: it can be written into
/// a pointer field of the same type, not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointer(pub Handle);

impl FromValue for Pointer {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Pointer(handle) => Ok(Self(handle)),
            other => Err(wrong("a pointer", &other)),
        }
    }
}

impl IntoValue for Option<Pointer> {
    fn into_value(self) -> Value {
        self.map_or(Value::None, |pointer| Value::Pointer(pointer.0))
    }
}

/// The view of an object a read gave, for a view's [`FromValue`].
///
/// # Errors
/// A value that is not an object.
pub fn object<V: View>(value: Value) -> Result<V, Error> {
    match value {
        Value::Object(handle) => Ok(V::from_handle(handle)),
        other => Err(wrong(V::STRUCTURE, &other)),
    }
}

/// An array of the game: its elements in use and each one.
#[derive(Debug, PartialEq, Eq)]
pub struct Array<T> {
    handle: Handle,
    element: PhantomData<fn() -> T>,
}

impl<T> Clone for Array<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Array<T> {}

impl<T> FromValue for Array<T> {
    fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Array(handle) => Ok(Self {
                handle,
                element: PhantomData,
            }),
            other => Err(wrong("an array", &other)),
        }
    }
}

impl<T: FromValue> Array<T> {
    /// The handle of the array.
    #[must_use]
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// How many elements are in use.
    ///
    /// # Errors
    /// A handle that is no longer valid.
    pub fn len(&self) -> Result<u32, Error> {
        crate::api::memory::length(self.handle)
    }

    /// Whether no element is in use.
    ///
    /// # Errors
    /// As [`Array::len`].
    pub fn is_empty(&self) -> Result<bool, Error> {
        Ok(self.len()? == 0)
    }

    /// The element at `index`.
    ///
    /// # Errors
    /// An index past the elements in use, and an element of a domain the
    /// plugin does not hold.
    pub fn get(&self, index: u32) -> Result<T, Error> {
        T::from_value(crate::api::memory::element(self.handle, index)?)
    }

    /// Every element in use, each read on its own: one the plugin may not
    /// read is an error and the others go on.
    pub fn iter(&self) -> impl Iterator<Item = Result<T, Error>> + '_ {
        let length = self.len();
        let (length, failed) = match length {
            Ok(length) => (length, None),
            Err(error) => (0, Some(error)),
        };
        failed
            .map(Err)
            .into_iter()
            .chain((0..length).map(|index| self.get(index)))
    }
}

/// Reads `field` of the object `of`.
///
/// # Errors
/// A field of another structure, a domain the plugin does not hold, memory
/// that cannot be read, and a value of another kind than `T`.
pub fn read<T: FromValue>(of: Handle, field: &Field) -> Result<T, Error> {
    T::from_value(crate::api::memory::read(of, field.id()?)?)
}

/// Writes `value` into `field` of the object `of`.
///
/// # Errors
/// A field the plugin may not write, a value that does not fit it, and
/// memory that cannot be written.
pub fn write(of: Handle, field: &Field, value: impl IntoValue) -> Result<(), Error> {
    crate::api::memory::write(of, field.id()?, &value.into_value())
}

/// Reads many fields in one call into the host.
///
/// # Errors
/// A field that cannot be looked up; each read fails on its own.
pub fn read_batch(reads: &[(Handle, &Field)]) -> Result<Vec<Result<Value, Error>>, Error> {
    let reads = reads
        .iter()
        .map(|(handle, field)| Ok((*handle, field.id()?)))
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(crate::api::memory::read_batch(&reads))
}

/// The object of the global `name`, or what it holds.
///
/// # Errors
/// An unknown global, one this build lacks, and a value of another kind.
pub fn global<T: FromValue>(name: &str) -> Result<T, Error> {
    T::from_value(crate::api::memory::global(name)?)
}

/// The object `view` stands for as a `V`, when it is one.
///
/// # Errors
/// Structures that do not extend one another, and an object whose
/// structure the description cannot tell.
pub fn cast<V: View>(view: &impl View) -> Result<Option<V>, Error> {
    Ok(crate::api::memory::cast(view.handle(), V::STRUCTURE)?.map(V::from_handle))
}

/// The text of an object whose structure reads as text.
///
/// # Errors
/// An object that does not read as text, and text that cannot be read.
pub fn text(of: &impl View) -> Result<Vec<u8>, Error> {
    crate::api::memory::text(of.handle())
}

/// Replaces the text of an object of the plugin's own structures with as
/// many bytes, in place.
///
/// # Errors
/// Another length, a NUL, and text that cannot be written.
pub fn edit_text(of: &impl View, bytes: &[u8]) -> Result<(), Error> {
    crate::api::memory::edit_text(of.handle(), bytes)
}

/// A view kept across frames: each use checks that the path it was reached
/// by still reaches the same object. Released when dropped.
#[derive(Debug)]
pub struct Held<V: View> {
    view: V,
}

/// In the main instance: keeps `view` across frames; at most 256 at once.
///
/// # Errors
/// A view reached through an intercepted call, and too many held.
pub fn hold<V: View>(view: &V) -> Result<Held<V>, Error> {
    let handle = crate::api::memory::hold(view.handle())?;
    Ok(Held {
        view: V::from_handle(handle),
    })
}

impl<V: View> Held<V> {
    /// The view, valid while the object stays where its path leads; reads
    /// through it fail once it is gone.
    #[must_use]
    pub fn get(&self) -> V {
        self.view
    }
}

impl<V: View> Drop for Held<V> {
    fn drop(&mut self) {
        crate::api::memory::release(self.view.handle());
    }
}
