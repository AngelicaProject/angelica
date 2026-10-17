//! Entries plugins add to the game's menus (places of the kind
//! `menu-entry`): the host keeps what each plugin returned last frame, the
//! adapter shows them when the game opens a menu, and what the player
//! chose reaches the plugin as an event in the next frame.
#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, PoisonError},
};

/// The most entries of plugins one menu shows.
pub const MOST_SHOWN: usize = 8;
/// The most choices kept between two frames.
const MOST_CHOSEN: usize = 64;

/// An entry as the game's menu shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub plugin: String,
    pub entry: String,
    /// The game's text of the label, in the color of plugin names.
    pub label: Vec<u8>,
}

/// A choice of the player: the plugin, its entry, and what the menu was
/// opened for (for an item, its row of `Item`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub plugin: String,
    pub entry: String,
    pub value: u32,
}

#[derive(Default)]
pub struct Menus {
    /// The item menu entries of each plugin, by plugin id, as it returned
    /// them last.
    items: Mutex<BTreeMap<String, Vec<(String, String)>>>,
    chosen: Mutex<VecDeque<Chosen>>,
}

impl Menus {
    /// Keeps the item menu entries a plugin returned this frame, as
    /// `(id, label)`.
    pub fn set_items(&self, plugin: &str, entries: Vec<(String, String)>) {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.is_empty() {
            items.remove(plugin);
        } else {
            items.insert(plugin.to_owned(), entries);
        }
    }

    /// Whether any plugin has item menu entries now.
    pub fn has_items(&self) -> bool {
        !self
            .items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    /// The entries an item menu shows now, at most [`MOST_SHOWN`], by
    /// plugin id and then in the order each plugin returned them.
    pub fn items(&self) -> Vec<Shown> {
        self.items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .flat_map(|(plugin, entries)| {
                entries.iter().map(move |(entry, label)| Shown {
                    plugin: plugin.clone(),
                    entry: entry.clone(),
                    label: crate::chat::named(label),
                })
            })
            .take(MOST_SHOWN)
            .collect()
    }

    /// Notes that the player chose an entry; the oldest choices beyond
    /// what the next frame takes are left out.
    pub fn choose(&self, chosen: Chosen) {
        let mut queue = self.chosen.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.len() < MOST_CHOSEN {
            queue.push_back(chosen);
        }
    }

    /// The choices since the last call, in order.
    pub fn take_chosen(&self) -> Vec<Chosen> {
        self.chosen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect()
    }

    /// Forgets a plugin's entries and choices, as when it stops.
    pub fn remove_plugin(&self, plugin: &str) {
        self.items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin);
        self.chosen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|chosen| chosen.plugin != plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_shows_the_entries_plugins_returned_last() {
        let menus = Menus::default();
        assert!(!menus.has_items());
        menus.set_items("b", vec![("look".into(), "Look up".into())]);
        menus.set_items(
            "a",
            (0..MOST_SHOWN)
                .map(|n| (format!("e{n}"), format!("Entry {n}")))
                .collect(),
        );
        let shown = menus.items();
        assert_eq!(shown.len(), MOST_SHOWN);
        assert!(shown.iter().all(|entry| entry.plugin == "a"));
        assert_eq!(crate::chat::plain(&shown[0].label), "Entry 0");
        menus.set_items("a", Vec::new());
        assert_eq!(menus.items()[0].entry, "look");

        menus.choose(Chosen {
            plugin: "b".into(),
            entry: "look".into(),
            value: 5057,
        });
        menus.remove_plugin("b");
        assert!(menus.take_chosen().is_empty());
        assert!(!menus.has_items());
    }
}
