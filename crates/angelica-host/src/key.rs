//! The key that opens and closes the Angelica menu, and the keys the player
//! can choose for it.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};

/// The keys the menu can be on, by Windows virtual-key code, with the name
/// the player sees and the settings file keeps. None of them types text.
const KEYS: [(u32, &str); 20] = [
    (0x2d, "Insert"),
    (0x2e, "Delete"),
    (0x24, "Home"),
    (0x23, "End"),
    (0x21, "Page Up"),
    (0x22, "Page Down"),
    (0x13, "Pause"),
    (0x91, "Scroll Lock"),
    (0x70, "F1"),
    (0x71, "F2"),
    (0x72, "F3"),
    (0x73, "F4"),
    (0x74, "F5"),
    (0x75, "F6"),
    (0x76, "F7"),
    (0x77, "F8"),
    (0x78, "F9"),
    (0x79, "F10"),
    (0x7a, "F11"),
    (0x7b, "F12"),
];

/// The key that opens and closes the menu; `Insert` without the player's
/// choice.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MenuKey(usize);

impl MenuKey {
    /// `End`: the key that stops plugins acting without the player's choice.
    #[must_use]
    pub fn stop() -> Self {
        Self::from_virtual_key(0x23).unwrap_or_default()
    }

    /// The key with the Windows virtual-key code `code`, when the menu can
    /// be on it.
    #[must_use]
    pub fn from_virtual_key(code: u32) -> Option<Self> {
        KEYS.iter().position(|(key, _)| *key == code).map(Self)
    }

    #[must_use]
    pub fn virtual_key(self) -> u32 {
        KEYS[self.0].0
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        KEYS[self.0].1
    }
}

impl TryFrom<String> for MenuKey {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        KEYS.iter()
            .position(|(_, key)| *key == name)
            .map(Self)
            .ok_or_else(|| format!("the menu cannot be on the key {name}"))
    }
}

impl From<MenuKey> for String {
    fn from(key: MenuKey) -> Self {
        key.name().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_is_on_insert_without_a_choice() {
        assert_eq!(MenuKey::default().name(), "Insert");
        assert_eq!(MenuKey::default().virtual_key(), 0x2d);
    }

    #[test]
    fn every_key_is_found_by_its_code_and_kept_by_its_name() {
        for (index, (code, name)) in KEYS.iter().enumerate() {
            let key = MenuKey::from_virtual_key(*code).unwrap();
            assert_eq!(key, MenuKey(index));
            assert_eq!(key.name(), *name);
            let json = serde_json::to_string(&key).unwrap();
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<MenuKey>(&json).unwrap(), key);
        }
    }

    #[test]
    fn keys_that_type_or_leave_the_menu_are_refused() {
        for code in [0x1b, 0x0d, 0x20, 0x41, 0x10, 0x11, 0x12] {
            assert_eq!(MenuKey::from_virtual_key(code), None);
        }
        assert!(serde_json::from_str::<MenuKey>("\"A\"").is_err());
        assert!(serde_json::from_str::<MenuKey>("\"insert\"").is_err());
    }
}
