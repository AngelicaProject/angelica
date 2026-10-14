//! Lens: plugins read the game's objects by paths through the game map's
//! object model (`docs/architecture/lens.md`). A query is checked and
//! planned once against the schema of the running build ([`prepare`]);
//! running it ([`run`]) only reads memory along the plan, which fails
//! instead of faulting. A watch is a query from an event of the map, read
//! when the game reaches the event's point ([`watch`]). Platform
//! independent: the Windows adapter only supplies the memory reads and the
//! hooks.
#![forbid(unsafe_code)]
pub mod act;
pub mod plan;
pub mod query;
pub mod read;
pub mod schema;
#[cfg(test)]
mod tests;
pub mod watch;

pub use plan::{Column, ColumnMeaning, Prepared, Stored, prepare, prepare_objects, prepare_watch};
pub use read::{Call, Memory, Row, Value, objects, run, run_call};
pub use schema::Schema;
pub use watch::{Delivery, Watches};

use crate::error::{Error, Kind, Result};
use std::sync::{
    Arc, PoisonError, RwLock,
    atomic::{AtomicU64, Ordering},
};

/// The most queries one plugin instance holds prepared.
pub const MOST_QUERIES: usize = 256;

/// The schema of the running build, which a reloaded map replaces.
#[derive(Default)]
pub struct Lens {
    schema: RwLock<Arc<Schema>>,
    generation: AtomicU64,
    /// The plugins' watches of events.
    pub watches: Watches,
    /// Whether plugins may act, and how often each acted.
    pub acting: act::Acting,
}

impl Lens {
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: RwLock::new(Arc::new(schema)),
            generation: AtomicU64::new(0),
            watches: Watches::default(),
            acting: act::Acting::default(),
        }
    }

    /// The current schema and its generation, which changes with each
    /// replacement.
    #[must_use]
    pub fn schema(&self) -> (Arc<Schema>, u64) {
        let schema = self
            .schema
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (schema, self.generation.load(Ordering::Acquire))
    }

    /// Takes `schema` in place of the current one; prepared queries are
    /// planned again at their next run, watches now.
    pub fn replace(&self, schema: Schema) {
        self.watches.replan(&schema);
        *self.schema.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(schema);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Starts a plugin's watch, prepared under `handle`: the event's hook
    /// is installed through `hook` (with the point's id, the event's map
    /// entry, and its address) the first time the event is watched.
    ///
    /// # Errors
    /// `unavailable` when the hook cannot be installed.
    pub fn watch(
        &self,
        plugin: &str,
        handle: u32,
        text: &str,
        read: std::collections::BTreeSet<String>,
        prepared: Prepared,
        hook: &dyn Fn(usize, &str, u64) -> std::result::Result<(), String>,
    ) -> Result<()> {
        let (schema, _) = self.schema();
        let Some(event) = prepared
            .event()
            .and_then(|name| schema.objects.events.get(name))
        else {
            return Err(Error::invalid("a watch starts at an event"));
        };
        let address = schema
            .events
            .get(&event.name)
            .cloned()
            .unwrap_or_else(|| Err("not in the schema".into()))
            .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
        let (id, hooked) = self.watches.point(&event.at);
        if !hooked {
            hook(id, &event.at, address).map_err(|reason| {
                Error::new(
                    Kind::Unavailable,
                    format!("event {} cannot be watched: {reason}", event.name),
                )
            })?;
            self.watches.hooked(id);
        }
        let subscriber = watch::Subscriber::new(
            plugin,
            handle,
            text,
            read,
            &event.name,
            event.rate,
            prepared,
        );
        self.watches.subscribe(id, subscriber);
        Ok(())
    }

    /// A domain of the map, for what the player is told.
    #[must_use]
    pub fn domain(&self, name: &str) -> Option<angelica_map::Domain> {
        self.schema().0.objects.domains.get(name).cloned()
    }
}

/// A query a plugin prepared, planned again when the schema changes.
struct Held {
    text: String,
    generation: u64,
    planned: Result<Prepared>,
    /// A watch's rows arrive as events; it is never run.
    watch: bool,
}

/// The queries one plugin instance prepared, by handle.
#[derive(Default)]
pub struct Queries {
    held: Vec<Option<Held>>,
}

impl Queries {
    /// Prepares `text` and keeps it under a new handle.
    ///
    /// # Errors
    /// The planner's errors, or `limit-exceeded` with [`MOST_QUERIES`] held.
    pub fn prepare(
        &mut self,
        lens: &Lens,
        text: &str,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<(u32, Prepared)> {
        let (schema, generation) = lens.schema();
        let prepared = prepare(&schema, text, permitted)?;
        self.hold(text, generation, prepared, false)
    }

    /// Prepares a watch of `text` and keeps it under a new handle; the
    /// caller starts it with [`Lens::watch`].
    ///
    /// # Errors
    /// As for [`Queries::prepare`].
    pub fn watch(
        &mut self,
        lens: &Lens,
        text: &str,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<(u32, Prepared)> {
        let (schema, generation) = lens.schema();
        let prepared = prepare_watch(&schema, text, permitted)?;
        self.hold(text, generation, prepared, true)
    }

    fn hold(
        &mut self,
        text: &str,
        generation: u64,
        prepared: Prepared,
        watch: bool,
    ) -> Result<(u32, Prepared)> {
        let slot = match self.held.iter().position(Option::is_none) {
            Some(slot) => slot,
            None if self.held.len() < MOST_QUERIES => {
                self.held.push(None);
                self.held.len() - 1
            }
            None => {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!(
                        "a plugin holds at most {MOST_QUERIES} prepared queries; release the ones it no longer runs"
                    ),
                ));
            }
        };
        self.held[slot] = Some(Held {
            text: text.to_owned(),
            generation,
            planned: Ok(prepared.clone()),
            watch,
        });
        Ok((u32::try_from(slot).unwrap_or(u32::MAX), prepared))
    }

    /// The plan of a held query for the current schema.
    ///
    /// # Errors
    /// `not-found` for an unknown handle; the planner's errors when the
    /// query no longer fits the reloaded map.
    pub fn plan(
        &mut self,
        lens: &Lens,
        handle: u32,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<Prepared> {
        let held = usize::try_from(handle)
            .ok()
            .and_then(|slot| self.held.get_mut(slot))
            .and_then(Option::as_mut)
            .ok_or_else(|| Error::new(Kind::NotFound, format!("no prepared query {handle}")))?;
        if held.watch {
            return Err(Error::invalid(format!(
                "query {handle} is a watch: its rows arrive as events"
            )));
        }
        let (schema, generation) = lens.schema();
        if held.generation != generation {
            held.planned = prepare(&schema, &held.text, permitted);
            held.generation = generation;
        }
        held.planned.clone()
    }

    /// Forgets a held query; an unknown handle is ignored. True when it
    /// was a watch, which the caller ends.
    pub fn release(&mut self, handle: u32) -> bool {
        usize::try_from(handle)
            .ok()
            .and_then(|slot| self.held.get_mut(slot))
            .and_then(Option::take)
            .is_some_and(|held| held.watch)
    }
}
