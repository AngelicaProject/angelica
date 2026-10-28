//! Reading one entry's TOML table key by key, so that an unknown or
//! malformed key makes only that entry absent.

use toml::{Table, Value};

/// The keys of one table not read yet.
pub(crate) struct Keys {
    table: Table,
}

impl Keys {
    pub(crate) fn new(value: Value) -> Result<Self, String> {
        match value {
            Value::Table(table) => Ok(Self { table }),
            other => Err(format!("is a {}, not a table", other.type_str())),
        }
    }

    pub(crate) fn first_key(&self) -> Option<&str> {
        self.table.keys().next().map(String::as_str)
    }

    pub(crate) fn has(&self, key: &str) -> bool {
        self.table.contains_key(key)
    }

    pub(crate) fn value(&mut self, key: &str) -> Option<Value> {
        self.table.remove(key)
    }

    pub(crate) fn string(&mut self, key: &str) -> Result<Option<String>, String> {
        match self.table.remove(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(other) => Err(format!("`{key}` is a string, not a {}", other.type_str())),
        }
    }

    pub(crate) fn required_string(&mut self, key: &str) -> Result<String, String> {
        self.string(key)?
            .ok_or_else(|| format!("`{key}` is required"))
    }

    pub(crate) fn integer(&mut self, key: &str) -> Result<Option<i64>, String> {
        match self.table.remove(key) {
            None => Ok(None),
            Some(Value::Integer(value)) => Ok(Some(value)),
            Some(other) => Err(format!("`{key}` is an integer, not a {}", other.type_str())),
        }
    }

    pub(crate) fn unsigned(&mut self, key: &str) -> Result<Option<u32>, String> {
        self.integer(key)?
            .map(|value| u32::try_from(value).map_err(|_| format!("`{key}` is 0 to {}", u32::MAX)))
            .transpose()
    }

    pub(crate) fn boolean(&mut self, key: &str) -> Result<Option<bool>, String> {
        match self.table.remove(key) {
            None => Ok(None),
            Some(Value::Boolean(value)) => Ok(Some(value)),
            Some(other) => Err(format!("`{key}` is a boolean, not a {}", other.type_str())),
        }
    }

    pub(crate) fn strings(&mut self, key: &str) -> Result<Vec<String>, String> {
        match self.table.remove(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .into_iter()
                .map(|item| match item {
                    Value::String(text) => Ok(text),
                    other => Err(format!("`{key}` holds strings, not a {}", other.type_str())),
                })
                .collect(),
            Some(other) => Err(format!("`{key}` is an array, not a {}", other.type_str())),
        }
    }

    pub(crate) fn array(&mut self, key: &str) -> Result<Vec<Value>, String> {
        match self.table.remove(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => Ok(items),
            Some(other) => Err(format!("`{key}` is an array, not a {}", other.type_str())),
        }
    }

    pub(crate) fn table(&mut self, key: &str) -> Result<Option<Table>, String> {
        match self.table.remove(key) {
            None => Ok(None),
            Some(Value::Table(table)) => Ok(Some(table)),
            Some(other) => Err(format!("`{key}` is a table, not a {}", other.type_str())),
        }
    }

    /// Refuses the keys nobody read: a key of a newer minor version, or a
    /// misspelling.
    pub(crate) fn finish(self) -> Result<(), String> {
        match self.table.keys().next() {
            None => Ok(()),
            Some(key) => Err(format!(
                "unknown key `{key}`, which this reader of Anatomia {}.{} does not know",
                crate::FORMAT_MAJOR,
                crate::FORMAT_MINOR
            )),
        }
    }
}
