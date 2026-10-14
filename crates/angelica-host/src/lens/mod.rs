//! Lens: plugins read the game's objects by paths through the game map's
//! object model (`docs/architecture/lens.md`). A query is checked and
//! planned once against the schema of the running build ([`prepare`]);
//! running it ([`run`]) only reads memory along the plan, which fails
//! instead of faulting. Platform independent: the Windows adapter only
//! supplies the memory reads.
#![forbid(unsafe_code)]
pub mod plan;
pub mod query;
pub mod read;
pub mod schema;
#[cfg(test)]
mod tests;

pub use plan::{Column, ColumnMeaning, Prepared, Stored, prepare};
pub use read::{Memory, Row, Value, run};
pub use schema::Schema;

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
}

impl Lens {
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: RwLock::new(Arc::new(schema)),
            generation: AtomicU64::new(0),
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
    /// planned again at their next run.
    pub fn replace(&self, schema: Schema) {
        *self.schema.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(schema);
        self.generation.fetch_add(1, Ordering::AcqRel);
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
        let (schema, generation) = lens.schema();
        if held.generation != generation {
            held.planned = prepare(&schema, &held.text, permitted);
            held.generation = generation;
        }
        held.planned.clone()
    }

    /// Forgets a held query; an unknown handle is ignored.
    pub fn release(&mut self, handle: u32) {
        if let Some(slot) = usize::try_from(handle)
            .ok()
            .and_then(|slot| self.held.get_mut(slot))
        {
            *slot = None;
        }
    }
}
