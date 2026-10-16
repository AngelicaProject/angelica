//! Notifications and popups of plugins, shown as notes at the top right of
//! the screen under the plugin's name, or as popups where the plugin asked.
#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::error::{Error, Kind, Result};

/// The most characters of one notification.
pub const MOST_CHARS: usize = 200;
/// How long a notification stays.
pub const SHOWN: Duration = Duration::from_secs(7);
/// The least time between two notifications of one plugin.
pub const SPACING: Duration = Duration::from_secs(2);
/// The most notifications shown at once, for all plugins.
pub const MOST_SHOWN: usize = 3;

/// A notification on the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub plugin: String,
    /// The line of a notification; empty for a popup.
    pub text: String,
    /// The parts of a popup and where it is.
    pub popup: Option<angelica_view::Popup>,
    /// Whether it asks for the player's attention.
    pub attention: bool,
    pub at: Instant,
    /// How long it stays.
    pub shown: Duration,
}

#[derive(Default)]
struct Shown {
    notifications: VecDeque<Notification>,
    /// When each plugin last notified.
    last: BTreeMap<String, Instant>,
    /// A notification asked for attention since the last frame.
    attention: bool,
}

#[derive(Default)]
pub struct Notifications {
    shown: Mutex<Shown>,
}

impl Notifications {
    /// Shows `text` from `plugin` at `now`; with `attention`, the game's
    /// button on the taskbar flashes while the game is in the background.
    ///
    /// # Errors
    /// Empty text, text over [`MOST_CHARS`] or with control characters, and
    /// a second notification of the plugin within [`SPACING`].
    pub fn show(&self, plugin: &str, text: &str, attention: bool, now: Instant) -> Result<()> {
        let chars = text.chars().count();
        if chars == 0 || chars > MOST_CHARS || text.chars().any(char::is_control) {
            return Err(Error::new(
                Kind::InvalidArgument,
                format!("a notification is one line of 1 to {MOST_CHARS} characters"),
            ));
        }
        self.add(Notification {
            plugin: plugin.to_owned(),
            text: text.to_owned(),
            popup: None,
            attention,
            at: now,
            shown: SHOWN,
        })
    }

    /// Shows a popup the plugin built, already checked, for its seconds.
    ///
    /// # Errors
    /// A second notification or popup of the plugin within [`SPACING`].
    pub fn pop_up(&self, plugin: &str, popup: angelica_view::Popup, now: Instant) -> Result<()> {
        self.add(Notification {
            plugin: plugin.to_owned(),
            text: String::new(),
            attention: popup.attention,
            shown: Duration::from_secs(u64::from(popup.seconds)),
            popup: Some(popup),
            at: now,
        })
    }

    fn add(&self, notification: Notification) -> Result<()> {
        let plugin = notification.plugin.as_str();
        let now = notification.at;
        let attention = notification.attention;
        let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        if shown
            .last
            .get(plugin)
            .is_some_and(|last| now.duration_since(*last) < SPACING)
        {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!(
                    "a plugin notifies at most once in {} seconds",
                    SPACING.as_secs()
                ),
            ));
        }
        shown.last.insert(plugin.to_owned(), now);
        shown.attention |= attention;
        shown.notifications.push_back(notification);
        while shown.notifications.len() > MOST_SHOWN {
            shown.notifications.pop_front();
        }
        Ok(())
    }

    /// The notifications shown at `now`, the newest first.
    pub fn current(&self, now: Instant) -> Vec<Notification> {
        let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        shown
            .notifications
            .retain(|notification| now.duration_since(notification.at) < notification.shown);
        shown.notifications.iter().rev().cloned().collect()
    }

    /// Whether a notification asked for attention since the last call.
    pub fn take_attention(&self) -> bool {
        std::mem::take(
            &mut self
                .shown
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .attention,
        )
    }

    /// Removes a stopped plugin's notifications.
    pub fn remove_plugin(&self, plugin: &str) {
        let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        shown
            .notifications
            .retain(|notification| notification.plugin != plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_are_spaced_few_and_brief() {
        let notifications = Notifications::default();
        let start = Instant::now();
        notifications.show("a", "pull in 10", true, start).unwrap();
        assert!(notifications.take_attention());
        assert!(!notifications.take_attention());
        assert_eq!(
            notifications
                .show("a", "pull in 5", false, start + Duration::from_secs(1))
                .unwrap_err()
                .kind,
            Kind::LimitExceeded
        );
        for (index, plugin) in ["b", "c", "d"].into_iter().enumerate() {
            notifications
                .show(plugin, plugin, false, start + Duration::from_secs(1))
                .unwrap_or_else(|error| panic!("{index}: {}", error.message));
        }
        let current = notifications.current(start + Duration::from_secs(2));
        let plugins: Vec<&str> = current.iter().map(|shown| shown.plugin.as_str()).collect();
        assert_eq!(plugins, ["d", "c", "b"], "the newest first, at most three");
        assert!(notifications.current(start + SHOWN * 2).is_empty());
        for text in ["", "two\nlines", &"x".repeat(MOST_CHARS + 1)] {
            assert_eq!(
                notifications
                    .show("e", text, false, start)
                    .unwrap_err()
                    .kind,
                Kind::InvalidArgument
            );
        }
        notifications
            .show("e", "stays", false, start + Duration::from_secs(3))
            .unwrap();
        notifications.remove_plugin("e");
        assert!(
            notifications
                .current(start + Duration::from_secs(3))
                .iter()
                .all(|shown| shown.plugin != "e")
        );
    }
}
