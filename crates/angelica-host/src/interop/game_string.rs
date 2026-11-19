use super::{entries::Entries, memory};
use angelica_map::ValueKind::{I64, Ptr, U64, Void};

pub(super) fn read<const N: usize>(address: usize) -> Option<[u8; N]> {
    memory::read(address, N).ok()?.try_into().ok()
}

pub(super) fn pointer(address: usize) -> Option<usize> {
    let value = usize::from_le_bytes(read(address)?);
    (value != 0).then_some(value)
}

pub(super) struct TextLayout {
    string: usize,
    used: usize,
}

impl TextLayout {
    pub(super) fn resolve(entries: &Entries<'_>) -> Result<Self, String> {
        Ok(Self {
            string: entries.field("text.utf8_string.string_ptr", Ptr)?,
            used: entries.field("text.utf8_string.used_size", I64)?,
        })
    }

    pub(super) fn text(&self, object: usize, limit: usize) -> Option<Vec<u8>> {
        if object == 0 {
            return None;
        }
        let size =
            usize::try_from(i64::from_le_bytes(read(object.checked_add(self.used)?)?)).ok()?;
        if size == 0 || size - 1 > limit {
            return None;
        }
        let mut bytes = memory::read(pointer(object.checked_add(self.string)?)?, size).ok()?;
        (bytes.pop() == Some(0)).then_some(bytes)
    }
}

pub(super) struct Strings {
    construct: usize,
    destroy: usize,
}

impl Strings {
    pub(super) fn resolve(entries: &Entries<'_>) -> Result<Self, String> {
        Ok(Self {
            construct: entries
                .function("text.utf8_string.from_sequence", &[Ptr, Ptr, U64], Ptr)?
                .address,
            destroy: entries
                .function("text.utf8_string.dtor", &[Ptr], Void)?
                .address,
        })
    }

    pub(super) fn with<T>(&self, bytes: &[u8], call: impl FnOnce(usize) -> T) -> T {
        type Construct = unsafe extern "system" fn(usize, *const u8, u64) -> usize;
        type Destroy = unsafe extern "system" fn(usize);
        let mut storage = [0_u64; 32];
        let address = storage.as_mut_ptr() as usize;
        // SAFETY: the map verifies the constructor and destructor ABIs. The
        // constructor uses the first 0x62 bytes of aligned storage and copies
        // exactly the supplied bytes. The guard destroys it before storage ends.
        unsafe {
            std::mem::transmute::<usize, Construct>(self.construct)(
                address,
                bytes.as_ptr(),
                bytes.len() as u64,
            );
            let _guard = TemporaryString {
                address,
                destroy: std::mem::transmute::<usize, Destroy>(self.destroy),
            };
            call(address)
        }
    }
}

struct TemporaryString {
    address: usize,
    destroy: unsafe extern "system" fn(usize),
}

impl Drop for TemporaryString {
    fn drop(&mut self) {
        // SAFETY: self owns a live, initialized game string in the caller's storage.
        unsafe { (self.destroy)(self.address) };
    }
}
