//! Messages between plugins: a plugin sends bytes under a name to a plugin
//! its manifest lists in `[messages] to`, and the host delivers them in the
//! next frame as a game event. A reply goes to the plugin that sent the
//! message it answers, listed or not. No plugin runs another's code: each
//! reads its messages in its own calls.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicU32, Ordering},
};

/// The most bytes of one message.
pub const MOST_PAYLOAD: usize = 1024 * 1024;
/// The most messages kept for one plugin between two frames.
pub const MOST_QUEUED: usize = 256;
/// The most bytes of a message's name.
pub const MOST_NAME: usize = 64;
/// How many sent messages are remembered for replies.
const REMEMBERED: usize = 4096;

/// A message as its receiver gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: u32,
    pub from: String,
    pub name: String,
    pub payload: Vec<u8>,
    /// The message this one answers.
    pub in_reply_to: Option<u32>,
}

/// What a plugin sends: to whom, under which name, and what.
pub struct Outgoing<'a> {
    pub to: &'a str,
    pub name: &'a str,
    pub payload: Vec<u8>,
    pub in_reply_to: Option<u32>,
}

#[derive(Default)]
struct Queues {
    /// Each plugin's messages for the next frame, and how many were left
    /// out.
    waiting: BTreeMap<String, (Vec<Message>, u32)>,
    /// The plugins running now, which can receive.
    running: BTreeSet<String>,
    /// Recent messages: id, sender, receiver; for checking replies.
    sent: VecDeque<(u32, String, String)>,
}

/// Every plugin's messages.
#[derive(Default)]
pub struct Messages {
    next: AtomicU32,
    queues: Mutex<Queues>,
}

impl Messages {
    /// Records that a plugin runs and can receive messages.
    pub fn started(&self, plugin: &str) {
        self.queues
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running
            .insert(plugin.to_owned());
    }

    /// Forgets a stopped plugin's messages; it receives no more.
    pub fn remove_plugin(&self, plugin: &str) {
        let mut queues = self.queues.lock().unwrap_or_else(PoisonError::into_inner);
        queues.running.remove(plugin);
        queues.waiting.remove(plugin);
    }

    /// Sends a message from `from`; `may_send_to` says whether its manifest
    /// lists and the player approved a receiver. Returns the message's id.
    ///
    /// # Errors
    /// `invalid-argument` for a name or payload that breaks the rules or a
    /// reply to a message `from` did not receive from `to`;
    /// `permission-denied` for a receiver not listed; `unavailable` for a
    /// receiver that does not run.
    pub fn send(
        &self,
        from: &str,
        message: Outgoing<'_>,
        may_send_to: &dyn Fn(&str) -> bool,
    ) -> Result<u32> {
        let Outgoing {
            to,
            name,
            payload,
            in_reply_to,
        } = message;
        if name.is_empty()
            || name.len() > MOST_NAME
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(Error::invalid(format!(
                "a message's name is 1 to {MOST_NAME} ASCII letters, digits, dots, underscores, and hyphens"
            )));
        }
        if payload.len() > MOST_PAYLOAD {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a message carries at most {MOST_PAYLOAD} bytes"),
            ));
        }
        let mut queues = self.queues.lock().unwrap_or_else(PoisonError::into_inner);
        match in_reply_to {
            Some(answered) => {
                let received = queues.sent.iter().any(|(id, sender, receiver)| {
                    *id == answered && sender == to && receiver == from
                });
                if !received {
                    return Err(Error::invalid(format!(
                        "message {answered} is not one the plugin received from {to}"
                    )));
                }
            }
            None if !may_send_to(to) => {
                return Err(Error::new(
                    Kind::PermissionDenied,
                    format!("{to} is not a plugin the manifest lists in [messages] to"),
                ));
            }
            None => {}
        }
        if !queues.running.contains(to) {
            return Err(Error::new(
                Kind::Unavailable,
                format!("plugin {to} is not running"),
            ));
        }
        let id = self.next.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        queues.sent.push_back((id, from.to_owned(), to.to_owned()));
        if queues.sent.len() > REMEMBERED {
            queues.sent.pop_front();
        }
        let (waiting, dropped) = queues.waiting.entry(to.to_owned()).or_default();
        if waiting.len() >= MOST_QUEUED {
            *dropped = dropped.saturating_add(1);
        } else {
            waiting.push(Message {
                id,
                from: from.to_owned(),
                name: name.to_owned(),
                payload,
                in_reply_to,
            });
        }
        Ok(id)
    }

    /// A plugin's messages since the last call, in the order they were
    /// sent, and how many were left out.
    pub fn take(&self, plugin: &str) -> (Vec<Message>, u32) {
        self.queues
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waiting
            .remove(plugin)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{Messages, Outgoing};
    use crate::error::Kind;

    fn outgoing(to: &str, in_reply_to: Option<u32>) -> Outgoing<'_> {
        Outgoing {
            to,
            name: "state",
            payload: b"{}".to_vec(),
            in_reply_to,
        }
    }

    #[test]
    fn messages_reach_listed_running_plugins_and_replies_their_senders() {
        let messages = Messages::default();
        let listed = |to: &str| to == "penumbra";
        let refused = messages.send("sync", outgoing("penumbra", None), &listed);
        assert_eq!(
            refused.unwrap_err().kind,
            Kind::Unavailable,
            "not running yet"
        );
        messages.started("penumbra");
        messages.started("sync");
        let id = messages
            .send("sync", outgoing("penumbra", None), &listed)
            .unwrap();
        let denied = messages.send("sync", outgoing("other", None), &listed);
        assert_eq!(denied.unwrap_err().kind, Kind::PermissionDenied);
        let (received, dropped) = messages.take("penumbra");
        assert_eq!((received.len(), dropped), (1, 0));
        assert_eq!(received[0].from, "sync");

        // Penumbra lists nobody, yet answers the message it received.
        let nobody = |_: &str| false;
        messages
            .send("penumbra", outgoing("sync", Some(id)), &nobody)
            .unwrap();
        assert!(
            messages
                .send("penumbra", outgoing("sync", Some(id + 7)), &nobody)
                .is_err()
        );
        assert_eq!(messages.take("sync").0[0].in_reply_to, Some(id));

        messages.remove_plugin("penumbra");
        let gone = messages.send("sync", outgoing("penumbra", None), &listed);
        assert_eq!(gone.unwrap_err().kind, Kind::Unavailable);
    }
}
