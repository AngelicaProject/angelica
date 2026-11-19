//! An example plugin that reads the player's bags through Lens: one query,
//! prepared when the plugin starts and run while its window is open, and
//! the items' names from the game's Item sheet, searched as the player
//! types.
use angelica_sdk::{
    Plugin, excel, lens,
    ui::{Entry, Event, Tone, Window},
};
use std::collections::{BTreeMap, HashMap};

/// The four bags, without the crystals, the currency, and the gear; a slot
/// that only links to a slot of another container is left out.
const CARRIED: &str = "inventory.containers[kind in (bag_1, bag_2, bag_3, bag_4)]\
                       .items[symbolic = false] { item, quantity, flags }";

/// One slot of a bag, as the query answers it.
#[derive(serde::Deserialize)]
struct Slot {
    /// A row of the Item sheet.
    item: u32,
    quantity: u32,
    /// The names of the item's flags, such as `high_quality`.
    flags: Vec<String>,
}

struct Bags {
    open: bool,
    /// `Err` when the game map cannot answer the query.
    carried: Result<lens::Query, String>,
    search: String,
    /// Item names by row, read once each.
    names: HashMap<u32, String>,
}

impl Bags {
    fn name(&mut self, item: u32) -> String {
        self.names
            .entry(item)
            .or_insert_with(|| {
                excel::row("Item", excel::ENGLISH, &["name"], item)
                    .ok()
                    .flatten()
                    .and_then(|values| values.first().and_then(excel::Value::plain))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Item {item}"))
            })
            .clone()
    }

    /// What the bags hold now: each item and quality once, with the
    /// quantity of every slot, sorted by name.
    fn lines(&mut self) -> Result<Vec<(String, u32)>, String> {
        let carried = self.carried.as_ref().map_err(Clone::clone)?;
        let slots: Vec<Slot> = carried.rows()?;
        let mut held: BTreeMap<(u32, bool), u32> = BTreeMap::new();
        for slot in slots {
            let high_quality = slot.flags.iter().any(|flag| flag == "high_quality");
            *held.entry((slot.item, high_quality)).or_default() += slot.quantity;
        }
        let wanted = self.search.to_lowercase();
        let mut lines: Vec<(String, u32)> = held
            .into_iter()
            .map(|((item, high_quality), quantity)| {
                let name = self.name(item);
                let name = if high_quality {
                    format!("{name} (HQ)")
                } else {
                    name
                };
                (name, quantity)
            })
            .filter(|(name, _)| name.to_lowercase().contains(&wanted))
            .collect();
        lines.sort();
        Ok(lines)
    }
}

impl Plugin for Bags {
    fn start() -> Result<Self, String> {
        Ok(Self {
            open: false,
            carried: lens::Query::prepare(CARRIED).map_err(|error| error.message),
            search: String::new(),
            names: HashMap::new(),
        })
    }

    fn ui(&mut self) -> Vec<Window> {
        if !self.open {
            return Vec::new();
        }
        let window =
            Window::new("main", "Bags").input("search", "Search", &self.search, "Item name");
        let window =
            match self.lines() {
                Err(error) => window.toned(format!("Cannot read the bags: {error}"), Tone::Bad),
                Ok(lines) if lines.is_empty() && self.search.is_empty() => {
                    window.muted("Nothing in your bags")
                }
                Ok(lines) if lines.is_empty() => window.muted("Nothing by that name"),
                Ok(lines) => lines.into_iter().enumerate().fold(
                    window,
                    |window, (index, (name, quantity))| {
                        window.entry(
                            Entry::new(format!("item-{index}"), name).value(format!("{quantity}")),
                        )
                    },
                ),
            };
        vec![window]
    }

    fn ui_event(&mut self, event: Event) {
        match event {
            Event::OpenRequested => self.open = true,
            Event::Closed(_) => self.open = false,
            Event::Edited(edited) if edited.id == "search" => self.search = edited.value,
            _ => {}
        }
    }
}

angelica_sdk::plugin!(Bags);
