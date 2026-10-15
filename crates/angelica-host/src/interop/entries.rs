//! Map entries as the adapters use them, checked for the kind, type, and
//! ABI each adapter was written against.
use crate::catalog::Catalog;
use angelica_map::{EntryKind, Value, ValueKind};

pub struct Entries<'a> {
    pub catalog: &'a Catalog,
    pub base: usize,
}

/// A function: its address, and its code as a hook checks it, or why that
/// is not known.
pub struct Function {
    pub address: usize,
    pub code: Result<angelica_hook::FunctionCode, String>,
}

#[cfg(test)]
impl Function {
    /// A function whose machine code is copied to executable memory.
    pub fn synthetic(code: &[u8]) -> Self {
        let address = angelica_hook::testing::executable(code);
        Self {
            address,
            code: Ok(angelica_hook::testing::whole(address, code.len())),
        }
    }
}

impl Entries<'_> {
    pub fn function(
        &self,
        name: &str,
        args: &[ValueKind],
        ret: ValueKind,
    ) -> Result<Function, String> {
        let entry = self
            .catalog
            .present
            .get(name)
            .ok_or_else(|| self.absent(name))?;
        match (&entry.entry.kind, entry.value) {
            (EntryKind::Function { abi, .. }, Value::Address(rva))
                if abi.args == args && abi.ret == ret =>
            {
                Ok(Function {
                    address: self.base + rva as usize,
                    code: entry.hook_code(self.base),
                })
            }
            _ => Err(format!(
                "map entry {name} is not a function with the expected ABI"
            )),
        }
    }

    pub fn field(&self, name: &str, value: ValueKind) -> Result<usize, String> {
        let entry = self
            .catalog
            .present
            .get(name)
            .ok_or_else(|| self.absent(name))?;
        match (&entry.entry.kind, entry.value) {
            (EntryKind::Field { value: kind, .. }, Value::Offset(offset)) if *kind == value => {
                Ok(offset as usize)
            }
            _ => Err(format!(
                "map entry {name} is not a field of the expected type"
            )),
        }
    }

    pub fn global(&self, name: &str, value: ValueKind) -> Result<usize, String> {
        let entry = self
            .catalog
            .present
            .get(name)
            .ok_or_else(|| self.absent(name))?;
        match (&entry.entry.kind, entry.value) {
            (EntryKind::Global { value: kind }, Value::Address(rva)) if *kind == value => {
                Ok(self.base + rva as usize)
            }
            _ => Err(format!(
                "map entry {name} is not a global of the expected type"
            )),
        }
    }

    fn absent(&self, name: &str) -> String {
        match self.catalog.absent.get(name) {
            Some(reason) => format!("map entry {name} is absent: {reason}"),
            None => format!("map entry {name} is not in the map"),
        }
    }
}
