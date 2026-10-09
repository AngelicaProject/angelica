//! Game events plugins receive (the `events` interface): adapters record
//! what the game did in a bounded queue, and the host delivers each event
//! in the next frame to the plugins that subscribed to its kind. Platform
//! independent: the Windows adapters only push events.
#![forbid(unsafe_code)]
use crate::manifest::Permission;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Mutex, PoisonError, RwLock},
};

/// Events kept between two frames; more are counted as dropped.
pub const MAX_QUEUED: usize = 256;
/// Bytes of a message's sender or text an event carries; longer ones are
/// cut at this length.
pub const MAX_TEXT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    ChatMessage,
}

impl EventKind {
    /// The permission a plugin needs to subscribe.
    #[must_use]
    pub fn permission(self) -> Permission {
        match self {
            Self::ChatMessage => Permission::ChatRead,
        }
    }
}

/// A message the game added to the chat log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// The game's log information: the kind of message, a row of the
    /// `LogKind` sheet, in the low 7 bits; the relations of sender and target
    /// above.
    pub info: u16,
    /// The sender's name as game text; empty for a message without one.
    pub sender: Vec<u8>,
    pub message: Vec<u8>,
    /// Seconds since 1970, UTC.
    pub timestamp: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    ChatMessage(ChatMessage),
}

impl GameEvent {
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            Self::ChatMessage(_) => EventKind::ChatMessage,
        }
    }
}

#[derive(Default)]
struct Queue {
    events: VecDeque<GameEvent>,
    dropped: u32,
}

/// The events of one frame and the plugins that want them.
#[derive(Default)]
pub struct Events {
    subscriptions: RwLock<BTreeMap<String, BTreeSet<EventKind>>>,
    queue: Mutex<Queue>,
}

impl Events {
    /// Replaces a plugin's subscriptions; an empty set removes them.
    pub fn subscribe(&self, plugin: &str, kinds: BTreeSet<EventKind>) {
        let mut subscriptions = self
            .subscriptions
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if kinds.is_empty() {
            subscriptions.remove(plugin);
        } else {
            subscriptions.insert(plugin.to_owned(), kinds);
        }
    }

    pub fn remove_plugin(&self, plugin: &str) {
        self.subscribe(plugin, BTreeSet::new());
    }

    /// The kinds a plugin subscribed to.
    #[must_use]
    pub fn subscribed(&self, plugin: &str) -> BTreeSet<EventKind> {
        self.subscriptions
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(plugin)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether any plugin wants events of `kind`; adapters record nothing
    /// otherwise.
    #[must_use]
    pub fn wanted(&self, kind: EventKind) -> bool {
        self.subscriptions
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|kinds| kinds.contains(&kind))
    }

    /// Records an event for the next frame; past [`MAX_QUEUED`] it is
    /// counted as dropped.
    pub fn push(&self, event: GameEvent) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.events.len() < MAX_QUEUED {
            queue.events.push_back(event);
        } else {
            queue.dropped = queue.dropped.saturating_add(1);
        }
    }

    /// The events recorded since the last call, and how many were dropped.
    pub fn take(&self) -> (Vec<GameEvent>, u32) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let dropped = std::mem::take(&mut queue.dropped);
        (queue.events.drain(..).collect(), dropped)
    }
}

/// Text from the game for an event: at most [`MAX_TEXT`] bytes.
#[must_use]
pub fn text(bytes: &[u8]) -> Vec<u8> {
    bytes[..bytes.len().min(MAX_TEXT)].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> GameEvent {
        GameEvent::ChatMessage(ChatMessage {
            info: 10,
            sender: b"Alphinaud".to_vec(),
            message: text.as_bytes().to_vec(),
            timestamp: 1,
        })
    }

    #[test]
    fn events_wait_for_the_next_frame_and_overflow_is_counted() {
        let events = Events::default();
        assert!(!events.wanted(EventKind::ChatMessage));
        events.subscribe("p", [EventKind::ChatMessage].into());
        assert!(events.wanted(EventKind::ChatMessage));
        for index in 0..MAX_QUEUED + 3 {
            events.push(message(&index.to_string()));
        }
        let (taken, dropped) = events.take();
        assert_eq!(taken.len(), MAX_QUEUED);
        assert_eq!(taken[0], message("0"));
        assert_eq!(dropped, 3);
        assert_eq!(events.take(), (Vec::new(), 0));
        events.remove_plugin("p");
        assert!(!events.wanted(EventKind::ChatMessage));
    }
}
